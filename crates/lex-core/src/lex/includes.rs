//! Include resolution for Lex documents.
//!
//! This module turns `:: lex.include src="..." ::` annotations into spliced
//! content from the referenced files. It is *opt-in*: callers that want the
//! unresolved tree (the formatter, tree-sitter parity, editor tooling that
//! displays include statements as authored) skip this pass entirely. The
//! parser itself never touches the filesystem — all I/O goes through the
//! injected [`Loader`] trait.
//!
//! See `comms/specs/proposals/includes.lex` for the full design.
//!
//! # Status
//!
//! This module is being built up across PRs 3–6:
//!
//! - PR 3: skeleton — trait, config, errors, stub.
//! - PR 4: single-pass splice + container-policy validation +
//!   doc-title/doc-annotation conversion + origin stamping + root-escape
//!   check.
//! - PR 5: recursive resolution into included files + cycle detection
//!   (chain stack) + depth limit. Each loaded file gets walked in its OWN
//!   directory, so relative paths inside an included file resolve from
//!   that file's directory, not the entry's.
//! - PR 6: origin-aware reference helpers. [`resolve_file_reference`]
//!   resolves a `ReferenceType::File` target from the authoring file's
//!   directory using `Range.origin_path`.
//!   `Document::find_annotation_by_label_in_origin` scopes footnote
//!   lookups to the file the reference was authored in.
//! - PR 7 (this PR): [`FsLoader`] — production loader that reads from the
//!   filesystem with `std::fs::read_to_string`. CLI wires the resolver
//!   into `lex convert` and `lex inspect` (default-on, opt-out via
//!   `--no-includes`); `lex format` never expands.
//!
//! # Layering
//!
//! Of all of lex-core, only [`FsLoader`] references `std::fs`. The
//! resolver itself does no I/O — it always goes through the [`Loader`]
//! trait. Callers can swap loaders to keep the resolver sandboxed:
//!
//! - The LSP wraps [`FsLoader`] with file-watch invalidation (PR 8).
//! - WASM builds provide a JS-backed loader instead of [`FsLoader`].
//! - Tests use [`MemoryLoader`] (gated behind `test-support`).
//!
//! For tests, lex-core itself ships [`MemoryLoader`] gated behind the
//! `test-support` cargo feature. It is not intended for production use.

use crate::lex::assembling::AttachAnnotations;
use crate::lex::ast::elements::container::GeneralContainer;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::paragraph::Paragraph;
use crate::lex::ast::elements::session::Session;
use crate::lex::ast::range::Range;
use crate::lex::ast::Document;
use crate::lex::transforms::Runnable;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Configuration for the include resolution pass.
#[derive(Debug, Clone)]
pub struct ResolveConfig {
    /// Directory all include paths resolve under. Any include that
    /// canonicalizes outside this root is a [`IncludeError::RootEscape`].
    ///
    /// Must be an **absolute** path. Lexical normalization treats `.`
    /// and `..` against an empty buffer as no-ops; passing a relative
    /// or unnormalized root weakens the root-escape prefix check.
    /// Callers (CLI, LSP) should canonicalize the root before
    /// constructing `ResolveConfig`.
    pub root: PathBuf,
    /// Maximum include depth. Default 8 (see [`ResolveConfig::DEFAULT_MAX_DEPTH`]).
    /// Hitting the limit is an error, not a silent truncation.
    pub max_depth: usize,
    /// Maximum total number of `lex.include` annotations resolved across
    /// the whole tree (depth × breadth). Default 1000
    /// (see [`ResolveConfig::DEFAULT_MAX_TOTAL_INCLUDES`]).
    ///
    /// Caps fan-out: `max_depth` alone bounds chain length but not
    /// breadth. A document with 100 thousand top-level includes at depth
    /// 1 sits inside `max_depth` but can still OOM the resolver / LSP /
    /// CI. Hitting this limit is an error, not a silent truncation.
    pub max_total_includes: usize,
}

impl ResolveConfig {
    /// Default maximum include depth — enough for any reasonable atomization
    /// strategy (aggregator → per-chapter → per-section), bounded enough to
    /// keep the resolver's worst-case work predictable.
    pub const DEFAULT_MAX_DEPTH: usize = 8;

    /// Default maximum total include count (DoS bound). Generous enough
    /// for a book-length document with thousands of small fragments,
    /// tight enough to contain adversarial fan-out within a few seconds
    /// of resolver work.
    pub const DEFAULT_MAX_TOTAL_INCLUDES: usize = 1000;

    /// Construct a config with the given root and default limits.
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            root,
            max_depth: Self::DEFAULT_MAX_DEPTH,
            max_total_includes: Self::DEFAULT_MAX_TOTAL_INCLUDES,
        }
    }
}

/// A pluggable source-text loader.
///
/// Implementations decide where bytes come from (filesystem, in-memory map,
/// virtual filesystem, content-addressed store, …). lex-core never references
/// `std::fs` directly through this trait; that keeps the resolver pure and
/// usable in WASM, sandboxes, and unit tests.
pub trait Loader {
    /// Load the source text for `path` and return both the contents and a
    /// canonical identity for the loaded resource. The path is what the
    /// resolver decided on after applying the rules in §4 of the proposal.
    ///
    /// `LoadedFile::canonical_path` is the loader's authoritative identity
    /// for the resource. For [`FsLoader`] this is the filesystem-canonical
    /// path (symlinks resolved, case-folded if the underlying FS is
    /// case-insensitive); for [`MemoryLoader`] it's the lookup key (since
    /// memory loaders have no symlinks). The resolver uses this for cycle
    /// detection and for stamping `Range.origin_path` on the loaded tree.
    fn load(&self, path: &Path) -> Result<LoadedFile, LoadError>;
}

/// Result of a successful [`Loader::load`].
#[derive(Debug, Clone)]
pub struct LoadedFile {
    /// The file's source text.
    pub source: String,
    /// The loader's authoritative identity for the resource. See
    /// [`Loader::load`] for how loaders decide this.
    pub canonical_path: PathBuf,
}

/// Errors a [`Loader`] can produce.
#[derive(Debug, Clone)]
pub enum LoadError {
    /// The loader could not find a resource at the given path.
    NotFound { path: PathBuf },
    /// The resource exists but resolves outside the loader's allowed
    /// boundary. The lexical resolver normalizes `..` in the requested
    /// path, but loaders that touch a real filesystem must do a second
    /// check post-canonicalization to catch symlinks that escape the
    /// boundary lexically-correct paths can't reach.
    OutsideRoot { path: PathBuf, root: PathBuf },
    /// The resource exists but its size exceeds the loader's configured
    /// limit. `size` and `limit` are in bytes. The resolver maps this to
    /// [`IncludeError::FileTooLarge`] with the offending annotation's site.
    TooLarge {
        path: PathBuf,
        size: u64,
        limit: u64,
    },
    /// Underlying I/O error (or virtual-filesystem equivalent).
    Io { path: PathBuf, message: String },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::NotFound { path } => write!(f, "include not found: {}", path.display()),
            LoadError::OutsideRoot { path, root } => write!(
                f,
                "include path {} resolves outside loader root {}",
                path.display(),
                root.display()
            ),
            LoadError::TooLarge { path, size, limit } => write!(
                f,
                "include file {} is {size} bytes, exceeds limit of {limit} bytes",
                path.display()
            ),
            LoadError::Io { path, message } => {
                write!(f, "io error reading {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// Errors the include resolver can produce.
#[derive(Debug, Clone)]
pub enum IncludeError {
    /// An include chain looped back on itself. `chain` is the resolution
    /// stack at the moment the duplicate `path` was about to be pushed,
    /// in source-order (entry first, deepest last). `include_site` is the
    /// range of the offending `lex.include` annotation in its host file —
    /// useful for diagnostics that highlight the exact line.
    Cycle {
        include_site: Range,
        path: PathBuf,
        chain: Vec<PathBuf>,
    },
    /// The include depth exceeded [`ResolveConfig::max_depth`]. `chain`
    /// shows the resolution stack at the moment of failure, in source
    /// order. `include_site` is the range of the offending
    /// `lex.include` annotation in its host file.
    DepthExceeded {
        include_site: Range,
        limit: usize,
        chain: Vec<PathBuf>,
    },
    /// The total number of includes resolved across the document
    /// exceeded [`ResolveConfig::max_total_includes`]. Bounds adversarial
    /// fan-out (which `max_depth` alone does not). `include_site` is the
    /// `lex.include` annotation that pushed the count past the limit.
    TotalIncludesExceeded { include_site: Range, limit: usize },
    /// The included file's size exceeded the loader's configured limit.
    /// Surfaced by loaders that read from a real filesystem (FsLoader)
    /// to bound memory allocation per include. `include_site` is the
    /// offending annotation; `size` and `limit` are in bytes.
    FileTooLarge {
        include_site: Range,
        path: PathBuf,
        size: u64,
        limit: u64,
    },
    /// A path resolved outside the configured [`ResolveConfig::root`].
    RootEscape { path: PathBuf, root: PathBuf },
    /// The loader could not find or read the included file. `include_site`
    /// is the range of the offending `lex.include` annotation in its host
    /// file, so editors can squiggle the line that asked for the missing
    /// file rather than the document head.
    NotFound { include_site: Range, path: PathBuf },
    /// The loader returned text that the parser rejected.
    ParseFailed { path: PathBuf, message: String },
    /// The included file's content is not legal in the include site's
    /// parent container.
    ///
    /// Today this only occurs when an included file has top-level Sessions
    /// and the include site is inside a `GeneralContainer` (Definition,
    /// ListItem, or another Annotation's body). The `violation` field
    /// names the offending content kind (e.g. `"Sessions"`) so future
    /// container/policy combinations can reuse this variant without a
    /// breaking change.
    ContainerPolicy {
        include_site: Range,
        container: &'static str,
        file: PathBuf,
        violation: &'static str,
    },
    /// Loader propagated a non-`NotFound` I/O error.
    LoaderIo { path: PathBuf, message: String },
    /// `lex.include` annotation was missing the mandatory `src=` parameter.
    MissingSrc { include_site: Range },
}

impl std::fmt::Display for IncludeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IncludeError::Cycle { path, chain, .. } => {
                let chain_display: Vec<String> =
                    chain.iter().map(|p| p.display().to_string()).collect();
                write!(
                    f,
                    "include cycle: {} (chain: {})",
                    path.display(),
                    chain_display.join(" -> ")
                )
            }
            IncludeError::DepthExceeded { limit, chain, .. } => {
                let chain_display: Vec<String> =
                    chain.iter().map(|p| p.display().to_string()).collect();
                write!(
                    f,
                    "include depth exceeded limit of {limit} (chain: {})",
                    chain_display.join(" -> ")
                )
            }
            IncludeError::TotalIncludesExceeded { limit, .. } => {
                write!(f, "total include count exceeded limit of {limit}")
            }
            IncludeError::FileTooLarge {
                path, size, limit, ..
            } => {
                write!(
                    f,
                    "included file {} is {size} bytes, exceeds limit of {limit} bytes",
                    path.display()
                )
            }
            IncludeError::RootEscape { path, root } => write!(
                f,
                "include path {} escapes resolution root {}",
                path.display(),
                root.display()
            ),
            IncludeError::NotFound { path, .. } => {
                write!(f, "include not found: {}", path.display())
            }
            IncludeError::ParseFailed { path, message } => {
                write!(f, "failed to parse {}: {message}", path.display())
            }
            IncludeError::ContainerPolicy {
                container,
                file,
                violation,
                ..
            } => write!(
                f,
                "included file {} contains {} but include site is inside {} \
                 (which does not allow {})",
                file.display(),
                violation,
                container,
                violation
            ),
            IncludeError::LoaderIo { path, message } => {
                write!(f, "loader error reading {}: {message}", path.display())
            }
            IncludeError::MissingSrc { .. } => {
                write!(f, "lex.include annotation missing required src= parameter")
            }
        }
    }
}

impl std::error::Error for IncludeError {}

// No `From<LoadError>` impl: `IncludeError::NotFound` carries the include
// site (the `lex.include` annotation's range), which a loader doesn't know
// about. Callers map `LoadError` explicitly at the call site, where the
// site is available.

/// Which container the include site sits in. Determines the splice-time
/// policy check (the only one today is "no Sessions in `GeneralContainer`").
#[derive(Debug, Clone, Copy)]
enum ContainerKind {
    /// `Document.root.children` or `Session.children` — accepts everything.
    Session,
    /// `Definition.children` — `GeneralContainer`.
    Definition,
    /// `Annotation.children` — `GeneralContainer`.
    AnnotationBody,
    /// `ListItem.children` — `GeneralContainer`.
    ListItem,
}

impl ContainerKind {
    fn name(self) -> &'static str {
        match self {
            ContainerKind::Session => "Session",
            ContainerKind::Definition => "Definition",
            ContainerKind::AnnotationBody => "Annotation body",
            ContainerKind::ListItem => "ListItem",
        }
    }

    fn allows_sessions(self) -> bool {
        matches!(self, ContainerKind::Session)
    }
}

/// Resolve `:: lex.include ::` annotations starting from `source`, recursively.
///
/// `source_path` identifies the entry-point file. It is used to (a) resolve
/// relative include paths against the entry file's directory, (b) stamp
/// `Range.origin_path` on every node so downstream code (file-ref resolution,
/// diagnostics, LSP goto) can report locations against the authoring file,
/// and (c) seed the cycle-detection chain so an include cycle that loops
/// back to the entry is caught. When `None`, relative paths resolve against
/// `config.root`, origin stamping is skipped on the entry, and the chain
/// starts empty.
///
/// # Pre/post-attachment
///
/// Internally this re-parses each source (entry + every loaded file) *without*
/// annotation attachment so `lex.include` annotations are visible as standalone
/// children where the splice can replace them in-place. After all splices,
/// [`AttachAnnotations`] runs once on the merged tree, which lands the include
/// annotation on the first spliced node by the standard "attach to next
/// sibling" rule. This matches the textual paste mental model from the proposal.
///
/// # Recursion
///
/// Each loaded file is fully resolved (its own includes replaced) *before*
/// being spliced into the host. The recursion uses each file's own directory
/// as `host_dir`, so a relative path inside an included file resolves from
/// that file's location — not the entry's. An active-chain stack of
/// canonicalized paths gates against cycles; the depth counter gates against
/// pathological nesting (default 8, configurable via [`ResolveConfig::max_depth`]).
pub fn resolve_from_source(
    source: &str,
    source_path: Option<PathBuf>,
    config: &ResolveConfig,
    loader: &dyn Loader,
) -> Result<Document, IncludeError> {
    let entry_origin = source_path.as_ref().map(|p| Arc::new(p.clone()));
    let host_dir = source_path
        .as_ref()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| config.root.clone());

    let mut doc = parse_no_attach(source).map_err(|message| IncludeError::ParseFailed {
        path: source_path.clone().unwrap_or_default(),
        message,
    })?;

    if let Some(origin) = entry_origin.as_ref() {
        stamp_doc(&mut doc, origin);
    }

    // Seed the chain with the lexically-normalized entry path (when known)
    // so an include that loops back to the entry is detected as a cycle.
    // Normalization here is essential — `target_path` values produced by
    // `resolve_path` are also lexically normalized, so an unnormalized
    // entry would never compare equal to its normalized self.
    let mut chain: Vec<PathBuf> = source_path
        .as_ref()
        .map(|p| vec![lexical_normalize(p)])
        .unwrap_or_default();
    let mut state = ResolverState {
        config,
        loader,
        chain: &mut chain,
        depth: 0,
        total_resolved: 0,
    };

    splice_in_session_container(doc.root.children.as_mut_vec(), &host_dir, &mut state)?;

    let doc = AttachAnnotations::new()
        .run(doc)
        .map_err(|e| IncludeError::ParseFailed {
            path: source_path.unwrap_or_default(),
            message: format!("annotation attachment failed: {e}"),
        })?;

    Ok(doc)
}

// ============================================================================
// Splicing
// ============================================================================

/// Per-resolution state threaded through the recursive walker. Keeps the
/// signatures of the splice/process functions short and ensures
/// `chain`/`depth` are updated in lock-step (push/pop, +1/back-out) at
/// each include site.
struct ResolverState<'a> {
    config: &'a ResolveConfig,
    loader: &'a dyn Loader,
    /// Active resolution stack: lexically-normalized absolute paths
    /// currently being resolved. Pushed when we begin loading a file and
    /// popped when its tree is fully resolved. A push that finds the
    /// path already on the stack is a cycle.
    ///
    /// Normalization (not filesystem canonicalization) is what's used
    /// here: the resolver never touches `std::fs`, so symlink resolution
    /// is out. Two paths that lexically refer to the same file (after
    /// `.`/`..` collapse) compare equal; two paths reaching the same
    /// inode via different routes do not. For real-FS use cases this is
    /// fine because `FsLoader` will canonicalize on load before the
    /// chain comparison sees the path.
    chain: &'a mut Vec<PathBuf>,
    /// Number of include hops from the entry point. Each recursion into a
    /// loaded file increments by 1. Hitting `config.max_depth` is an error.
    depth: usize,
    /// Total includes resolved across the entire walk (depth × breadth).
    /// Incremented on every successful load. Hitting
    /// `config.max_total_includes` aborts with `TotalIncludesExceeded` —
    /// caps adversarial fan-out that `max_depth` alone wouldn't catch.
    total_resolved: usize,
}

fn splice_in_session_container(
    children: &mut Vec<ContentItem>,
    host_dir: &Path,
    state: &mut ResolverState<'_>,
) -> Result<(), IncludeError> {
    // Post-order: recurse into nested containers first, splice this
    // container's includes second. The recurse step walks the *original*
    // tree; the splice step inserts already-fully-resolved content
    // (recursion happens inside `process_includes`), which is therefore
    // never re-walked.
    recurse_into_children(children, host_dir, state)?;
    process_includes(children, host_dir, state, ContainerKind::Session)
}

fn splice_in_general_container(
    container: &mut GeneralContainer,
    host_dir: &Path,
    state: &mut ResolverState<'_>,
    kind: ContainerKind,
) -> Result<(), IncludeError> {
    recurse_into_children(container.as_mut_vec(), host_dir, state)?;
    process_includes(container.as_mut_vec(), host_dir, state, kind)
}

// Allow &mut Vec because `splice` needs Vec-specific operations.
#[allow(clippy::ptr_arg)]
fn process_includes(
    children: &mut Vec<ContentItem>,
    host_dir: &Path,
    state: &mut ResolverState<'_>,
    kind: ContainerKind,
) -> Result<(), IncludeError> {
    // Collect indices of standalone include annotations in this container.
    let include_indices: Vec<usize> = children
        .iter()
        .enumerate()
        .filter_map(|(i, item)| match item {
            ContentItem::Annotation(a) if a.is_include() => Some(i),
            _ => None,
        })
        .collect();

    // Process in reverse order so earlier indices stay valid.
    for i in include_indices.into_iter().rev() {
        let annotation = match &children[i] {
            ContentItem::Annotation(a) => a.clone(),
            _ => unreachable!("index came from include filter"),
        };

        let splice_items = resolve_one_include(&annotation, host_dir, state, kind)?;

        // Replace the include annotation with the splice content.
        // The annotation itself stays in the children list immediately
        // before the splice, so the post-resolution AttachAnnotations
        // pass moves it onto the first spliced node by the standard
        // "attach to next sibling" rule.
        let mut replacement = Vec::with_capacity(splice_items.len() + 1);
        replacement.push(ContentItem::Annotation(annotation));
        replacement.extend(splice_items);
        children.splice(i..=i, replacement);
    }

    Ok(())
}

/// Resolve a single include annotation: path → load → parse → recurse →
/// stamp → policy-check → splice list.
///
/// The recursion happens *here*: after parsing the loaded file, we walk
/// its tree with the loaded file's own directory as `host_dir`, with the
/// loaded file pushed onto `state.chain` and `state.depth` bumped by 1.
/// When this call returns, the splice list is fully resolved and ready to
/// be inserted into the host container.
fn resolve_one_include(
    annotation: &crate::lex::ast::elements::annotation::Annotation,
    host_dir: &Path,
    state: &mut ResolverState<'_>,
    parent_kind: ContainerKind,
) -> Result<Vec<ContentItem>, IncludeError> {
    let src = annotation
        .include_src()
        .ok_or_else(|| IncludeError::MissingSrc {
            include_site: annotation.location.clone(),
        })?;

    let target_path = resolve_path(&src, host_dir, &state.config.root)?;

    // Depth check before any FS access. A site sitting exactly at
    // `max_depth` is fine; one that would push us *past* it is the
    // failure case.
    if state.depth >= state.config.max_depth {
        return Err(IncludeError::DepthExceeded {
            include_site: annotation.location.clone(),
            limit: state.config.max_depth,
            chain: state.chain.clone(),
        });
    }

    // Total-count check before loading. Caps fan-out — a doc with
    // 100k top-level includes would blow past max_total_includes long
    // before max_depth would catch anything.
    if state.total_resolved >= state.config.max_total_includes {
        return Err(IncludeError::TotalIncludesExceeded {
            include_site: annotation.location.clone(),
            limit: state.config.max_total_includes,
        });
    }

    // Load via the injected loader. The loader returns the source plus
    // a *canonical* identity for the resource — for FsLoader that's
    // post-`fs::canonicalize` (symlinks resolved, case-folded on
    // case-insensitive FS); for MemoryLoader it's the lookup key. We
    // use the canonical path for cycle detection so a symlink loop or
    // a case-folded re-include is caught here rather than slipping
    // through to `max_depth`.
    let LoadedFile {
        source: target_source,
        canonical_path,
    } = state.loader.load(&target_path).map_err(|e| match e {
        LoadError::NotFound { path } => IncludeError::NotFound {
            include_site: annotation.location.clone(),
            path,
        },
        LoadError::OutsideRoot { path, root } => IncludeError::RootEscape { path, root },
        LoadError::TooLarge { path, size, limit } => IncludeError::FileTooLarge {
            include_site: annotation.location.clone(),
            path,
            size,
            limit,
        },
        LoadError::Io { path, message } => IncludeError::LoaderIo { path, message },
    })?;
    state.total_resolved += 1;

    // Cycle check uses the canonical path so symlink/case-fold cycles
    // are caught even though `target_path` (which we used for the load
    // request) was just lexically resolved.
    if state.chain.iter().any(|p| p == &canonical_path) {
        return Err(IncludeError::Cycle {
            include_site: annotation.location.clone(),
            path: canonical_path,
            chain: state.chain.clone(),
        });
    }

    let mut included =
        parse_no_attach(&target_source).map_err(|message| IncludeError::ParseFailed {
            path: canonical_path.clone(),
            message,
        })?;

    let target_origin = Arc::new(canonical_path.clone());
    stamp_doc(&mut included, &target_origin);

    // Recursively resolve includes inside the loaded file. The host_dir
    // for that walk is the loaded file's own canonical parent; the
    // chain gains the canonical path and depth bumps by 1 — both are
    // popped/restored on the way back so siblings see the same state.
    let included_dir = canonical_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| state.config.root.clone());

    state.chain.push(canonical_path.clone());
    let saved_depth = state.depth;
    state.depth = saved_depth + 1;
    let recurse_result =
        splice_in_session_container(included.root.children.as_mut_vec(), &included_dir, state);
    state.depth = saved_depth;
    state.chain.pop();
    recurse_result?;

    let splice_items = prepare_splice_list(included);
    validate_against_kind(
        &splice_items,
        parent_kind,
        &annotation.location,
        &canonical_path,
    )?;

    Ok(splice_items)
}

#[allow(clippy::ptr_arg)]
fn recurse_into_children(
    children: &mut Vec<ContentItem>,
    host_dir: &Path,
    state: &mut ResolverState<'_>,
) -> Result<(), IncludeError> {
    for item in children.iter_mut() {
        match item {
            ContentItem::Session(s) => {
                splice_in_session_container(s.children.as_mut_vec(), host_dir, state)?;
            }
            ContentItem::Definition(d) => {
                splice_in_general_container(
                    &mut d.children,
                    host_dir,
                    state,
                    ContainerKind::Definition,
                )?;
            }
            ContentItem::Annotation(a) if !a.is_include() => {
                splice_in_general_container(
                    &mut a.children,
                    host_dir,
                    state,
                    ContainerKind::AnnotationBody,
                )?;
            }
            ContentItem::List(l) => {
                for li in l.items.as_mut_vec().iter_mut() {
                    if let ContentItem::ListItem(item) = li {
                        splice_in_general_container(
                            &mut item.children,
                            host_dir,
                            state,
                            ContainerKind::ListItem,
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn prepare_splice_list(mut included: Document) -> Vec<ContentItem> {
    let mut items: Vec<ContentItem> = Vec::new();

    // Document title → Paragraph, prepended.
    // Equivalent to what a textual paste would parse (an unindented line
    // becomes a paragraph in the host's context). Per the revised
    // spec §5.2 this is "do nothing" semantics — converting matches what
    // the parser would do if the included source were inlined and reparsed.
    if let Some(title) = included.title {
        let location = title.location.clone();
        let para = Paragraph::from_line(title.as_str().to_string()).at(location);
        items.push(ContentItem::Paragraph(para));
    }

    // Document-level annotations → regular annotations, prepended.
    for ann in included.annotations {
        items.push(ContentItem::Annotation(ann));
    }

    // Body of the included document.
    items.append(included.root.children.as_mut_vec());

    items
}

fn validate_against_kind(
    items: &[ContentItem],
    kind: ContainerKind,
    site: &Range,
    file: &Path,
) -> Result<(), IncludeError> {
    if kind.allows_sessions() {
        return Ok(());
    }
    if items.iter().any(|i| matches!(i, ContentItem::Session(_))) {
        return Err(IncludeError::ContainerPolicy {
            include_site: site.clone(),
            container: kind.name(),
            file: file.to_path_buf(),
            violation: "Sessions",
        });
    }
    Ok(())
}

// ============================================================================
// Path resolution
// ============================================================================

/// Resolve a file-reference target string the same way the include
/// resolver resolves include paths.
///
/// Use this when consuming `ReferenceType::File { target }` (or any other
/// node-attached path) so that relative paths resolve from the *authoring*
/// file's directory, not from wherever the merged document happens to be
/// rooted. Pass `ref_origin` as the [`Range::origin_path`] of the inline's
/// containing node (or `None` if the node was never stamped — in that case
/// the path is treated as if authored at the root).
///
/// Behaviour matches the include resolver:
/// - Root-absolute targets (leading `/`) resolve under `root`.
/// - Other targets resolve relative to `ref_origin`'s parent (or `root`
///   when `ref_origin` is `None`).
/// - The result is lexically normalized and checked against `root` —
///   paths that escape it return `RootEscape`.
///
/// This is a sister to the resolver's internal `resolve_path` and shares
/// the same lexical-normalization caveat: it does not touch the filesystem.
pub fn resolve_file_reference(
    target: &str,
    ref_origin: Option<&Path>,
    root: &Path,
) -> Result<PathBuf, IncludeError> {
    let host_dir: PathBuf = ref_origin
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());
    resolve_path(target, &host_dir, root)
}

fn resolve_path(src: &str, host_dir: &Path, root: &Path) -> Result<PathBuf, IncludeError> {
    let candidate = if let Some(rel) = src.strip_prefix('/') {
        // Root-absolute: leading slash means "from the resolution root".
        root.join(rel)
    } else {
        // Relative: from the host file's directory.
        host_dir.join(src)
    };
    let normalized = lexical_normalize(&candidate);
    let canonical_root = lexical_normalize(root);
    if !normalized.starts_with(&canonical_root) {
        return Err(IncludeError::RootEscape {
            path: normalized,
            root: canonical_root,
        });
    }
    Ok(normalized)
}

/// Lexical (no-filesystem) path normalization: resolve `.` and `..` components.
///
/// Filesystem-based canonicalization (`std::fs::canonicalize`) requires the
/// path to exist, which breaks tests that use [`MemoryLoader`]. The lexical
/// version is sufficient for include-site path resolution because the
/// resolver only needs a stable identity for cycle detection and a uniform
/// shape for the root-escape prefix check.
///
/// `..` is collapsed only when the *last* component in the buffer is a
/// real directory name (`Component::Normal`). When the buffer is empty
/// or its last component is itself `..` (or a root marker), the new `..`
/// is *preserved* in the buffer.
///
/// This is what defeats `../../etc/passwd` from collapsing to
/// `etc/passwd` and bypassing the root-escape check — `PathBuf::pop`
/// would happily strip a `..` (since `Path::new("..").parent()` returns
/// `Some("")`), silently losing the second `..` and producing a path
/// that falsely starts with the root prefix. Each unmatched `..` in the
/// preserved form keeps the normalized path outside any sane root, so
/// the escape check fires correctly.
fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                let can_pop = matches!(
                    out.components().next_back(),
                    Some(std::path::Component::Normal(_))
                );
                if can_pop {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ============================================================================
// Origin stamping
// ============================================================================
//
// Walk every node in a Document and set `Range.origin_path` on each
// `.location` field. The walk only stamps the *block-level* `.location`
// fields here; finer-grained inline ranges land in PR 6 when file-ref
// resolution starts consulting them.

fn stamp_doc(doc: &mut Document, origin: &Arc<PathBuf>) {
    if let Some(title) = doc.title.as_mut() {
        title.location.origin_path = Some(Arc::clone(origin));
    }
    for ann in doc.annotations.iter_mut() {
        stamp_annotation(ann, origin);
    }
    stamp_session(&mut doc.root, origin);
}

fn stamp_session(s: &mut Session, origin: &Arc<PathBuf>) {
    s.location.origin_path = Some(Arc::clone(origin));
    if let Some(loc) = s.title.location.as_mut() {
        loc.origin_path = Some(Arc::clone(origin));
    }
    for ann in s.annotations.iter_mut() {
        stamp_annotation(ann, origin);
    }
    for item in s.children.as_mut_vec().iter_mut() {
        stamp_item(item, origin);
    }
}

fn stamp_annotation(
    a: &mut crate::lex::ast::elements::annotation::Annotation,
    origin: &Arc<PathBuf>,
) {
    a.location.origin_path = Some(Arc::clone(origin));
    a.data.location.origin_path = Some(Arc::clone(origin));
    for item in a.children.as_mut_vec().iter_mut() {
        stamp_item(item, origin);
    }
}

fn stamp_item(item: &mut ContentItem, origin: &Arc<PathBuf>) {
    match item {
        ContentItem::Session(s) => stamp_session(s, origin),
        ContentItem::Annotation(a) => stamp_annotation(a, origin),
        ContentItem::Paragraph(p) => {
            p.location.origin_path = Some(Arc::clone(origin));
            for ann in p.annotations.iter_mut() {
                stamp_annotation(ann, origin);
            }
            for line in p.lines.iter_mut() {
                stamp_item(line, origin);
            }
        }
        ContentItem::List(l) => {
            l.location.origin_path = Some(Arc::clone(origin));
            for li in l.items.as_mut_vec().iter_mut() {
                stamp_item(li, origin);
            }
        }
        ContentItem::ListItem(li) => {
            li.location.origin_path = Some(Arc::clone(origin));
            for ann in li.annotations.iter_mut() {
                stamp_annotation(ann, origin);
            }
            for child in li.children.as_mut_vec().iter_mut() {
                stamp_item(child, origin);
            }
        }
        ContentItem::Definition(d) => {
            d.location.origin_path = Some(Arc::clone(origin));
            for ann in d.annotations.iter_mut() {
                stamp_annotation(ann, origin);
            }
            for child in d.children.as_mut_vec().iter_mut() {
                stamp_item(child, origin);
            }
        }
        ContentItem::VerbatimBlock(v) => {
            v.location.origin_path = Some(Arc::clone(origin));
        }
        ContentItem::VerbatimLine(vl) => {
            vl.location.origin_path = Some(Arc::clone(origin));
        }
        ContentItem::Table(t) => {
            t.location.origin_path = Some(Arc::clone(origin));
        }
        ContentItem::TextLine(tl) => {
            tl.location.origin_path = Some(Arc::clone(origin));
        }
        ContentItem::BlankLineGroup(b) => {
            b.location.origin_path = Some(Arc::clone(origin));
        }
    }
}

// ============================================================================
// Parser glue
// ============================================================================

/// Parse `source` into a Document but skip the annotation-attachment stage,
/// so include annotations are findable in container children lists.
fn parse_no_attach(source: &str) -> Result<Document, String> {
    crate::lex::testing::parse_without_annotation_attachment(source)
}

// ============================================================================
// Filesystem-backed loader
// ============================================================================

/// [`Loader`] that reads files from the filesystem with `std::fs::read_to_string`.
///
/// This is the production loader used by the CLI; the LSP wraps it with a
/// file-watch invalidation layer in PR 8. lex-core's *resolver* code does not
/// reference `std::fs` — `FsLoader` is the one place where it does, isolated
/// behind the [`Loader`] trait so the rest of the crate stays sandbox- and
/// WASM-friendly.
///
/// `FsLoader` is constructed with the resolution root and rechecks every
/// load against it post-`fs::canonicalize`, so a symlink pointing outside
/// the root is rejected even though the lexical-only check in
/// [`resolve_path`] cannot see it. Also rejects non-regular files (devices,
/// FIFOs, directories) before reading, so the loader can't be tricked into
/// blocking on `/dev/zero` or allocating against an open device.
///
/// Errors map:
/// - canonicalization fails (file missing, permission denied at a parent,
///   broken symlink, …) → [`LoadError::NotFound`]
/// - canonical path doesn't sit under canonical root → [`LoadError::OutsideRoot`]
/// - target is not a regular file → [`LoadError::Io`] with a clear message
/// - any other I/O error during read → [`LoadError::Io`]
pub struct FsLoader {
    /// Filesystem-canonical resolution root. Constructed once at
    /// `FsLoader::new`; if canonicalization fails (e.g., the configured
    /// root doesn't exist on disk), we fall back to the input verbatim
    /// and the bounds check will simply never pass — visible to the user
    /// as a `LoadError::OutsideRoot` instead of silently disabling the
    /// security check.
    canonical_root: PathBuf,
    /// Per-file size cap (bytes). Loads of larger files surface as
    /// `LoadError::TooLarge` before any bytes are read into memory.
    /// Default [`FsLoader::DEFAULT_MAX_FILE_SIZE`].
    max_file_size: u64,
}

impl FsLoader {
    /// Default per-file size cap: 10 MiB. Generous for realistic Lex
    /// source documents (text only) and tight enough to bound memory
    /// allocation per include against an adversarial 1 GB file.
    pub const DEFAULT_MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

    /// Construct a loader rooted at `root` with default size limits.
    /// The loader stores `root`'s fs-canonical form (with symlinks
    /// resolved); subsequent loads validate that the requested path's
    /// canonical form lives under it.
    pub fn new(root: PathBuf) -> Self {
        let canonical_root = std::fs::canonicalize(&root).unwrap_or(root);
        Self {
            canonical_root,
            max_file_size: Self::DEFAULT_MAX_FILE_SIZE,
        }
    }

    /// Override the default per-file size cap (bytes). Use to widen the
    /// limit for projects with genuinely large source files, or tighten
    /// it for stricter sandboxes (e.g., LSPs serving untrusted content).
    pub fn with_max_file_size(mut self, max_file_size: u64) -> Self {
        self.max_file_size = max_file_size;
        self
    }
}

impl Loader for FsLoader {
    fn load(&self, path: &Path) -> Result<LoadedFile, LoadError> {
        // 1. Canonicalize. Resolves symlinks and `..` segments against the
        //    real filesystem. NotFound / broken-symlink / permission errors
        //    all surface here.
        let canonical_path = std::fs::canonicalize(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => LoadError::NotFound {
                path: path.to_path_buf(),
            },
            _ => LoadError::Io {
                path: path.to_path_buf(),
                message: e.to_string(),
            },
        })?;

        // 2. Bounds check against the *canonical* root. This is the
        //    actual security gate against symlink traversal — the lexical
        //    check in resolve_path can't see through symlinks.
        if !canonical_path.starts_with(&self.canonical_root) {
            return Err(LoadError::OutsideRoot {
                path: canonical_path,
                root: self.canonical_root.clone(),
            });
        }

        // 3. Reject non-regular files. Without this, an attacker (with
        //    write access to the repo) could symlink an include target to
        //    `/dev/zero` or a FIFO and block / OOM the reader. The
        //    is_file() metadata call is a cheap sanity check.
        let meta = std::fs::metadata(&canonical_path).map_err(|e| LoadError::Io {
            path: canonical_path.clone(),
            message: e.to_string(),
        })?;
        if !meta.is_file() {
            return Err(LoadError::Io {
                path: canonical_path,
                message: "include target is not a regular file".to_string(),
            });
        }

        // 4. Size cap. Bounds memory allocation per include against an
        //    adversarial 1 GB file before any bytes hit the heap.
        let size = meta.len();
        if size > self.max_file_size {
            return Err(LoadError::TooLarge {
                path: canonical_path,
                size,
                limit: self.max_file_size,
            });
        }

        // 5. Read. By this point we know the path is a regular file under
        //    the canonical root and within the size cap; anything that
        //    fails here is a real I/O error worth surfacing.
        let source = std::fs::read_to_string(&canonical_path).map_err(|e| LoadError::Io {
            path: canonical_path.clone(),
            message: e.to_string(),
        })?;

        Ok(LoadedFile {
            source,
            canonical_path,
        })
    }
}

// ============================================================================
// Test fixtures (test-support feature + cfg(test))
// ============================================================================

/// In-memory [`Loader`] backed by a `HashMap<PathBuf, String>`.
#[cfg(any(test, feature = "test-support"))]
pub struct MemoryLoader {
    files: std::collections::HashMap<PathBuf, String>,
}

#[cfg(any(test, feature = "test-support"))]
impl MemoryLoader {
    /// Create an empty loader. Add files with [`MemoryLoader::insert`].
    pub fn new() -> Self {
        Self {
            files: std::collections::HashMap::new(),
        }
    }

    /// Register a file at `path` with the given source text.
    pub fn insert<P: Into<PathBuf>, S: Into<String>>(&mut self, path: P, contents: S) -> &mut Self {
        self.files.insert(path.into(), contents.into());
        self
    }

    /// Convenience constructor: build a loader from any iterator of
    /// `(path, contents)` pairs.
    pub fn from_pairs<I, P, S>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (P, S)>,
        P: Into<PathBuf>,
        S: Into<String>,
    {
        let mut loader = Self::new();
        for (path, contents) in pairs {
            loader.insert(path, contents);
        }
        loader
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Default for MemoryLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Loader for MemoryLoader {
    fn load(&self, path: &Path) -> Result<LoadedFile, LoadError> {
        // Memory loaders have no symlinks; the lookup key *is* the
        // canonical identity. Cycle detection in the resolver compares
        // `LoadedFile::canonical_path` values; for tests this matches the
        // lexically-normalized paths the resolver already produces.
        let source = self
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| LoadError::NotFound {
                path: path.to_path_buf(),
            })?;
        Ok(LoadedFile {
            source,
            canonical_path: path.to_path_buf(),
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests;
