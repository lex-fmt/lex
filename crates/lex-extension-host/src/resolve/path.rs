//! `path:` scheme resolver — local filesystem paths.
//!
//! Not a [`super::Fetcher`] impl because `path:` is fundamentally
//! different from the remote schemes:
//!
//! - No network IO, so no cache.
//! - No `#rev` / `?subdir` (those are remote-only knobs; carrying
//!   them on a `path:` URI is rejected as a typo upstream of dispatch).
//! - Path is resolved against the workspace root, not fetched into a
//!   user-cache directory.
//!
//! So this module is invoked directly by
//! [`super::resolve_namespace_with`] when it sees `parsed.scheme ==
//! "path"`, before the registry/cache layers come into play. Keeping
//! `path:` outside the [`super::Fetcher`] trait avoids leaking
//! "what if the fetcher is supposed to not fetch anything?" into
//! the trait shape.

use std::path::{Path, PathBuf};

use super::uri::ParsedUri;
use super::{ResolveError, ResolvedNamespace};

/// Resolve a `path:` URI against `workspace_root`. Called from
/// [`super::resolve_namespace_with`] after URI parsing.
pub(super) fn resolve(
    parsed: &ParsedUri,
    original_uri: &str,
    workspace_root: &Path,
) -> Result<ResolvedNamespace, ResolveError> {
    debug_assert_eq!(parsed.scheme, "path");

    // Reject `path:dir#rev` / `path:dir?subdir=x` — `#` and `?` are
    // remote-only knobs. Silent stripping would hide typos like
    // `path:dir#main` (where the user almost certainly meant a
    // remote URI).
    if parsed.has_fragment() || parsed.has_query() {
        return Err(ResolveError::PathUriHasFragmentOrQuery {
            uri: original_uri.to_string(),
        });
    }

    let body = &parsed.body;
    let candidate = if Path::new(body).is_absolute() {
        PathBuf::from(body)
    } else {
        workspace_root.join(body)
    };

    // Lexical-only root-escape check — same invariant as the
    // includes resolver. We don't canonicalise (symlinks are the
    // loader's problem) but we reject `../`-walks past the root.
    //
    // `lexically_normalize` returns `None` when normalisation would
    // pop past an empty buffer — that's the case we care about.
    // A naive `pop` no-op makes `../../etc/passwd` collapse to
    // `etc/passwd`, and an equally-empty normalised root then
    // trivially passes `starts_with` — a directory-traversal bypass
    // when workspace_root was relative or shorter than the escape
    // depth. Returning `None` surfaces the escape attempt as a typed
    // error.
    let normalized = lexically_normalize(&candidate).ok_or_else(|| ResolveError::RootEscape {
        path: candidate.clone(),
    })?;
    let normalized_root =
        lexically_normalize(workspace_root).ok_or_else(|| ResolveError::RootEscape {
            path: candidate.clone(),
        })?;
    if !normalized.starts_with(&normalized_root) {
        return Err(ResolveError::RootEscape { path: candidate });
    }

    let metadata = std::fs::metadata(&candidate).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            ResolveError::PathNotADirectory {
                path: candidate.clone(),
            }
        } else {
            ResolveError::Io {
                path: candidate.clone(),
                source,
            }
        }
    })?;
    if !metadata.is_dir() {
        return Err(ResolveError::PathNotADirectory { path: candidate });
    }

    Ok(ResolvedNamespace {
        schema_dir: candidate,
        source_uri: original_uri.to_string(),
    })
}

/// Lexical normalisation: collapse `.` and `..` components without
/// touching the filesystem. Returns `None` when a `..` would pop
/// past an empty buffer — that case is the previously-discovered
/// directory-traversal bypass: a naive `pop` no-op makes
/// `../../etc/passwd` collapse to `etc/passwd`, and an equally-
/// emptied normalised root then trivially passes `starts_with`.
/// Returning `None` lets callers treat that as a typed root-escape
/// error.
fn lexically_normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(uri: &str) -> ParsedUri {
        ParsedUri::parse(uri).unwrap()
    }

    #[test]
    fn path_uri_resolves_to_directory() {
        let workspace = tempfile::tempdir().unwrap();
        let labels_dir = workspace.path().join("acme-labels");
        std::fs::create_dir(&labels_dir).unwrap();
        std::fs::write(
            labels_dir.join("task.yaml"),
            "schema_version: 1\nlabel: acme.task\n",
        )
        .unwrap();

        let resolved = resolve(
            &parse("path:acme-labels"),
            "path:acme-labels",
            workspace.path(),
        )
        .unwrap();
        assert_eq!(resolved.schema_dir, labels_dir);
        assert_eq!(resolved.source_uri, "path:acme-labels");
    }

    #[test]
    fn path_uri_with_absolute_path_resolves() {
        let workspace = tempfile::tempdir().unwrap();
        let labels_dir = workspace.path().join("labels");
        std::fs::create_dir(&labels_dir).unwrap();
        let uri = format!("path:{}", labels_dir.display());
        let resolved = resolve(&parse(&uri), &uri, workspace.path()).unwrap();
        assert_eq!(resolved.schema_dir, labels_dir);
    }

    #[test]
    fn path_uri_root_escape_is_rejected() {
        let workspace = tempfile::tempdir().unwrap();
        let err = resolve(
            &parse("path:../../../etc/passwd"),
            "path:../../../etc/passwd",
            workspace.path(),
        )
        .unwrap_err();
        assert!(matches!(err, ResolveError::RootEscape { .. }));
    }

    /// Directory-traversal bypass guard: when the `workspace_root`
    /// resolves to a relative path (e.g., `.`), a previous version
    /// of `lexically_normalize` silently swallowed `..` components
    /// on an empty buffer. So `../../etc/passwd` collapsed to
    /// `etc/passwd`, and an equally-empty normalised root made the
    /// `starts_with(root)` check pass — a directory-traversal bypass
    /// letting a malicious `lex.toml` reach arbitrary filesystem
    /// locations. The fix makes `lexically_normalize` return `None`
    /// on underflow; the caller surfaces that as
    /// `ResolveError::RootEscape`.
    #[test]
    fn relative_workspace_does_not_let_dotdot_escape() {
        let relative = std::path::Path::new(".");
        let err = resolve(
            &parse("path:../../../etc/passwd"),
            "path:../../../etc/passwd",
            relative,
        )
        .unwrap_err();
        assert!(
            matches!(err, ResolveError::RootEscape { .. }),
            "expected RootEscape, got: {err}"
        );
    }

    #[test]
    fn lexically_normalize_returns_none_on_underflow() {
        // Direct unit test on the normaliser so the bypass can't
        // re-emerge if someone changes the caller's check.
        assert!(lexically_normalize(std::path::Path::new("../foo")).is_none());
        assert!(lexically_normalize(std::path::Path::new("../../etc")).is_none());
        // Non-escaping paths still normalise.
        assert_eq!(
            lexically_normalize(std::path::Path::new("a/./b/../c")),
            Some(PathBuf::from("a/c"))
        );
    }

    #[test]
    fn path_uri_missing_directory_yields_path_not_a_directory() {
        let workspace = tempfile::tempdir().unwrap();
        let err = resolve(
            &parse("path:does-not-exist"),
            "path:does-not-exist",
            workspace.path(),
        )
        .unwrap_err();
        assert!(matches!(err, ResolveError::PathNotADirectory { .. }));
    }

    /// Regression: `path:dir?` (or `path:dir#`) with empty
    /// fragment/query bodies must still be rejected. Earlier the
    /// query parser collapsed `?` with empty body to `subdir: None`,
    /// which let `has_query()` return false and slipped the URI past
    /// this rejection — silently treating `path:dir?` as `path:dir`.
    /// The fix is in `parse_query`: empty `?` body parses to
    /// `Some("")` so the syntactic-presence check works.
    #[test]
    fn path_uri_with_empty_query_is_rejected() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = workspace.path().join("acme");
        std::fs::create_dir(&dir).unwrap();
        let err = resolve(&parse("path:acme?"), "path:acme?", workspace.path()).unwrap_err();
        assert!(
            matches!(err, ResolveError::PathUriHasFragmentOrQuery { .. }),
            "expected PathUriHasFragmentOrQuery, got: {err}"
        );
    }

    #[test]
    fn path_uri_with_empty_fragment_is_rejected() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = workspace.path().join("acme");
        std::fs::create_dir(&dir).unwrap();
        let err = resolve(&parse("path:acme#"), "path:acme#", workspace.path()).unwrap_err();
        assert!(
            matches!(err, ResolveError::PathUriHasFragmentOrQuery { .. }),
            "expected PathUriHasFragmentOrQuery, got: {err}"
        );
    }

    #[test]
    fn path_uri_pointing_at_file_yields_path_not_a_directory() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("not-a-dir.txt"), "x").unwrap();
        let err = resolve(
            &parse("path:not-a-dir.txt"),
            "path:not-a-dir.txt",
            workspace.path(),
        )
        .unwrap_err();
        assert!(matches!(err, ResolveError::PathNotADirectory { .. }));
    }
}
