//! [`Fetcher`] trait — the contract per-transport network resolvers
//! implement.
//!
//! The host owns the cache (content-hashed lookup, TTL bookkeeping,
//! `~/.cache/lex/labels/` layout); the fetcher only knows how to
//! fetch one URI's contents to a directory. This split keeps the
//! per-transport implementation small — the git fetcher only needs to
//! shell out to `git clone`, not understand lex's cache layout.
//!
//! ## Transports vs. URL templates
//!
//! The model (specified in `comms/specs/proposals/extending-lex-stores.lex`)
//! decomposes the resolver into three real *transports* and N *URL
//! templates*:
//!
//! - **Transports** carry the actual data movement. Three ship today:
//!   - `path:` — built-in local filesystem read, implemented in
//!     [`super::path`]. Special-cased upstream of registry dispatch
//!     (no [`Fetcher`] impl, no cache); listed here for completeness
//!     of the transport set.
//!   - `https:` — HTTPS GET of a tarball/zip, implemented as the
//!     [`HttpsFetcher`] [`Fetcher`] in this module.
//!   - `git:` / `git+ssh:` — git clone, implemented as the
//!     [`GitFetcher`] [`Fetcher`] in this module. Accepts any URL
//!     form `git clone` accepts; the `git+ssh:` scheme is retained
//!     for backwards compatibility and dispatched to the same fetcher.
//! - **URL templates** are forge-shorthands that expand into a
//!   transport URI before registry dispatch. They live in
//!   [`super::template`] and have no `Fetcher` impl — they're pure
//!   functions over URIs. `github:owner/repo` and `gitlab:owner/repo`
//!   are the two templates shipped today.
//!
//! See [lex#562](https://github.com/lex-fmt/lex/issues/562) for the
//! tracking issue covering the two real transport implementations.
//! Until those land, both fetchers ship as stubs that return
//! [`FetchError::Unimplemented`] when the dispatch reaches them.

use std::path::Path;

use super::uri::ParsedUri;

/// Per-transport network resolver. Implementations fetch the URI's
/// contents into a caller-provided destination directory.
///
/// ## Contract
///
/// - **`dest` is an empty directory the caller owns.** The fetcher
///   writes the schema files (or a subdirectory if the URI's
///   `subdir` knob is set) directly into `dest`. Cache layout,
///   content hashing, and TTL bookkeeping are the host's
///   responsibility; the fetcher just fetches.
/// - **Honour `uri.subdir` if present.** After extracting a tarball
///   or cloning a repo, copy the contents of `uri.subdir/` (relative
///   to the fetched root) into `dest`, not the whole repo. The
///   schema loader scans `dest` directly — it doesn't descend.
/// - **Return [`FetchError`] variants the host can surface.** Keep
///   the per-fetcher error type small; specific causes (HTTP status
///   code, git error code) go in the `Other` variant's message.
pub trait Fetcher: Send + Sync {
    /// Fetch `uri`'s contents into `dest`. `dest` is guaranteed to
    /// exist and be empty when this is called.
    fn fetch(&self, uri: &ParsedUri, dest: &Path) -> Result<(), FetchError>;

    /// URI schemes this fetcher handles. Typically a single-element
    /// slice (one fetcher per scheme), but a fetcher can claim
    /// multiple schemes if its implementation is shared — e.g.,
    /// [`GitFetcher`] claims both `git` and `git+ssh` because the
    /// underlying `git clone` accepts both URL forms.
    ///
    /// Returned as `&'static [&'static str]` so the
    /// [`super::registry::FetcherRegistry`] can build its scheme map
    /// without allocating.
    fn schemes(&self) -> &'static [&'static str];

    /// True when `rev` is an immutable reference (Git tag, content
    /// hash, SHA). Drives cache TTL: immutable refs cache
    /// indefinitely; mutable refs (branches, `None`) have a 24-hour
    /// TTL after which the cache invalidates and the next resolve
    /// re-fetches.
    ///
    /// Default: `false` for any input. Fetchers should override
    /// when they can confidently distinguish — e.g., [`GitFetcher`]
    /// would return `true` for `rev` matching `^[0-9a-f]{7,40}$`
    /// (SHA-ish) or `^v?\d+\.\d+`-ish (tag heuristic). Returning
    /// `false` from a default-impl-using fetcher is always safe
    /// (cache invalidates more often than necessary; never less).
    fn is_immutable_rev(&self, _rev: Option<&str>) -> bool {
        false
    }
}

/// Errors a [`Fetcher`] surfaces. Wrapped by [`super::ResolveError::Fetch`]
/// at the top-level resolve API.
#[derive(Debug)]
#[non_exhaustive]
pub enum FetchError {
    /// The fetcher hasn't been implemented yet — placeholder for the
    /// pre-implementation stubs. Real fetchers never return this.
    Unimplemented { scheme: String, message: String },
    /// Network IO failed (timeout, DNS, connection refused, …).
    Network { message: String },
    /// Server returned a non-success status (HTTP 4xx/5xx, git
    /// permission denied, …).
    UpstreamStatus { status: String, message: String },
    /// The fetched archive couldn't be extracted (corrupt tarball,
    /// unrecognised format, …).
    Extract { message: String },
    /// IO failed during the fetcher's local writes (out of disk,
    /// permission denied on the cache dir, …).
    Io(std::io::Error),
    /// Some other per-fetcher condition the variants above don't
    /// capture. Use sparingly — prefer adding a typed variant if the
    /// condition is recurring.
    Other { message: String },
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Unimplemented { scheme, message } => {
                write!(f, "`{scheme}:` resolver not implemented: {message}")
            }
            FetchError::Network { message } => write!(f, "network error: {message}"),
            FetchError::UpstreamStatus { status, message } => {
                write!(f, "upstream returned {status}: {message}")
            }
            FetchError::Extract { message } => write!(f, "archive extraction failed: {message}"),
            FetchError::Io(e) => write!(f, "fetcher io error: {e}"),
            FetchError::Other { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FetchError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FetchError {
    fn from(e: std::io::Error) -> Self {
        FetchError::Io(e)
    }
}

/// Stub for the `https:` transport. Performs a single HTTPS GET of a
/// tarball/zip and extracts it. Returns [`FetchError::Unimplemented`]
/// until the real network code lands; tracked at lex#562.
///
/// This is also the underlying transport that `github:` and `gitlab:`
/// URL templates expand into when their `via` knob picks https (the
/// default).
#[derive(Debug, Default, Clone, Copy)]
pub struct HttpsFetcher;

impl Fetcher for HttpsFetcher {
    fn fetch(&self, _uri: &ParsedUri, _dest: &Path) -> Result<(), FetchError> {
        Err(FetchError::Unimplemented {
            scheme: "https".into(),
            message: "https: resolver not yet implemented (tracked at lex#562); use path: or --ext-schema for local schemas".into(),
        })
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["https"]
    }
}

/// Stub for the `git:` / `git+ssh:` transport. Shells out to `git
/// clone` (preferred over libgit2 for credential coverage; see
/// `comms/specs/proposals/extending-lex-stores.lex` §6.3). Accepts any
/// URL form `git clone` accepts — `https://...git`, `git@host:path`,
/// `git+ssh://git@host/path` — across both registered schemes.
///
/// Returns [`FetchError::Unimplemented`] until the real shell-out code
/// lands; tracked at lex#562. This is also the underlying transport
/// that `github:` and `gitlab:` URL templates expand into when their
/// `via` knob picks git (for private repositories needing the user's
/// git credential setup).
#[derive(Debug, Default, Clone, Copy)]
pub struct GitFetcher;

impl Fetcher for GitFetcher {
    fn fetch(&self, _uri: &ParsedUri, _dest: &Path) -> Result<(), FetchError> {
        Err(FetchError::Unimplemented {
            scheme: "git".into(),
            message: "git: resolver not yet implemented (tracked at lex#562); use path: or --ext-schema for local schemas".into(),
        })
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["git", "git+ssh"]
    }
}
