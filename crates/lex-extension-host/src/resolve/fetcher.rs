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
//! Implementation status: `https:` ships a real fetcher (ureq + tar +
//! zip extraction, see [`HttpsFetcher`]). `git:` / `git+ssh:` ship as
//! a stub returning [`FetchError::Unimplemented`]; tracked at lex#650.

use std::path::Path;

#[cfg(feature = "https-fetcher")]
use std::io::Read;

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

/// HTTPS tarball/zip transport. Performs a single HTTPS GET against
/// the URI body, expects a `tar.gz` or `zip` archive in response, and
/// extracts it into the destination directory. Honors `uri.subdir`
/// for archives that wrap their content in a top-level directory (the
/// GitHub tarball API does this) or that ship schemas alongside
/// unrelated content.
///
/// This is also the underlying transport that `github:` and `gitlab:`
/// URL templates expand into when their `via` knob picks https (the
/// default — see [`super::template`]).
///
/// Auth is by way of an optional `Authorization` (or arbitrary)
/// header pass-through with `${ENV_VAR}` interpolation. Plumbing the
/// header through from `lex-config` is a follow-up (see issue #651);
/// for now the fetcher reads no headers from configuration.
///
/// Implementation notes:
///
/// - Sync via `ureq` — keeps tokio off the resolver boot path.
///   `rustls` + `webpki-roots` so HTTPS works without OS-OpenSSL.
/// - 256 MiB response cap — a pathological server can't OOM us.
/// - Path-traversal defence: archive members with absolute paths or
///   `..` components are rejected; symlinks are skipped.
///
/// See `comms/specs/proposals/extending-lex-stores.lex` §3.2 and §6.2.
#[derive(Debug, Default, Clone, Copy)]
pub struct HttpsFetcher;

/// Hard cap on archive size. 256 MiB is generous for any plausible
/// schema bundle; a tarball larger than this is almost certainly the
/// wrong artifact pointed at the wrong URI.
const HTTPS_RESPONSE_CAP_BYTES: u64 = 256 * 1024 * 1024;

/// Hard cap on error-response bodies. Error bodies don't need to be
/// large (they're consumed verbatim into a diagnostic string); a
/// hostile or misbehaving server returning a 500 with a 1 GiB body
/// shouldn't be allowed to OOM us via the error path either.
#[cfg(feature = "https-fetcher")]
const HTTPS_ERROR_BODY_CAP_BYTES: u64 = 64 * 1024;

/// Per-request connect timeout. The resolver runs at boot; a stalled
/// server shouldn't be able to hang it indefinitely.
#[cfg(feature = "https-fetcher")]
const HTTPS_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Overall read timeout. Covers both DNS-to-headers and headers-to-EOF
/// — generous enough for slow tarball fetches over flaky links, tight
/// enough that a wedged connection doesn't sit forever.
#[cfg(feature = "https-fetcher")]
const HTTPS_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

#[cfg(feature = "https-fetcher")]
impl Fetcher for HttpsFetcher {
    fn fetch(&self, uri: &ParsedUri, dest: &Path) -> Result<(), FetchError> {
        // ParsedUri::body for `https:` includes the leading `//`
        // (`//api.github.com/...`); ureq wants the full URL with
        // scheme, so reconstruct.
        let url = format!("https:{}", uri.body);

        let agent = ureq::AgentBuilder::new()
            .timeout_connect(HTTPS_CONNECT_TIMEOUT)
            .timeout_read(HTTPS_READ_TIMEOUT)
            .build();

        let response = agent
            .get(&url)
            .set(
                "User-Agent",
                "lex-extension-host (https://github.com/lex-fmt/lex)",
            )
            .call()
            .map_err(map_ureq_error)?;

        let content_type = response.header("Content-Type").map(|s| s.to_string());
        let format = super::extract::detect_format(content_type.as_deref(), &uri.body);

        // Stream the response body to a tempfile rather than buffering
        // the whole archive in memory. Schema bundles are typically
        // KB-MB but the cap is 256 MiB; the tempfile keeps resident
        // memory bounded for the pathological case. `zip::ZipArchive`
        // needs `Read + Seek`, which a File provides; `tar::Archive`
        // doesn't need Seek but accepts it.
        let mut response_reader = response.into_reader().take(HTTPS_RESPONSE_CAP_BYTES + 1);
        let mut temp = tempfile::tempfile().map_err(FetchError::Io)?;
        let written = std::io::copy(&mut response_reader, &mut temp).map_err(FetchError::Io)?;
        if written > HTTPS_RESPONSE_CAP_BYTES {
            return Err(FetchError::Extract {
                message: format!("response exceeded {HTTPS_RESPONSE_CAP_BYTES}-byte cap"),
            });
        }
        use std::io::Seek;
        temp.rewind().map_err(FetchError::Io)?;

        super::extract::extract_archive_into(temp, format, dest, uri.subdir.as_deref())
            .map_err(map_extract_error)?;

        Ok(())
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["https"]
    }
}

/// Stub HttpsFetcher impl for builds that disable the `https-fetcher`
/// feature (notably wasm32-unknown-unknown, where `ring`'s `getrandom`
/// dep doesn't compile). Returns [`FetchError::Unimplemented`] so the
/// trait shape stays uniform across feature variants — callers don't
/// need to special-case "is this build's HttpsFetcher real?"
#[cfg(not(feature = "https-fetcher"))]
impl Fetcher for HttpsFetcher {
    fn fetch(&self, _uri: &ParsedUri, _dest: &Path) -> Result<(), FetchError> {
        Err(FetchError::Unimplemented {
            scheme: "https".into(),
            message: "https: fetcher disabled at build time (the `https-fetcher` feature on lex-extension-host wasn't enabled — common for wasm targets where the underlying TLS chain doesn't compile)".into(),
        })
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["https"]
    }
}

#[cfg(feature = "https-fetcher")]
fn map_ureq_error(e: ureq::Error) -> FetchError {
    match e {
        ureq::Error::Status(code, response) => {
            // Cap the error-body read at 64 KiB. ureq's
            // `Response::into_string` reads without bound; a
            // misbehaving server returning a giant 4xx/5xx body could
            // bypass HTTPS_RESPONSE_CAP_BYTES (which only applies to
            // the success path) and exhaust memory on the error
            // diagnostic. 64 KiB is far more than any sane error body
            // would carry.
            let mut reader = response.into_reader().take(HTTPS_ERROR_BODY_CAP_BYTES);
            let mut buf = String::new();
            use std::io::Read as _;
            let _ = reader.read_to_string(&mut buf);
            FetchError::UpstreamStatus {
                status: format!("{code}"),
                message: if buf.is_empty() {
                    "<empty body>".into()
                } else {
                    buf
                },
            }
        }
        ureq::Error::Transport(t) => FetchError::Network {
            message: t.to_string(),
        },
    }
}

#[cfg(feature = "https-fetcher")]
fn map_extract_error(e: super::extract::ExtractError) -> FetchError {
    use super::extract::ExtractError;
    match e {
        ExtractError::Io(io_err) => FetchError::Io(io_err),
        other => FetchError::Extract {
            message: other.to_string(),
        },
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
