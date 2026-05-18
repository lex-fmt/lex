//! Namespace URI resolver.
//!
//! A namespace declaration in `lex.toml` (or a `--ext-schema` flag)
//! gives the host a URI; the resolver turns that URI into a
//! filesystem directory the schema loader can scan. The model is
//! specified in `comms/specs/proposals/extending-lex-stores.lex` and
//! decomposes into:
//!
//! - **Three real transports**:
//!   - `path:` — built-in local filesystem read. Special-cased
//!     upstream of registry dispatch — no [`Fetcher`] impl, no cache.
//!   - `https:` — HTTPS GET of a tarball/zip. Implemented by the
//!     [`fetcher::HttpsFetcher`] in the registry.
//!   - `git:` / `git+ssh:` — git clone of a repository. Implemented
//!     by the [`fetcher::GitFetcher`] in the registry (claims both
//!     schemes).
//! - **N URL templates** that expand into one of the transports
//!   above before dispatch:
//!   - `github:owner/repo[#rev]` — github tarball (https) or clone (git).
//!   - `gitlab:owner/repo[#rev]` — gitlab archive (https) or clone (git).
//!
//! ## Architecture
//!
//! The resolver has four layers:
//!
//! - **URI parsing** ([`uri::ParsedUri`]) — splits the input string
//!   into `scheme`, `body`, `rev`, `subdir` components. Pure
//!   syntactic, no IO.
//! - **URL-template expansion** ([`template::expand`]) — pure
//!   functions that rewrite forge-shorthand URIs (`github:`,
//!   `gitlab:`) into transport URIs (`https:`, `git:`). No-op for
//!   URIs already in a transport scheme.
//! - **Fetchers** ([`Fetcher`] trait + per-transport impls) — each
//!   transport has an implementation that fetches the (expanded) URI's
//!   contents into a caller-provided directory. `path:` is built-in
//!   and special-cased (no network, no cache); the remote transports
//!   are pluggable via the [`FetcherRegistry`].
//! - **Cache** ([`ResolverCache`]) — content-keyed at
//!   `~/.cache/lex/labels/<hash>/`. Caches fetched directories
//!   indefinitely for immutable refs (tags, SHAs) and for a 24-hour
//!   TTL for mutable refs (branches, `None`). The fetcher tells the
//!   cache which a given `rev` is.
//!
//! ## Status
//!
//! All three transports ship today. `path:` is built-in and special-
//! cased upstream of registry dispatch; `https:` uses ureq + tar/zip
//! extraction (see [`fetcher::HttpsFetcher`]); `git:` / `git+ssh:`
//! shell out to `git clone --depth=1` (see [`fetcher::GitFetcher`]).
//! Custom registries can compose alternative or in-process fetchers
//! via [`FetcherRegistry::register`] — the rest of the pipeline picks
//! them up without changes.

pub mod cache;
#[cfg(feature = "https-fetcher")]
mod extract;
pub mod fetcher;
mod path;
pub mod registry;
mod template;
pub mod uri;

use std::path::{Path, PathBuf};

pub use cache::ResolverCache;
pub use fetcher::{FetchError, Fetcher};
pub use registry::{default_fetcher_registry, FetcherRegistry};
pub use uri::{ParsedUri, UriParseError};

/// One resolved namespace: where its schema files live on disk and
/// the canonical URI it came from. Returned by [`resolve_namespace`]
/// and [`resolve_namespace_with`].
#[derive(Debug, Clone)]
pub struct ResolvedNamespace {
    /// Directory the [`crate::SchemaLoader`] should scan for `.yaml`
    /// files.
    pub schema_dir: PathBuf,
    /// The URI the resolver was asked about — useful for diagnostics
    /// that want to remind the user which declaration they're
    /// looking at.
    pub source_uri: String,
}

/// Errors raised by [`resolve_namespace`] and [`resolve_namespace_with`].
#[derive(Debug)]
#[non_exhaustive]
pub enum ResolveError {
    /// URI didn't match any registered scheme. `scheme` is the actual
    /// missing scheme — for plain transport URIs that matches the
    /// scheme of `uri`, but for forge-template URIs (`github:`,
    /// `gitlab:`) it's the *expanded* transport scheme (typically
    /// `https`). That's what the diagnostic needs to name so the user
    /// understands which transport fetcher is missing from the
    /// registry, not just that the original URI failed.
    UnknownScheme { uri: String, scheme: String },
    /// URI failed to parse syntactically (bad fragment, missing
    /// scheme, …). Distinct from `UnknownScheme`: the URI is
    /// malformed at the lex layer, not just pointed at a scheme we
    /// don't know.
    UriParseError { uri: String, source: UriParseError },
    /// A `path:` URI pointed at a file that doesn't exist or isn't
    /// a directory.
    PathNotADirectory { path: PathBuf },
    /// `path:` URI resolved to a path that escapes the workspace
    /// root (relative paths like `../../etc/passwd`). Same
    /// invariant as the include-resolver — keeps a malicious
    /// `lex.toml` from pointing at arbitrary system locations.
    RootEscape { path: PathBuf },
    /// `path:` resolution failed at the filesystem layer (permission
    /// denied, broken symlink, …).
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// A `path:` URI carried a `#` fragment or `?` query — those
    /// are remote-only knobs (the resolver uses them on
    /// `github:`/`gitlab:`/etc. for `rev` and `subdir`). Rejecting
    /// instead of silently stripping surfaces typos like
    /// `path:dir#main` (where the user almost certainly meant a
    /// remote URI).
    PathUriHasFragmentOrQuery { uri: String },
    /// A registered fetcher returned an error during the network
    /// fetch. Wraps the per-fetcher error type for context.
    Fetch { uri: String, source: FetchError },
    /// The cache directory couldn't be created or written to.
    /// Distinct from a fetch IO error: this happens before we even
    /// call the fetcher.
    CacheIo {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::UnknownScheme { uri, scheme } => {
                // When the URI's original scheme equals the missing
                // scheme, no template expansion happened — give the
                // plain "unknown scheme" phrasing. Otherwise the user
                // wrote a forge template (`github:`/`gitlab:`) that
                // expanded into a transport scheme they haven't
                // registered; say that explicitly so the diagnostic
                // points at what's actually missing.
                let user_scheme = uri.split_once(':').map(|(s, _)| s).unwrap_or(uri);
                if user_scheme == scheme {
                    write!(
                        f,
                        "namespace URI `{uri}` uses transport scheme `{scheme}:` which has no registered fetcher (known: path:, https:, git:, git+ssh:, plus the github:/gitlab: URL templates)"
                    )
                } else {
                    write!(
                        f,
                        "namespace URI `{uri}` (a `{user_scheme}:` URL template) expands to transport scheme `{scheme}:` which has no registered fetcher (known: path:, https:, git:, git+ssh:)"
                    )
                }
            }
            ResolveError::UriParseError { uri, source } => {
                write!(f, "namespace URI `{uri}` is malformed: {source}")
            }
            ResolveError::PathNotADirectory { path } => write!(
                f,
                "namespace URI `path:{}` does not point at an existing directory",
                path.display()
            ),
            ResolveError::RootEscape { path } => write!(
                f,
                "namespace URI `path:{}` escapes the workspace root",
                path.display()
            ),
            ResolveError::Io { path, source } => {
                write!(f, "{}: namespace resolve io error: {source}", path.display())
            }
            ResolveError::PathUriHasFragmentOrQuery { uri } => write!(
                f,
                "namespace URI `{uri}` is a `path:` scheme but carries `#` or `?` — those are remote-only knobs. Drop the fragment/query, or switch to a remote scheme that supports them."
            ),
            ResolveError::Fetch { uri, source } => {
                write!(f, "namespace URI `{uri}` fetch failed: {source}")
            }
            ResolveError::CacheIo { path, source } => write!(
                f,
                "cache directory `{}` io error: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ResolveError::Io { source, .. } => Some(source),
            ResolveError::UriParseError { source, .. } => Some(source),
            ResolveError::Fetch { source, .. } => Some(source),
            ResolveError::CacheIo { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Resolve a namespace URI using the default fetcher registry and
/// cache. Convenience wrapper around [`resolve_namespace_with`] for
/// callers that don't need to override either.
///
/// The default registry ships real fetchers for the `https:` and
/// `git:` transports (the latter also claims `git+ssh:`). `github:`
/// and `gitlab:` are URL templates that expand into one of those
/// transports before dispatch.
///
/// The default cache lives at `$XDG_CACHE_HOME/lex/labels/` (falling
/// back to `~/.cache/lex/labels/` per XDG conventions). Cache
/// initialisation failures surface as [`ResolveError::CacheIo`].
pub fn resolve_namespace(
    uri: &str,
    workspace_root: &Path,
) -> Result<ResolvedNamespace, ResolveError> {
    let registry = default_fetcher_registry();
    let cache = ResolverCache::user_default().map_err(|source| ResolveError::CacheIo {
        path: ResolverCache::default_root(),
        source,
    })?;
    resolve_namespace_with(uri, workspace_root, &registry, &cache)
}

/// Resolve a namespace URI with an explicit fetcher registry and
/// cache. Used by [`crate::lex-fmt::boot_registry`] (one cache +
/// one registry constructed at boot, shared across all namespaces)
/// and by tests that want a tempdir cache or a custom fetcher.
///
/// Dispatch:
///
/// 1. Parse the URI ([`ParsedUri::parse`]). `path:` is special-cased
///    here — it bypasses templates, registry, and cache, resolving
///    directly against `workspace_root` like a local path.
/// 2. Run URL-template expansion ([`template::expand`]) on the parsed
///    URI. Forge shorthands (`github:`, `gitlab:`) become transport
///    URIs; transport URIs pass through unchanged.
/// 3. Look up the fetcher for the (expanded) URI's scheme in
///    `registry`. Return [`ResolveError::UnknownScheme`] if no fetcher
///    is registered.
/// 4. Consult `cache` for the URI+rev. If hit (and still valid by
///    TTL / immutability), return the cached path.
/// 5. Otherwise call `fetcher.fetch(uri, dest)` with a fresh cache
///    directory. Record the fetch timestamp in the cache. Return the
///    path on success.
pub fn resolve_namespace_with(
    uri: &str,
    workspace_root: &Path,
    registry: &FetcherRegistry,
    cache: &ResolverCache,
) -> Result<ResolvedNamespace, ResolveError> {
    let parsed = ParsedUri::parse(uri).map_err(|source| ResolveError::UriParseError {
        uri: uri.to_string(),
        source,
    })?;

    if parsed.scheme == "path" {
        return path::resolve(&parsed, uri, workspace_root);
    }

    let expanded = template::expand(parsed).map_err(|source| ResolveError::UriParseError {
        uri: uri.to_string(),
        source,
    })?;

    let fetcher = registry
        .get(&expanded.scheme)
        .ok_or_else(|| ResolveError::UnknownScheme {
            uri: uri.to_string(),
            scheme: expanded.scheme.clone(),
        })?;

    let schema_dir = cache.fetch_or_reuse(&expanded, fetcher.as_ref())?;

    Ok(ResolvedNamespace {
        schema_dir,
        source_uri: uri.to_string(),
    })
}

#[cfg(test)]
mod tests {
    //! Dispatch-level tests. Per-scheme behaviour is covered in the
    //! submodule tests (uri, path, cache, registry); these exercise
    //! the public [`resolve_namespace`] / [`resolve_namespace_with`]
    //! entry points and confirm errors thread through correctly.

    use super::*;

    fn fresh_cache() -> (tempfile::TempDir, ResolverCache) {
        let tmp = tempfile::tempdir().unwrap();
        let cache = ResolverCache::new(tmp.path()).unwrap();
        (tmp, cache)
    }

    #[test]
    fn unknown_scheme_yields_typed_error() {
        let workspace = tempfile::tempdir().unwrap();
        let registry = default_fetcher_registry();
        let (_tmp, cache) = fresh_cache();
        let err = resolve_namespace_with("ftp:server/path", workspace.path(), &registry, &cache)
            .unwrap_err();
        match err {
            ResolveError::UnknownScheme { uri, scheme } => {
                assert_eq!(uri, "ftp:server/path");
                assert_eq!(scheme, "ftp");
                // Plain transport URI (no template expansion) — the
                // diagnostic should NOT use the "expands to" phrasing
                // that's reserved for the template-expansion branch.
                // (The "known schemes" footer mentions URL templates
                // either way, so we discriminate on "expands to"
                // instead.)
                let msg = format!(
                    "{}",
                    ResolveError::UnknownScheme {
                        uri,
                        scheme: scheme.clone()
                    }
                );
                assert!(
                    !msg.contains("expands to"),
                    "plain transport URI shouldn't use template-expansion phrasing: {msg}"
                );
            }
            other => panic!("expected UnknownScheme, got: {other}"),
        }
    }

    #[test]
    fn unknown_scheme_after_template_expansion_names_transport() {
        // If a custom registry omits `https:`, a `github:` template
        // expansion still produces an https URI, and the error needs
        // to say "expands to transport scheme `https:`" rather than
        // misleadingly claiming `github:` is unknown.
        let workspace = tempfile::tempdir().unwrap();
        let registry = FetcherRegistry::new(); // empty — no https registered
        let (_tmp, cache) = fresh_cache();
        let err = resolve_namespace_with("github:acme/repo", workspace.path(), &registry, &cache)
            .unwrap_err();
        match err {
            ResolveError::UnknownScheme { uri, scheme } => {
                assert_eq!(uri, "github:acme/repo");
                assert_eq!(scheme, "https", "should report the expanded transport");
                let msg = format!(
                    "{}",
                    ResolveError::UnknownScheme {
                        uri: uri.clone(),
                        scheme: scheme.clone()
                    }
                );
                assert!(
                    msg.contains("expands to") && msg.contains("`https:`"),
                    "template-expansion diagnostic should name the expanded transport: {msg}"
                );
            }
            other => panic!("expected UnknownScheme, got: {other}"),
        }
    }

    #[test]
    fn malformed_uri_yields_parse_error() {
        let workspace = tempfile::tempdir().unwrap();
        let registry = default_fetcher_registry();
        let (_tmp, cache) = fresh_cache();
        let err =
            resolve_namespace_with("not-a-uri", workspace.path(), &registry, &cache).unwrap_err();
        assert!(matches!(err, ResolveError::UriParseError { .. }));
    }

    #[test]
    fn path_uri_dispatches_to_path_module() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = workspace.path().join("acme");
        std::fs::create_dir(&dir).unwrap();
        let registry = default_fetcher_registry();
        let (_tmp, cache) = fresh_cache();
        let resolved =
            resolve_namespace_with("path:acme", workspace.path(), &registry, &cache).unwrap();
        assert_eq!(resolved.schema_dir, dir);
    }

    #[test]
    fn convenience_resolve_namespace_works_for_path() {
        // The convenience entry point uses the default registry +
        // user-default cache. For path: URIs that don't touch the
        // cache, this should work even without a real cache dir
        // (the cache constructor creates ~/.cache/lex/labels if
        // missing, but the cache isn't consulted for path:).
        let workspace = tempfile::tempdir().unwrap();
        let dir = workspace.path().join("acme");
        std::fs::create_dir(&dir).unwrap();
        let resolved = resolve_namespace("path:acme", workspace.path()).unwrap();
        assert_eq!(resolved.schema_dir, dir);
    }
}
