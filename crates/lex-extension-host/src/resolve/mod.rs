//! Namespace URI resolver.
//!
//! A namespace declaration in `lex.toml` (or a `--ext-schema` flag)
//! gives the host a URI; the resolver turns that URI into a
//! filesystem directory the schema loader can scan. Five schemes
//! are specified in the proposal (§4.2):
//!
//! - `path:` — local filesystem path. No network, no cache. Resolved
//!   relative to the workspace root unless absolute.
//! - `github:` — `github:owner/repo[#rev][?subdir=…]`. Fetched +
//!   cached at `~/.cache/lex/labels/<hash>/`.
//! - `gitlab:` — same shape, gitlab.com.
//! - `https:` — generic HTTPS tarball.
//! - `git+ssh:` — explicit ssh remote.
//!
//! ## Architecture
//!
//! The resolver has three layers:
//!
//! - **URI parsing** ([`uri::ParsedUri`]) — splits the input string
//!   into `scheme`, `body`, `rev`, `subdir` components. Pure
//!   syntactic, no IO.
//! - **Fetchers** ([`Fetcher`] trait + per-scheme impls) — each
//!   scheme has an implementation that fetches the URI's contents
//!   into a caller-provided directory. `path:` is built-in and
//!   special-cased (no network, no cache); the four remote schemes
//!   are pluggable via the [`FetcherRegistry`].
//! - **Cache** ([`ResolverCache`]) — content-keyed at
//!   `~/.cache/lex/labels/<hash>/`. Caches fetched directories
//!   indefinitely for immutable refs (tags, SHAs) and for a 24-hour
//!   TTL for mutable refs (branches, `None`). The fetcher tells the
//!   cache which a given `rev` is.
//!
//! ## Status (post-machinery PR)
//!
//! The machinery (trait, registry, cache, dispatch) is in place.
//! `path:` is the only fully-implemented scheme; the four remote
//! schemes ship as stubs that return
//! [`FetchError::Unimplemented`] when the dispatch reaches them.
//! Per-scheme network implementations are tracked at
//! [lex#562](https://github.com/lex-fmt/lex/issues/562) — implementers
//! plug their fetchers into [`default_fetcher_registry`] (or compose
//! a custom registry) and the rest of the pipeline picks them up
//! without changes.

pub mod cache;
pub mod fetcher;
mod path;
pub mod registry;
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
    /// URI didn't match any registered scheme.
    UnknownScheme { uri: String },
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
            ResolveError::UnknownScheme { uri } => write!(
                f,
                "namespace URI `{uri}` does not start with a known scheme (path:, github:, gitlab:, https:, git+ssh:)"
            ),
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
/// The default registry has stub fetchers for `github:`, `gitlab:`,
/// `https:`, and `git+ssh:` that return [`FetchError::Unimplemented`]
/// — same observable behaviour as the pre-machinery resolver.
/// Per-scheme implementations are tracked at
/// [lex#562](https://github.com/lex-fmt/lex/issues/562).
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
///    here — it bypasses the registry + cache entirely and resolves
///    against `workspace_root` like a local path.
/// 2. Look up the fetcher for the URI's scheme in `registry`. Return
///    [`ResolveError::UnknownScheme`] if no fetcher is registered.
/// 3. Consult `cache` for the URI+rev. If hit (and still valid by
///    TTL / immutability), return the cached path.
/// 4. Otherwise call `fetcher.fetch(uri, dest)` with a fresh cache
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

    let fetcher = registry
        .get(&parsed.scheme)
        .ok_or_else(|| ResolveError::UnknownScheme {
            uri: uri.to_string(),
        })?;

    let schema_dir = cache.fetch_or_reuse(&parsed, fetcher.as_ref())?;

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
        assert!(matches!(err, ResolveError::UnknownScheme { .. }));
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
    fn github_scheme_through_default_registry_yields_unimplemented_fetch() {
        let workspace = tempfile::tempdir().unwrap();
        let registry = default_fetcher_registry();
        let (_tmp, cache) = fresh_cache();
        let err = resolve_namespace_with(
            "github:acme/lex-labels",
            workspace.path(),
            &registry,
            &cache,
        )
        .unwrap_err();
        match err {
            ResolveError::Fetch {
                source: FetchError::Unimplemented { scheme, .. },
                ..
            } => assert_eq!(scheme, "github"),
            other => panic!("expected Fetch(Unimplemented {{ github }}), got: {other}"),
        }
    }

    #[test]
    fn gitlab_https_git_ssh_all_unimplemented_through_default_registry() {
        let workspace = tempfile::tempdir().unwrap();
        let registry = default_fetcher_registry();
        let (_tmp, cache) = fresh_cache();
        for scheme in ["gitlab", "https", "git+ssh"] {
            let uri = if scheme == "https" || scheme == "git+ssh" {
                format!("{scheme}://example.com/foo")
            } else {
                format!("{scheme}:foo/bar")
            };
            let err =
                resolve_namespace_with(&uri, workspace.path(), &registry, &cache).unwrap_err();
            match err {
                ResolveError::Fetch {
                    source: FetchError::Unimplemented { scheme: s, .. },
                    ..
                } => assert_eq!(s, scheme, "wrong scheme reported"),
                other => panic!("expected Fetch(Unimplemented) for {scheme}, got: {other}"),
            }
        }
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
