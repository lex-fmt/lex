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
//! This is the module skeleton (PR 3 of 10). [`resolve_includes`] is currently
//! a no-op stub that returns the input document unchanged. The trait, config,
//! and error surface are stable; the splice logic, container-policy
//! validation, recursion, cycle detection, and depth limiting will land in
//! subsequent PRs (4-6).
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
//!
//! # Example (post-stub)
//!
//! ```ignore
//! use lex_core::lex::includes::{resolve_includes, ResolveConfig};
//! use lex_core::lex::loader::DocumentLoader;
//! use std::path::PathBuf;
//!
//! let doc = DocumentLoader::from_path("main.lex")?.parse()?;
//! let resolved = resolve_includes(
//!     doc,
//!     &ResolveConfig {
//!         root: PathBuf::from("/project"),
//!         max_depth: 8,
//!     },
//!     &my_loader,
//! )?;
//! ```

use crate::lex::ast::range::Range;
use crate::lex::ast::Document;
use std::path::{Path, PathBuf};

/// Configuration for the include resolution pass.
#[derive(Debug, Clone)]
pub struct ResolveConfig {
    /// Directory all include paths resolve under. Any include that
    /// canonicalizes outside this root is a [`IncludeError::RootEscape`].
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
///
/// Paths handed to [`Loader::load`] are *canonicalized absolute paths inside
/// the resolution root*, normalized by the resolver before the call.
/// Implementations do not need to perform their own path resolution.
pub trait Loader {
    /// Load the source text for `path`. The path is the canonical absolute
    /// path the resolver decided on after applying the rules in §4 of the
    /// proposal (relative-from-authoring-file or root-absolute, then
    /// normalized + checked against the root).
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
    /// An include chain looped back on itself. `chain` is the resolution
    /// stack at the moment the cycle was detected, with `path` as the
    /// duplicate.
    Cycle { path: PathBuf, chain: Vec<PathBuf> },
    /// The include depth exceeded [`ResolveConfig::max_depth`].
    DepthExceeded { limit: usize },
    /// A path resolved outside the configured [`ResolveConfig::root`].
    /// Both relative and root-absolute paths are checked uniformly.
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

/// Resolve `:: lex.include ::` annotations in `doc`, returning the merged tree.
///
/// # Stub
///
/// In this PR (module skeleton) the function is a no-op: it walks no tree,
/// makes no loader calls, and returns its input. Subsequent PRs will:
///
/// - PR 4: implement single-file splice + container-policy validation +
///   doc-title/doc-annotation conversion.
/// - PR 5: add recursion, cycle detection, depth limiting, root-escape check.
/// - PR 6: hook in per-file footnote resolution and `Range.origin_path` for
///   file-reference resolution.
///
/// The signature is stable from this PR forward; downstream wiring (CLI in
/// PR 7, LSP in PR 8) can be authored against it.
pub fn resolve_includes(
    doc: Document,
    _config: &ResolveConfig,
    _loader: &dyn Loader,
) -> Result<Document, IncludeError> {
    // Stub — see PR 4 for the actual splice logic.
    Ok(doc)
}

// ============================================================================
// Test fixtures (test-support feature + cfg(test))
// ============================================================================

/// In-memory [`Loader`] backed by a `HashMap<PathBuf, String>`.
///
/// Provided so downstream crates can write include-resolution tests without
/// touching the filesystem.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::loader::DocumentLoader;

    #[test]
    fn resolve_includes_is_noop_stub() {
        // The skeleton returns input unchanged.
        let doc = DocumentLoader::from_string("Some Title\n\n    A paragraph.\n")
            .parse()
            .unwrap();
        let cloned_for_compare = doc.clone();
        let loader = MemoryLoader::new();
        let config = ResolveConfig::with_root(PathBuf::from("/tmp"));

        let resolved = resolve_includes(doc, &config, &loader).unwrap();
        assert_eq!(resolved, cloned_for_compare);
    }

    #[test]
    fn resolve_config_default_depth() {
        let cfg = ResolveConfig::with_root(PathBuf::from("/x"));
        assert_eq!(cfg.max_depth, 8);
        assert_eq!(ResolveConfig::DEFAULT_MAX_DEPTH, 8);
    }

    #[test]
    fn memory_loader_returns_inserted_files() {
        let loader = MemoryLoader::from_pairs([
            (PathBuf::from("/a.lex"), "Aaa\n"),
            (PathBuf::from("/b.lex"), "Bbb\n"),
        ]);
        assert_eq!(loader.load(Path::new("/a.lex")).unwrap(), "Aaa\n");
        assert_eq!(loader.load(Path::new("/b.lex")).unwrap(), "Bbb\n");
    }

    #[test]
    fn memory_loader_missing_returns_not_found() {
        let loader = MemoryLoader::new();
        match loader.load(Path::new("/missing.lex")) {
            Err(LoadError::NotFound { path }) => assert_eq!(path, PathBuf::from("/missing.lex")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn load_error_converts_to_include_error_preserving_kind() {
        let not_found: IncludeError = LoadError::NotFound {
            path: PathBuf::from("/x"),
        }
        .into();
        assert!(matches!(not_found, IncludeError::NotFound { .. }));

        let io: IncludeError = LoadError::Io {
            path: PathBuf::from("/y"),
            message: "boom".into(),
        }
        .into();
        assert!(matches!(io, IncludeError::LoaderIo { .. }));
    }

    #[test]
    fn errors_format_with_relevant_paths() {
        let cycle = IncludeError::Cycle {
            path: PathBuf::from("/a.lex"),
            chain: vec![PathBuf::from("/main.lex"), PathBuf::from("/a.lex")],
        };
        let s = cycle.to_string();
        assert!(s.contains("/a.lex"));
        assert!(s.contains("/main.lex"));

        let depth = IncludeError::DepthExceeded { limit: 8 };
        assert!(depth.to_string().contains("8"));

        let escape = IncludeError::RootEscape {
            path: PathBuf::from("/etc/passwd"),
            root: PathBuf::from("/project"),
        };
        let s = escape.to_string();
        assert!(s.contains("/etc/passwd"));
        assert!(s.contains("/project"));

        let policy = IncludeError::ContainerPolicy {
            include_site: Range::default(),
            container: "Definition",
            file: PathBuf::from("/chapter.lex"),
            violation: "Sessions",
        };
        let s = policy.to_string();
        assert!(s.contains("Definition"));
        assert!(s.contains("/chapter.lex"));
        assert!(s.contains("Sessions"));
        // Generic phrasing — the violation kind appears in both
        // "contains" and "does not allow" clauses, so a future
        // policy with violation="ListItems" reads correctly too.
        assert!(s.contains("does not allow Sessions"));
    }
}
