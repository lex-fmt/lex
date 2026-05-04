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
//! - PR 4 (this PR): single-pass splice + container-policy validation +
//!   doc-title/doc-annotation conversion + origin stamping + root-escape
//!   check. **No recursion into included files yet** — `lex.include`
//!   annotations inside *included* files survive into the merged tree
//!   as unresolved annotations.
//! - PR 5: recursive resolution + cycle detection + depth limit.
//! - PR 6: per-file footnote resolution + file-ref `Range.origin_path`
//!   consultation.
//!
//! # Layering
//!
//! lex-core's own code does *not* reference `std::fs` for include resolution.
//! Production [`Loader`] implementations live in the calling layer:
//!
//! - `FsLoader` will land in PR 7 (CLI integration), in lex-core but feature-gated
//!   to keep the no-FS-by-default property of this crate's library code.
//! - The LSP wraps an FS loader with file-watch invalidation (PR 8).
//! - WASM builds provide a JS-backed loader.
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
}

impl ResolveConfig {
    /// Default maximum include depth — enough for any reasonable atomization
    /// strategy (aggregator → per-chapter → per-section), bounded enough to
    /// keep the resolver's worst-case work predictable.
    pub const DEFAULT_MAX_DEPTH: usize = 8;

    /// Construct a config with the given root and default depth.
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            root,
            max_depth: Self::DEFAULT_MAX_DEPTH,
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
    /// Load the source text for `path`. The path is the canonical absolute
    /// path the resolver decided on after applying the rules in §4 of the
    /// proposal.
    fn load(&self, path: &Path) -> Result<String, LoadError>;
}

/// Errors a [`Loader`] can produce.
#[derive(Debug, Clone)]
pub enum LoadError {
    /// The loader could not find a resource at the given path.
    NotFound { path: PathBuf },
    /// Underlying I/O error (or virtual-filesystem equivalent).
    Io { path: PathBuf, message: String },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::NotFound { path } => write!(f, "include not found: {}", path.display()),
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
    /// An include chain looped back on itself. (PR 5.)
    Cycle { path: PathBuf, chain: Vec<PathBuf> },
    /// The include depth exceeded [`ResolveConfig::max_depth`]. (PR 5.)
    DepthExceeded { limit: usize },
    /// A path resolved outside the configured [`ResolveConfig::root`].
    RootEscape { path: PathBuf, root: PathBuf },
    /// The loader could not find or read the included file.
    NotFound { path: PathBuf },
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
            IncludeError::Cycle { path, chain } => {
                let chain_display: Vec<String> =
                    chain.iter().map(|p| p.display().to_string()).collect();
                write!(
                    f,
                    "include cycle: {} (chain: {})",
                    path.display(),
                    chain_display.join(" -> ")
                )
            }
            IncludeError::DepthExceeded { limit } => {
                write!(f, "include depth exceeded limit of {limit}")
            }
            IncludeError::RootEscape { path, root } => write!(
                f,
                "include path {} escapes resolution root {}",
                path.display(),
                root.display()
            ),
            IncludeError::NotFound { path } => write!(f, "include not found: {}", path.display()),
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

impl From<LoadError> for IncludeError {
    fn from(err: LoadError) -> Self {
        match err {
            LoadError::NotFound { path } => IncludeError::NotFound { path },
            LoadError::Io { path, message } => IncludeError::LoaderIo { path, message },
        }
    }
}

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

/// Resolve `:: lex.include ::` annotations starting from `source`.
///
/// `source_path` identifies the entry-point file. It is used to (a) resolve
/// relative include paths against the entry file's directory, and (b) stamp
/// `Range.origin_path` on every node so downstream code (file-ref resolution,
/// diagnostics, LSP goto) can report locations against the authoring file.
/// When `None`, relative paths resolve against `config.root` and origin
/// stamping is skipped on the entry document.
///
/// # Pre/post-attachment
///
/// Internally this re-parses the source *without* annotation attachment so
/// `lex.include` annotations are visible as standalone children where the
/// splice can replace them in-place. After splicing, [`AttachAnnotations`]
/// runs once on the merged tree, which lands the include annotation on the
/// first spliced node by the standard "attach to next sibling" rule. This
/// matches the textual paste mental model from the proposal.
///
/// # Scope (PR 4)
///
/// Single-pass splice only — no recursion into included files. PR 5 adds
/// recursion + cycle/depth/root-escape safety; PR 6 hooks per-file footnote
/// resolution.
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

    splice_in_session_container(doc.root.children.as_mut_vec(), &host_dir, config, loader)?;

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

fn splice_in_session_container(
    children: &mut Vec<ContentItem>,
    host_dir: &Path,
    config: &ResolveConfig,
    loader: &dyn Loader,
) -> Result<(), IncludeError> {
    // Post-order: recurse into the *original* nested containers first,
    // splice this container's includes second. Any newly-spliced content
    // (which itself may contain `lex.include` annotations) is therefore
    // never re-walked, so this PR stays single-pass: includes inside
    // included files survive into the merged tree as unresolved
    // annotations. PR 5 will replace this with proper recursive
    // resolution that tracks each loaded file's own host_dir.
    recurse_into_children(children, host_dir, config, loader)?;
    process_includes(children, host_dir, config, loader, ContainerKind::Session)
}

fn splice_in_general_container(
    container: &mut GeneralContainer,
    host_dir: &Path,
    config: &ResolveConfig,
    loader: &dyn Loader,
    kind: ContainerKind,
) -> Result<(), IncludeError> {
    recurse_into_children(container.as_mut_vec(), host_dir, config, loader)?;
    process_includes(container.as_mut_vec(), host_dir, config, loader, kind)
}

// Allow &mut Vec because `splice` needs Vec-specific operations.
#[allow(clippy::ptr_arg)]
fn process_includes(
    children: &mut Vec<ContentItem>,
    host_dir: &Path,
    config: &ResolveConfig,
    loader: &dyn Loader,
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

        let src = annotation
            .include_src()
            .ok_or_else(|| IncludeError::MissingSrc {
                include_site: annotation.location.clone(),
            })?;

        let target_path = resolve_path(&src, host_dir, &config.root)?;
        let target_source = loader.load(&target_path)?;

        let mut included =
            parse_no_attach(&target_source).map_err(|message| IncludeError::ParseFailed {
                path: target_path.clone(),
                message,
            })?;

        let target_origin = Arc::new(target_path.clone());
        stamp_doc(&mut included, &target_origin);

        let splice_items = prepare_splice_list(included);
        validate_against_kind(&splice_items, kind, &annotation.location, &target_path)?;

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

#[allow(clippy::ptr_arg)]
fn recurse_into_children(
    children: &mut Vec<ContentItem>,
    host_dir: &Path,
    config: &ResolveConfig,
    loader: &dyn Loader,
) -> Result<(), IncludeError> {
    for item in children.iter_mut() {
        match item {
            ContentItem::Session(s) => {
                splice_in_session_container(s.children.as_mut_vec(), host_dir, config, loader)?;
            }
            ContentItem::Definition(d) => {
                splice_in_general_container(
                    &mut d.children,
                    host_dir,
                    config,
                    loader,
                    ContainerKind::Definition,
                )?;
            }
            ContentItem::Annotation(a) if !a.is_include() => {
                splice_in_general_container(
                    &mut a.children,
                    host_dir,
                    config,
                    loader,
                    ContainerKind::AnnotationBody,
                )?;
            }
            ContentItem::List(l) => {
                for li in l.items.as_mut_vec().iter_mut() {
                    if let ContentItem::ListItem(item) = li {
                        splice_in_general_container(
                            &mut item.children,
                            host_dir,
                            config,
                            loader,
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
    // becomes a paragraph in the host's context).
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
/// Unresolvable `..` components (those that would pop past the start of the
/// buffer) are *preserved* in the output. This matters when the buffer
/// happens to be empty or holds only a relative prefix — silently dropping
/// the `..` would let a path masquerade as inside the root and defeat the
/// escape check. With absolute roots (the documented contract) `..`
/// components only matter near the root itself, where preserving them
/// makes the prefix check fail correctly.
fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                if !out.pop() {
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
    fn load(&self, path: &Path) -> Result<String, LoadError> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| LoadError::NotFound {
                path: path.to_path_buf(),
            })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests;
