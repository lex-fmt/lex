//! Error types for include loading and resolution.
//!
//! [`LoadError`] is what a [`Loader`](super::Loader) produces; it knows
//! about paths and I/O but not about the `lex.include` annotation that
//! asked for the file. [`IncludeError`] is what the resolver produces; it
//! carries the include *site* (the annotation's [`Range`]) so editors can
//! squiggle the exact line. There is intentionally no `From<LoadError>`
//! impl — callers map `LoadError` explicitly at the call site, where the
//! site is available.

use crate::lex::ast::range::Range;
use std::path::PathBuf;

/// Errors a [`Loader`](super::Loader) can produce.
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
    /// The include depth exceeded [`ResolveConfig::max_depth`](super::ResolveConfig::max_depth).
    /// `chain` shows the resolution stack at the moment of failure, in source
    /// order. `include_site` is the range of the offending
    /// `lex.include` annotation in its host file.
    DepthExceeded {
        include_site: Range,
        limit: usize,
        chain: Vec<PathBuf>,
    },
    /// The total number of includes resolved across the document
    /// exceeded [`ResolveConfig::max_total_includes`](super::ResolveConfig::max_total_includes).
    /// Bounds adversarial fan-out (which `max_depth` alone does not).
    /// `include_site` is the `lex.include` annotation that pushed the count
    /// past the limit.
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
    /// A path resolved outside the configured [`ResolveConfig::root`](super::ResolveConfig::root).
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
