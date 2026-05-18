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
//! Implementation status: both real transports ship today. `https:`
//! uses ureq + tar + zip extraction (see [`HttpsFetcher`]); `git:` /
//! `git+ssh:` shell out to `git clone --depth=1` (see [`GitFetcher`]).

use std::path::Path;
use std::process::Command;

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
#[cfg(feature = "https-fetcher")]
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

/// Git transport. Shells out to `git clone --depth=1` to fetch a
/// repository into the destination directory. Honors `uri.rev` as
/// `--branch` (branch or tag name) and `uri.subdir` to extract a
/// subdirectory of the repo as the schema root. Removes `.git/` after
/// clone — the cache only holds schema content.
///
/// Accepts both `git:` and `git+ssh:` schemes (claimed in
/// [`Self::schemes`]). The URL forms supported:
///
/// - `git:https://host/path/repo.git` — body is the full URL, passed
///   to `git clone` as-is.
/// - `git:git@host:owner/repo.git` — body is the scp-like URL, passed
///   to `git clone` as-is.
/// - `git:file:///path/to/bare` — body is a `file://` URL pointing at
///   a local bare repo (used in tests and as the local-mirror
///   escape hatch).
/// - `git+ssh://git@host/path/repo.git` — body is `//git@host/...`;
///   the fetcher reconstructs `git+ssh://...` for the clone command
///   (git accepts the `git+ssh` scheme directly).
///
/// ## Why shell out
///
/// Spec §6.3 spells out the reasoning. Briefly: libgit2's credential
/// coverage is incomplete in ways that matter (macOS keychain
/// integration, SAML SSO, Kerberos), so a libgit2-backed fetcher would
/// produce a UX divide between "private repos that work" and "private
/// repos that don't" with no clear story for the user. Shell-out
/// inherits everything `git clone` honors at the command line: SSH
/// agent, OS keychain helpers (osxkeychain, libsecret, GCM, GCMcore),
/// `gh auth setup-git`, `gitconfig`-declared SSO providers. There is
/// no Lex-side credential knob.
///
/// ## Constraints
///
/// `git` must be in `PATH`. The fetcher returns
/// [`FetchError::Other`] with a clear message if the binary isn't
/// found; the diagnostic surface tells the user to install git or fall
/// back to a [`path:`-scheme] / `--ext-schema` local schema. Spec §6.3
/// covers the rationale for not bundling git.
///
/// ## Errors
///
/// Git's stderr is classified into typed variants for the host
/// diagnostic surface:
///
/// - [`FetchError::Network`] for connectivity failures (DNS,
///   connection refused/timeout, unreachable).
/// - [`FetchError::UpstreamStatus`] for auth-shaped failures
///   (permission denied, authentication failed, repository not found
///   — which the github/gitlab APIs also use as a private-repo
///   not-authorised signal).
/// - [`FetchError::Other`] carrying the raw stderr text for anything
///   else (unknown ref, corrupted upstream, etc.).
///
/// ## Interaction with URL templates
///
/// This is the transport the `github:` and `gitlab:` URL templates
/// expand into when their `via` knob is `"git"` (the private-repo
/// path; default for those templates is `via = "https"`, which uses
/// the [`HttpsFetcher`] tarball API instead). See [`super::template`].
///
/// [`path:`-scheme]: super::path
#[derive(Debug, Default, Clone, Copy)]
pub struct GitFetcher;

impl Fetcher for GitFetcher {
    fn fetch(&self, uri: &ParsedUri, dest: &Path) -> Result<(), FetchError> {
        let url = reconstruct_git_url(&uri.scheme, &uri.body);

        // Normalize subdir (`/labels/`, `labels/`, `/labels` → `labels`).
        // Empty after trim → treat as no subdir.
        let subdir = uri
            .subdir
            .as_deref()
            .map(|s| s.trim_matches('/').to_string())
            .filter(|s| !s.is_empty());

        // Clone into a hidden subdirectory of dest, then promote the
        // desired contents (whole repo, or `subdir/` if set) up to
        // dest. Cloning into a subdirectory of dest avoids needing a
        // tempfile dep (the issue spec calls for std-only deps) and
        // avoids needing write access to dest's parent. The `.lex-` /
        // dot prefix means we won't shadow any real file the schema
        // ships.
        let clone_dir = dest.join(".lex-git-clone");

        let mut cmd = Command::new("git");
        cmd.arg("clone").arg("--depth=1");
        if let Some(rev) = uri.rev.as_deref().filter(|s| !s.is_empty()) {
            cmd.arg("--branch").arg(rev);
        }
        cmd.arg(&url).arg(&clone_dir);
        // Suppress interactive credential prompts; if the user's
        // credential helper can't satisfy the request non-interactively
        // we want a clean error rather than a hung boot path.
        cmd.env("GIT_TERMINAL_PROMPT", "0");

        let output = cmd.output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                FetchError::Other {
                    message: "git binary not in PATH; install git, or use a `path:` URI / `--ext-schema` flag for a local schema".into(),
                }
            } else {
                FetchError::Io(e)
            }
        })?;

        if !output.status.success() {
            // Best-effort cleanup of the partial clone dir before
            // surfacing the error.
            let _ = std::fs::remove_dir_all(&clone_dir);
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            return Err(classify_git_clone_error(&stderr));
        }

        // Source of the content we keep is either `<clone>/<subdir>`
        // or `<clone>` directly.
        let source = match subdir.as_deref() {
            Some(sub) => {
                let p = clone_dir.join(sub);
                if !p.is_dir() {
                    let _ = std::fs::remove_dir_all(&clone_dir);
                    return Err(FetchError::Other {
                        message: format!(
                            "subdir `{sub}` not found in cloned repo (clone succeeded but the path doesn't exist)"
                        ),
                    });
                }
                p
            }
            None => clone_dir.clone(),
        };

        // Copy contents into dest. Skip `.git` (the cache only holds
        // schema content) and skip the `.lex-git-clone` directory
        // itself (we're walking it as a source, but for the no-subdir
        // case it's literally a sibling of where we're writing, so
        // exclude it to avoid recursive copying).
        copy_dir_contents(&source, dest, &clone_dir).map_err(FetchError::Io)?;

        // Clean up the clone dir; we've copied what we need.
        std::fs::remove_dir_all(&clone_dir).map_err(FetchError::Io)?;

        Ok(())
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["git", "git+ssh"]
    }

    fn is_immutable_rev(&self, rev: Option<&str>) -> bool {
        is_immutable_git_rev(rev)
    }
}

/// Reconstruct the URL to pass to `git clone` from the parsed URI's
/// `(scheme, body)` pair.
///
/// - `git:<body>` → `<body>` (body is the verbatim URL — `https://...`,
///   `git@host:path`, `file:///...`, etc.).
/// - `git+ssh:<body>` (body is `//user@host/path`) → `git+ssh:<body>`.
///   Git accepts `git+ssh://...` as a synonym for `ssh://...` since
///   2.x.
fn reconstruct_git_url(scheme: &str, body: &str) -> String {
    match scheme {
        "git+ssh" => format!("git+ssh:{body}"),
        // `git:` and anything else (the registry only routes `git:` and
        // `git+ssh:` here, but be defensive in case a custom registry
        // routes another scheme).
        _ => body.to_string(),
    }
}

/// Classify git's stderr into a typed [`FetchError`]. The heuristics
/// are conservative — when we can't recognise the failure shape we
/// fall through to [`FetchError::Other`] with the raw stderr so the
/// user sees git's own message verbatim.
fn classify_git_clone_error(stderr: &str) -> FetchError {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("could not resolve host")
        || lower.contains("could not connect")
        || lower.contains("connection refused")
        || lower.contains("connection timed out")
        || lower.contains("network is unreachable")
        || lower.contains("no route to host")
    {
        FetchError::Network {
            message: stderr.trim().to_string(),
        }
    } else if lower.contains("permission denied")
        || lower.contains("authentication failed")
        || lower.contains("could not read username")
        || lower.contains("access denied")
        || lower.contains("repository not found")
    {
        // Note: `could not read from remote repository` is intentionally
        // NOT in this list. Git emits that line on most clone failures
        // (auth, missing repo, wrong endpoint, etc.) so it's too broad
        // to disambiguate. The auth-shaped failures all surface a more
        // specific marker above; everything else falls through to
        // FetchError::Other with git's raw stderr.
        FetchError::UpstreamStatus {
            status: "auth".into(),
            message: stderr.trim().to_string(),
        }
    } else {
        FetchError::Other {
            message: stderr.trim().to_string(),
        }
    }
}

/// True when `rev` looks like an immutable git reference. Drives the
/// cache TTL: immutable refs are cached indefinitely, mutable refs
/// expire after 24 hours.
///
/// Heuristics:
///
/// - SHA-shaped: 7-40 lowercase hex characters. Matches both
///   short-SHA (`abc1234`) and full-SHA (40-char) forms. Uppercase hex
///   is not matched — git itself emits lowercase, and matching
///   uppercase would expand the false-positive surface for branch
///   names that happen to be hex-shaped.
/// - Tag-shaped: optional `v` prefix, then `<digit>+.<digit>+`
///   (matches `v1.2`, `1.2`, `v0.14.0`, `1.2.3-rc4`, etc.). The
///   `\d+\.\d+` minimum requirement excludes single-digit "branches"
///   like `1` while keeping the common semver-prefixed tag shape.
///
/// Everything else (branch names, `None`) returns `false` — the cache
/// treats them as mutable and invalidates on TTL.
fn is_immutable_git_rev(rev: Option<&str>) -> bool {
    let Some(rev) = rev else { return false };
    let bytes = rev.as_bytes();

    // SHA: 7-40 lowercase hex digits, nothing else.
    if (7..=40).contains(&bytes.len())
        && bytes
            .iter()
            .all(|&b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return true;
    }

    // Tag: optional `v` prefix, then `<digits>.<digits>` (more
    // components allowed after; we only require the first two).
    let after_v = bytes.strip_prefix(b"v").unwrap_or(bytes);
    let mut parts = after_v.split(|&b| b == b'.');
    let (Some(first), Some(second)) = (parts.next(), parts.next()) else {
        return false;
    };
    !first.is_empty()
        && first.iter().all(|b| b.is_ascii_digit())
        && !second.is_empty()
        // Second component can have trailing non-digit characters
        // (e.g. `1.2.3-rc4` → second = `2`; e.g. `1.2-pre` → second =
        // `2-pre`). Require at least one leading digit, allow whatever
        // after — git tag names can be arbitrarily decorated.
        && second.iter().take_while(|b| b.is_ascii_digit()).count() > 0
}

/// Recursively copy contents of `src` into `dest`. Skips:
///
/// - The top-level `.git` directory (we don't need git's index /
///   objects in the cache — only the schema content).
/// - Anything matching `skip_path` (used to skip the
///   `.lex-git-clone/` directory itself when `src` is its sibling, so
///   the copy doesn't recursively follow into the source-of-truth).
/// - Symlinks (same trust-surface reasoning as the extract module —
///   archive/repo-shipped symlinks expand what the schema loader
///   trusts).
/// - Special files (sockets, FIFOs).
fn copy_dir_contents(src: &Path, dest: &Path, skip_path: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        if src_path == skip_path {
            continue;
        }
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let dest_path = dest.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            std::fs::create_dir_all(&dest_path)?;
            copy_dir_contents_no_skip(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dest_path)?;
        }
        // Anything else (sockets, FIFOs, etc.) — skip.
    }
    Ok(())
}

/// Inner recursion that doesn't reapply the top-level skip rules.
/// Nested directories shouldn't filter `.git` (a real `.git` deeper
/// in the tree is regular content, not metadata) or `.lex-git-clone`
/// (only the outermost level is a sibling of the source).
fn copy_dir_contents_no_skip(src: &Path, dest: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            std::fs::create_dir_all(&dest_path)?;
            copy_dir_contents_no_skip(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod git_helper_tests {
    use super::*;

    // ---- is_immutable_git_rev ----

    #[test]
    fn immutable_rev_full_sha() {
        assert!(is_immutable_git_rev(Some(
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"
        )));
    }

    #[test]
    fn immutable_rev_short_sha_seven_chars() {
        assert!(is_immutable_git_rev(Some("a1b2c3d")));
    }

    #[test]
    fn immutable_rev_rejects_sha_below_seven_chars() {
        assert!(!is_immutable_git_rev(Some("abc123")));
    }

    #[test]
    fn immutable_rev_rejects_uppercase_hex() {
        // We only match lowercase — branch names that happen to be
        // uppercase-hex-shaped shouldn't false-match as SHAs.
        assert!(!is_immutable_git_rev(Some("ABC1234")));
    }

    #[test]
    fn immutable_rev_semver_tag_with_v_prefix() {
        assert!(is_immutable_git_rev(Some("v1.2.0")));
        assert!(is_immutable_git_rev(Some("v0.14.0")));
    }

    #[test]
    fn immutable_rev_semver_tag_without_v_prefix() {
        assert!(is_immutable_git_rev(Some("1.2")));
        assert!(is_immutable_git_rev(Some("1.2.3")));
    }

    #[test]
    fn immutable_rev_semver_tag_with_decoration() {
        // Tag names can carry decorations after the version
        // (`-rc4`, `-pre`, etc.). The heuristic accepts these.
        assert!(is_immutable_git_rev(Some("v1.2.3-rc4")));
        assert!(is_immutable_git_rev(Some("1.2-pre")));
    }

    #[test]
    fn immutable_rev_rejects_single_digit_branch_lookalike() {
        // `1` alone isn't enough — no minor component.
        assert!(!is_immutable_git_rev(Some("1")));
        assert!(!is_immutable_git_rev(Some("v1")));
    }

    #[test]
    fn immutable_rev_rejects_branch_names() {
        assert!(!is_immutable_git_rev(Some("main")));
        assert!(!is_immutable_git_rev(Some("master")));
        assert!(!is_immutable_git_rev(Some("feature/foo")));
        assert!(!is_immutable_git_rev(Some("release-2026-05")));
    }

    #[test]
    fn immutable_rev_rejects_none() {
        assert!(!is_immutable_git_rev(None));
    }

    #[test]
    fn immutable_rev_rejects_empty_string() {
        assert!(!is_immutable_git_rev(Some("")));
    }

    // ---- reconstruct_git_url ----

    #[test]
    fn reconstruct_url_git_scheme_passes_body_verbatim() {
        assert_eq!(
            reconstruct_git_url("git", "https://host/path/repo.git"),
            "https://host/path/repo.git"
        );
        assert_eq!(
            reconstruct_git_url("git", "git@host:owner/repo.git"),
            "git@host:owner/repo.git"
        );
        assert_eq!(
            reconstruct_git_url("git", "file:///tmp/bare"),
            "file:///tmp/bare"
        );
    }

    #[test]
    fn reconstruct_url_git_ssh_scheme_rebuilds_full_url() {
        // ParsedUri::parse("git+ssh://git@host/path.git") gives
        // body = "//git@host/path.git"; the fetcher reconstructs
        // the full URL by prepending the scheme.
        assert_eq!(
            reconstruct_git_url("git+ssh", "//git@host/path.git"),
            "git+ssh://git@host/path.git"
        );
    }

    // ---- classify_git_clone_error ----

    #[test]
    fn classify_dns_failure_is_network() {
        let err = classify_git_clone_error(
            "fatal: unable to access 'https://nonexistent.example/r.git/': Could not resolve host: nonexistent.example",
        );
        assert!(matches!(err, FetchError::Network { .. }), "got: {err:?}");
    }

    #[test]
    fn classify_connection_refused_is_network() {
        let err = classify_git_clone_error(
            "fatal: unable to access 'https://localhost:1/r.git/': Failed to connect to localhost port 1: Connection refused",
        );
        assert!(matches!(err, FetchError::Network { .. }), "got: {err:?}");
    }

    #[test]
    fn classify_auth_failure_is_upstream_status() {
        let err = classify_git_clone_error(
            "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.",
        );
        assert!(
            matches!(err, FetchError::UpstreamStatus { .. }),
            "got: {err:?}"
        );
    }

    #[test]
    fn classify_repository_not_found_is_upstream_status() {
        // GitHub's "private repo without auth" surfaces as
        // "Repository not found" — semantically an auth failure (the
        // public can't see it).
        let err = classify_git_clone_error(
            "remote: Repository not found.\nfatal: repository 'https://github.com/private/secret.git/' not found",
        );
        assert!(
            matches!(err, FetchError::UpstreamStatus { .. }),
            "got: {err:?}"
        );
    }

    #[test]
    fn classify_unknown_ref_falls_through_to_other() {
        let err = classify_git_clone_error(
            "warning: Could not find remote branch nonexistent to clone.\nfatal: Remote branch nonexistent not found in upstream origin",
        );
        assert!(matches!(err, FetchError::Other { .. }), "got: {err:?}");
    }
}
