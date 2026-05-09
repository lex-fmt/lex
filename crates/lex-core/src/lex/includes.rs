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

// `IncludeError` carries diagnostic context (paths, source ranges,
// handler messages) on every variant; the `result_large_err` lint
// would have us box the whole error or split it into a thinner shape
// just to satisfy the size heuristic. The enum is already part of
// the public API and the error path is rare; suppress the lint for
// this module rather than churn the public surface.
#![allow(clippy::result_large_err)]

use crate::lex::assembling::AttachAnnotations;
use crate::lex::ast::elements::container::GeneralContainer;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::session::Session;
use crate::lex::ast::range::Range;
use crate::lex::ast::Document;
use crate::lex::transforms::Runnable;
use lex_extension::handler::HandlerError;
use lex_extension_host::registry::Registry;
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
    /// The include `src` was a platform-absolute filesystem path
    /// (e.g. Windows `C:\foo`, `\\server\share`, `\foo`). The spec
    /// forbids absolute filesystem paths from entering the
    /// resolution pipeline; the *root-absolute* form (leading `/`
    /// resolved against the includes root) is the only spec-allowed
    /// way to write a path that doesn't start from the host's
    /// directory. On Unix the only thing that's `Path::is_absolute()`
    /// is a leading `/`, which is consumed by the root-absolute
    /// branch first; this variant therefore only fires in practice
    /// for Windows-shaped absolute paths.
    AbsolutePath { path: PathBuf },
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
    /// A registered handler returned an error the pass could not map
    /// onto a more specific variant — typically a third-party
    /// namespace's resolve hook surfacing an internal failure, or an
    /// unrecognised handler-defined code from `lex.*` built-ins. The
    /// `code` is the string identifier the registry attaches to the
    /// diagnostic (`"handler.internal"`, `"handler.custom"`, …).
    HandlerFailed {
        include_site: Range,
        label: String,
        code: String,
        message: String,
    },
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
            IncludeError::AbsolutePath { path } => write!(
                f,
                "include src {} is a platform-absolute path; \
                 the spec forbids absolute filesystem paths — use a relative path \
                 (chapters/01.lex) or a root-absolute path (/shared/01.lex)",
                path.display()
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
            IncludeError::HandlerFailed {
                label,
                code,
                message,
                ..
            } => write!(f, "extension handler `{label}` failed ({code}): {message}"),
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

/// Hard cap on resolution depth, applied even when the
/// configurable [`ResolveConfig::max_depth`] is set higher. Bounds
/// adversarial varying-position recursion (a handler that returns
/// content with a different invocation site each iteration so the
/// cycle key never matches) so the resolver always terminates.
pub const KERNEL_DEPTH_BACKSTOP: usize = 32;

/// Resolve every `hooks.resolve = true` labelled annotation starting
/// from `source`, dispatching through `registry`, and recursively
/// processing the spliced content.
///
/// `source_path` identifies the entry-point file. It is used to
/// (a) stamp `Range.origin_path` on every node so downstream code
/// (file-ref resolution, diagnostics, LSP goto) can report locations
/// against the authoring file, and (b) provide the host directory
/// the built-in `lex.include` handler resolves relative `src=` paths
/// against (via `LabelCtx.node.origin`). When `None`, origin stamping
/// is skipped on the entry and the handler resolves relative paths
/// against `config.root`.
///
/// # Generic dispatch
///
/// Every label whose schema declares `hooks.resolve = true` flows
/// through the same path: build a [`LabelCtx`] from the annotation,
/// call [`Registry::dispatch_resolve_raw`], decode the returned
/// [`WireNode`] back into typed [`ContentItem`]s via
/// [`crate::lex::wire::from_wire_node`], and splice in place. The
/// built-in `lex.include` handler is registered the same way as any
/// third-party namespace.
///
/// # Pre/post-attachment
///
/// Internally this re-parses the entry source *without* annotation
/// attachment so labelled annotations stay visible as standalone
/// children. The handler does its own `parse_no_attach` for loaded
/// content. After all splices, [`AttachAnnotations`] runs once on
/// the merged tree.
///
/// # Recursion + cycle detection
///
/// Cycle detection keys on `(label, origin_path, start_position)` of
/// the invocation site. A handler that returns content containing
/// another invocation at the same source position is caught
/// immediately. A handler that varies the invocation position each
/// iteration terminates at `min(config.max_depth, KERNEL_DEPTH_BACKSTOP)`
/// with `IncludeError::DepthExceeded`. The total-includes counter
/// caps adversarial fan-out independent of depth.
pub fn resolve_from_source(
    source: &str,
    source_path: Option<PathBuf>,
    config: &ResolveConfig,
    registry: &Registry,
) -> Result<Document, IncludeError> {
    let entry_origin = source_path.as_ref().map(|p| Arc::new(p.clone()));

    let mut doc = parse_no_attach(source).map_err(|message| IncludeError::ParseFailed {
        path: source_path.clone().unwrap_or_default(),
        message,
    })?;

    if let Some(origin) = entry_origin.as_ref() {
        stamp_doc(&mut doc, origin);
    }

    let mut chain: Vec<ResolveKey> = Vec::new();
    let mut state = ResolverState {
        config,
        registry,
        chain: &mut chain,
        depth: 0,
        total_resolved: 0,
    };

    splice_in_session_container(doc.root.children.as_mut_vec(), &mut state)?;

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

/// One frame on the resolve-pass cycle stack. Two invocations at the
/// same `(label, origin, start)` position are a cycle, regardless of
/// what parameters either invocation uses — a handler that varies
/// params per call (random IDs, timestamps) cannot defeat the
/// detector by changing param values.
#[derive(Debug, Clone, PartialEq)]
struct ResolveKey {
    label: String,
    /// `Range.origin_path` of the annotation — the file the
    /// invocation was authored in. `None` when stamping was skipped
    /// (e.g., entry source loaded from a string with no path).
    origin: Option<PathBuf>,
    start: crate::lex::ast::range::Position,
}

impl ResolveKey {
    fn from_annotation(a: &crate::lex::ast::elements::annotation::Annotation) -> Self {
        Self {
            label: a.data.label.value.clone(),
            origin: a.location.origin_path.as_ref().map(|p| (**p).clone()),
            start: a.location.start,
        }
    }
}

/// Per-resolution state threaded through the recursive walker. Keeps the
/// signatures of the splice/process functions short and ensures
/// `chain`/`depth` are updated in lock-step (push/pop, +1/back-out) at
/// each invocation.
struct ResolverState<'a> {
    config: &'a ResolveConfig,
    registry: &'a Registry,
    /// Active resolution stack of `(label, origin, position)` keys.
    /// Pushed when we begin dispatching for an invocation and popped
    /// when its splice subtree is fully resolved. A push that finds
    /// the same key already on the stack is a cycle.
    chain: &'a mut Vec<ResolveKey>,
    /// Number of dispatch hops from the entry point. Each recursion
    /// increments by 1. Hitting `config.max_depth` or the
    /// [`KERNEL_DEPTH_BACKSTOP`] (whichever is lower) is an error.
    depth: usize,
    /// Total invocations resolved across the entire walk
    /// (depth × breadth). Incremented on every successful dispatch.
    /// Hitting `config.max_total_includes` aborts with
    /// `TotalIncludesExceeded`.
    total_resolved: usize,
}

fn splice_in_session_container(
    children: &mut Vec<ContentItem>,
    state: &mut ResolverState<'_>,
) -> Result<(), IncludeError> {
    // Post-order: recurse into nested containers first, splice this
    // container's invocations second. Recursion happens inside
    // `process_resolves` for any spliced subtree, so that subtree
    // is never re-walked at the parent level.
    recurse_into_children(children, state)?;
    process_resolves(children, state, ContainerKind::Session)
}

fn splice_in_general_container(
    container: &mut GeneralContainer,
    state: &mut ResolverState<'_>,
    kind: ContainerKind,
) -> Result<(), IncludeError> {
    recurse_into_children(container.as_mut_vec(), state)?;
    process_resolves(container.as_mut_vec(), state, kind)
}

/// Walk the children of a container, dispatch every annotation whose
/// schema declares `hooks.resolve = true` through the registry, and
/// splice the returned content in place of the annotation. Recurses
/// into the spliced content so nested invocations resolve too.
// Allow &mut Vec because `splice` needs Vec-specific operations.
#[allow(clippy::ptr_arg)]
fn process_resolves(
    children: &mut Vec<ContentItem>,
    state: &mut ResolverState<'_>,
    kind: ContainerKind,
) -> Result<(), IncludeError> {
    // Collect indices of annotations whose schema has hooks.resolve.
    let resolve_indices: Vec<usize> = children
        .iter()
        .enumerate()
        .filter_map(|(i, item)| match item {
            ContentItem::Annotation(a) => {
                let label = &a.data.label.value;
                if state
                    .registry
                    .schema_for(label)
                    .map(|s| s.hooks.resolve)
                    .unwrap_or(false)
                {
                    Some(i)
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    for i in resolve_indices.into_iter().rev() {
        let annotation = match &children[i] {
            ContentItem::Annotation(a) => a.clone(),
            _ => unreachable!("index came from resolve filter"),
        };

        match resolve_one_invocation(&annotation, state, kind)? {
            ResolveOutcome::Spliced(splice_items) => {
                // Replace the annotation with `[annotation, ...splice_items]`.
                // The annotation itself stays in the children list immediately
                // before the splice, so the post-resolution AttachAnnotations
                // pass moves it onto the first spliced node by the standard
                // "attach to next sibling" rule.
                let mut replacement = Vec::with_capacity(splice_items.len() + 1);
                replacement.push(ContentItem::Annotation(annotation));
                replacement.extend(splice_items);
                children.splice(i..=i, replacement);
            }
            ResolveOutcome::Unexpanded => {
                // Handler opted out of expanding this invocation. The
                // annotation stays in place, but its body wasn't
                // walked by `recurse_into_children` (that walker
                // skips resolve-hooked annotations to avoid double-
                // resolution). Walk the body now so any nested
                // invocations inside the unexpanded annotation get
                // resolved on the way back up.
                let mut owned = annotation;
                splice_in_general_container(
                    &mut owned.children,
                    state,
                    ContainerKind::AnnotationBody,
                )?;
                children[i] = ContentItem::Annotation(owned);
            }
        }
    }

    Ok(())
}

/// Outcome of dispatching a single resolve-hooked annotation. The
/// pass needs to distinguish between "handler returned content,
/// splice it in" and "handler opted out, leave the annotation
/// alone": the second case still requires walking the annotation's
/// body for nested invocations because `recurse_into_children`
/// otherwise skips resolve-hooked annotations to prevent double-
/// resolution.
enum ResolveOutcome {
    Spliced(Vec<ContentItem>),
    Unexpanded,
}

/// Dispatch a single resolve-hooked annotation through the registry,
/// decode the returned `WireNode` back into typed children, then
/// recursively walk the splice items so nested invocations resolve
/// before the splice is placed into the parent container.
///
/// Returns [`ResolveOutcome::Unexpanded`] when the handler returned
/// `Ok(None)` (third-party handlers can opt out of expanding a
/// particular invocation). The caller is then responsible for
/// walking the annotation's body for nested invocations — the
/// resolve walker normally skips resolve-hooked annotations'
/// bodies.
fn resolve_one_invocation(
    annotation: &crate::lex::ast::elements::annotation::Annotation,
    state: &mut ResolverState<'_>,
    parent_kind: ContainerKind,
) -> Result<ResolveOutcome, IncludeError> {
    let label = &annotation.data.label.value;
    let key = ResolveKey::from_annotation(annotation);

    // Cycle check on (label, origin, start) of the invocation site.
    if state.chain.contains(&key) {
        return Err(IncludeError::Cycle {
            include_site: annotation.location.clone(),
            path: key.origin.clone().unwrap_or_default(),
            chain: state
                .chain
                .iter()
                .map(|k| k.origin.clone().unwrap_or_default())
                .collect(),
        });
    }

    // Depth check — both the user-facing config and the hard kernel
    // backstop. Whichever fires first wins; both surface as
    // DepthExceeded since the user-facing message reflects the
    // configured limit.
    let effective_depth_limit = state.config.max_depth.min(KERNEL_DEPTH_BACKSTOP);
    if state.depth >= effective_depth_limit {
        return Err(IncludeError::DepthExceeded {
            include_site: annotation.location.clone(),
            limit: effective_depth_limit,
            chain: state
                .chain
                .iter()
                .map(|k| k.origin.clone().unwrap_or_default())
                .collect(),
        });
    }

    // Total-count check before dispatch.
    if state.total_resolved >= state.config.max_total_includes {
        return Err(IncludeError::TotalIncludesExceeded {
            include_site: annotation.location.clone(),
            limit: state.config.max_total_includes,
        });
    }

    let ctx = build_label_ctx(annotation);

    let wire_node = match state.registry.dispatch_resolve_raw(&ctx) {
        Ok(Some(node)) => node,
        Ok(None) => {
            // Handler returned "nothing to splice" — leave the
            // annotation in place. The caller still needs to walk
            // its body for nested invocations (built-in lex.include
            // never returns None; this path is reachable only via
            // third-party handlers that opt out per-invocation).
            return Ok(ResolveOutcome::Unexpanded);
        }
        Err(handler_err) => {
            return Err(handler_error_to_include_error(
                &handler_err,
                label,
                &annotation.location,
            ));
        }
    };

    state.total_resolved += 1;

    // Decode the wire payload into typed lex-core ContentItems.
    let mut splice_items = decode_wire_to_items(&wire_node, label, &annotation.location)?;

    // Recurse into the spliced subtree FIRST so nested resolve-hooked
    // annotations are processed before the splice lands. Validation
    // must wait until *after* this step: a nested invocation can
    // splice in content (e.g. a top-level `Session` from a chained
    // `lex.include`) that wasn't in the handler's original output,
    // and the final shape is what has to satisfy the parent
    // container's policy.
    let included_path = key.origin.clone().unwrap_or_default();
    state.chain.push(key);
    let saved_depth = state.depth;
    state.depth = saved_depth + 1;
    let recurse_result = splice_in_session_container(&mut splice_items, state);
    state.depth = saved_depth;
    state.chain.pop();
    recurse_result?;

    // Container-policy validation: enforce no-Sessions inside
    // `GeneralContainer` (Definition / Annotation body / ListItem).
    // Runs against the post-recursion splice list so nested
    // expansions can't smuggle disallowed shapes past the check.
    validate_against_kind(
        &splice_items,
        parent_kind,
        &annotation.location,
        &included_path,
    )?;

    Ok(ResolveOutcome::Spliced(splice_items))
}

/// Build a [`LabelCtx`] from a lex-core [`Annotation`]. The body is
/// derived from the annotation's children (parsed-Lex form), the
/// params from `Annotation::data::parameters`, and the host node info
/// from `Annotation::location`.
fn build_label_ctx(
    a: &crate::lex::ast::elements::annotation::Annotation,
) -> lex_extension::wire::LabelCtx {
    use crate::lex::wire::to_wire_node;
    use lex_extension::wire::{AnnotationBody, LabelCtx, NodeRef};

    let label = a.data.label.value.clone();
    let params = {
        // Pass *semantic* parameter values to handlers (quotes
        // stripped, escape sequences resolved). Handlers consume
        // params as JSON values, where there is no "quoted string"
        // vs "unquoted token" distinction; only the decoded value
        // is meaningful. The codec's `parameters_to_json` (used by
        // `annotation_to_wire` for round-tripping annotation
        // *content*) keeps the raw form to preserve source — the
        // two paths intentionally differ.
        let mut obj = serde_json::Map::with_capacity(a.data.parameters.len());
        for p in &a.data.parameters {
            obj.insert(p.key.clone(), serde_json::Value::String(p.unquoted_value()));
        }
        serde_json::Value::Object(obj)
    };
    let body = if a.children.is_empty() {
        AnnotationBody::None
    } else {
        let wire_children: Vec<lex_extension::wire::WireNode> =
            a.children.iter().map(to_wire_node).collect();
        AnnotationBody::Lex {
            children: wire_children,
        }
    };
    let range = lex_extension::wire::Range::new(
        lex_extension::wire::Position::new(
            u32::try_from(a.location.start.line).unwrap_or(u32::MAX),
            u32::try_from(a.location.start.column).unwrap_or(u32::MAX),
        ),
        lex_extension::wire::Position::new(
            u32::try_from(a.location.end.line).unwrap_or(u32::MAX),
            u32::try_from(a.location.end.column).unwrap_or(u32::MAX),
        ),
    );
    let origin = a
        .location
        .origin_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    LabelCtx {
        label,
        params,
        body,
        node: NodeRef {
            kind: "annotation".into(),
            range,
            origin,
        },
    }
}

/// Convert a handler-returned [`WireNode`] back into a list of
/// [`ContentItem`]s ready for splicing. `WireNode::Document` is
/// unwrapped (its children become the splice list); any other root
/// shape is wrapped as a single-item list.
///
/// `invocation_label` is the label whose handler produced `wire` —
/// threaded through so wire-decode failures are attributed to the
/// real namespace rather than a hardcoded `lex.include`. A
/// third-party `acme.expand` handler that returns malformed wire
/// will surface as `IncludeError::HandlerFailed { label:
/// "acme.expand", .. }`.
fn decode_wire_to_items(
    wire: &lex_extension::wire::WireNode,
    invocation_label: &str,
    include_site: &Range,
) -> Result<Vec<ContentItem>, IncludeError> {
    use crate::lex::wire::from_wire_node;

    from_wire_node(wire).map_err(|e| IncludeError::HandlerFailed {
        include_site: include_site.clone(),
        label: invocation_label.to_string(),
        code: "wire.decode".into(),
        message: format!("decoding handler-returned wire payload failed: {e}"),
    })
}

/// Map a [`HandlerError`] returned by the registry into the most
/// specific [`IncludeError`] variant available. Codes in the
/// `-32001..=-32005` range emitted by [`crate::lex::builtins::LexIncludeHandler`]
/// translate back to their corresponding pre-extension-system
/// variants so existing CLI/LSP error rendering and the integration
/// test suite keep working unchanged. Unknown codes (third-party
/// namespaces, future built-ins) surface as `HandlerFailed`.
fn handler_error_to_include_error(
    err: &HandlerError,
    label: &str,
    include_site: &Range,
) -> IncludeError {
    use crate::lex::builtins::include::{
        CODE_ABSOLUTE_PATH, CODE_IO, CODE_MISSING_SRC, CODE_NOT_FOUND, CODE_OUTSIDE_ROOT,
        CODE_TOO_LARGE,
    };

    match err {
        HandlerError::Custom {
            code,
            message,
            data,
        } => match *code {
            CODE_NOT_FOUND => IncludeError::NotFound {
                include_site: include_site.clone(),
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
            },
            CODE_OUTSIDE_ROOT => IncludeError::RootEscape {
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
                root: data_str(data, "root")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
            },
            CODE_TOO_LARGE => IncludeError::FileTooLarge {
                include_site: include_site.clone(),
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
                size: data_u64(data, "size").unwrap_or(0),
                limit: data_u64(data, "limit").unwrap_or(0),
            },
            CODE_ABSOLUTE_PATH => IncludeError::AbsolutePath {
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
            },
            CODE_IO => IncludeError::LoaderIo {
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
                message: message.clone(),
            },
            CODE_MISSING_SRC => IncludeError::MissingSrc {
                include_site: include_site.clone(),
            },
            other => IncludeError::HandlerFailed {
                include_site: include_site.clone(),
                label: label.to_string(),
                code: format!("handler.custom({other})"),
                message: message.clone(),
            },
        },
        HandlerError::Internal { message } => {
            // Built-in lex.include flags parse failures with the
            // distinctive `parse of `<path>` failed: <msg>` prefix.
            // Recover the path so existing tests that match
            // `IncludeError::ParseFailed` keep working.
            if let Some((path, msg)) = parse_internal_parse_failure(message) {
                IncludeError::ParseFailed { path, message: msg }
            } else {
                IncludeError::HandlerFailed {
                    include_site: include_site.clone(),
                    label: label.to_string(),
                    code: "handler.internal".into(),
                    message: message.clone(),
                }
            }
        }
        HandlerError::Unsupported { detail } => IncludeError::HandlerFailed {
            include_site: include_site.clone(),
            label: label.to_string(),
            code: "handler.unsupported".into(),
            message: detail.clone(),
        },
    }
}

fn data_str(data: &Option<serde_json::Value>, key: &str) -> Option<String> {
    data.as_ref()?.get(key)?.as_str().map(str::to_string)
}

fn data_u64(data: &Option<serde_json::Value>, key: &str) -> Option<u64> {
    data.as_ref()?.get(key)?.as_u64()
}

/// Recover `(path, message)` from a parse-failure `Internal` message
/// shaped `parse of `<path>` failed: <msg>`. Returns `None` if the
/// message doesn't match — the caller then falls back to a generic
/// `HandlerFailed`.
fn parse_internal_parse_failure(message: &str) -> Option<(PathBuf, String)> {
    let rest = message.strip_prefix("parse of `")?;
    let close_idx = rest.find("` failed: ")?;
    let path = PathBuf::from(&rest[..close_idx]);
    let msg = rest[close_idx + "` failed: ".len()..].to_string();
    Some((path, msg))
}

#[allow(clippy::ptr_arg)]
fn recurse_into_children(
    children: &mut Vec<ContentItem>,
    state: &mut ResolverState<'_>,
) -> Result<(), IncludeError> {
    for item in children.iter_mut() {
        match item {
            ContentItem::Session(s) => {
                splice_in_session_container(s.children.as_mut_vec(), state)?;
            }
            ContentItem::Definition(d) => {
                splice_in_general_container(&mut d.children, state, ContainerKind::Definition)?;
            }
            ContentItem::Annotation(a) => {
                // Skip the body of annotations whose schema declares
                // `hooks.resolve = true` — those are dispatched at the
                // parent level by `process_resolves`, and walking
                // their bodies here would trip the resolve again on
                // the same invocation. Other annotations recurse
                // normally so their nested bodies get processed.
                let is_resolve_hooked = state
                    .registry
                    .schema_for(&a.data.label.value)
                    .map(|s| s.hooks.resolve)
                    .unwrap_or(false);
                if !is_resolve_hooked {
                    splice_in_general_container(
                        &mut a.children,
                        state,
                        ContainerKind::AnnotationBody,
                    )?;
                }
            }
            ContentItem::List(l) => {
                for li in l.items.as_mut_vec().iter_mut() {
                    if let ContentItem::ListItem(item) = li {
                        splice_in_general_container(
                            &mut item.children,
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
        // Root-absolute (Lex spec convention): leading `/` means "from
        // the resolution root", not "filesystem root".
        root.join(rel)
    } else {
        // Anything else must be a relative path. Reject inputs the
        // host platform would treat as absolute (Windows `C:\foo`,
        // `\\server\share`, `\foo`) up front: the spec forbids
        // platform-absolute paths from entering the resolution
        // pipeline. Without this, `host_dir.join(src)` would silently
        // discard `host_dir` because Rust's `PathBuf::join` replaces
        // the base when the joined path is absolute. The downstream
        // root-escape check would still catch the security side, but
        // we'd surface a misleading "escapes root" error instead of
        // "absolute paths not allowed", and we'd be relying on
        // `PathBuf::join`'s override semantics for the security
        // outcome rather than holding the line at the input boundary.
        if Path::new(src).is_absolute() {
            return Err(IncludeError::AbsolutePath {
                path: PathBuf::from(src),
            });
        }
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

pub(crate) fn stamp_doc(doc: &mut Document, origin: &Arc<PathBuf>) {
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
pub(crate) fn parse_no_attach(source: &str) -> Result<Document, String> {
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
