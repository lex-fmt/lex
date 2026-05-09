//! Namespace URI resolver.
//!
//! A namespace declaration in `lex.toml` (or a `--ext-schema` flag)
//! gives the host a URI; the resolver turns that URI into a
//! filesystem directory the schema loader can scan. Five schemes
//! are specified in the proposal (§13):
//!
//! - `path:` — local filesystem path. No network, no cache.
//! - `github:` — `github:owner/repo[#rev][?subdir=…]`. Fetched +
//!   cached at `~/.cache/lex/labels/<content-hash>/`.
//! - `gitlab:` — same shape, gitlab.com.
//! - `https:` — generic HTTPS tarball.
//! - `git+ssh:` — explicit ssh remote.
//!
//! ## Status (PR 9)
//!
//! Only `path:` is implemented in this PR. The remote schemes
//! return [`ResolveError::Unimplemented`] with a clear message
//! pointing the user at `--ext-schema` for local schema loading.
//! Network resolvers + cache + 24-hour TTL + `lexd labels update`
//! land in a follow-up — that's the bulk of the resolver scope and
//! gets its own focused review surface.
//!
//! `path:` is enough for end-to-end demonstration of the dispatch
//! fabric: a `[labels]` block can point at a local schema directory
//! today, the registry boot loads schemas through the same path
//! the future remote resolvers will take, and the trust gate /
//! dispatch path are exercised in tests.

use std::path::{Path, PathBuf};

/// One resolved namespace: where its schema files live on disk and
/// the canonical URI it came from. Returned by [`resolve_namespace`].
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

/// Errors raised by [`resolve_namespace`].
#[derive(Debug)]
#[non_exhaustive]
pub enum ResolveError {
    /// URI didn't match any known scheme.
    UnknownScheme { uri: String },
    /// A `path:` URI pointed at a file that doesn't exist or isn't
    /// a directory.
    PathNotADirectory { path: PathBuf },
    /// The scheme is recognised but not yet wired up. Currently
    /// `github:`, `gitlab:`, `https:`, and `git+ssh:` all return
    /// this — network fetching + cache lands in a follow-up PR.
    Unimplemented { scheme: String, uri: String },
    /// `path:` URI resolved to a path that escapes the workspace
    /// root (relative paths like `../../etc/passwd`). Same
    /// invariant as the include-resolver — keeps a malicious
    /// `lex.toml` from pointing at arbitrary system locations.
    RootEscape { path: PathBuf },
    /// `path:` resolution failed at the filesystem layer
    /// (permission denied, broken symlink, …).
    Io {
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
            ResolveError::PathNotADirectory { path } => write!(
                f,
                "namespace URI `path:{}` does not point at an existing directory",
                path.display()
            ),
            ResolveError::Unimplemented { scheme, uri } => write!(
                f,
                "namespace URI scheme `{scheme}:` is not yet implemented (uri: `{uri}`); use `path:` or `--ext-schema` for local schemas in this release"
            ),
            ResolveError::RootEscape { path } => write!(
                f,
                "namespace URI `path:{}` escapes the workspace root",
                path.display()
            ),
            ResolveError::Io { path, source } => {
                write!(f, "{}: namespace resolve io error: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ResolveError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Resolve one namespace URI into a [`ResolvedNamespace`].
///
/// `workspace_root` is the directory `path:` URIs are resolved
/// relative to — usually the directory containing the `lex.toml`
/// the URI came from. `path:` URIs that, after joining with the
/// workspace root, lexically escape that root are rejected with
/// [`ResolveError::RootEscape`] (defence-in-depth against a
/// `lex.toml` pointing at `../../etc/`).
pub fn resolve_namespace(
    uri: &str,
    workspace_root: &Path,
) -> Result<ResolvedNamespace, ResolveError> {
    if let Some(rest) = uri.strip_prefix("path:") {
        return resolve_path(rest, uri, workspace_root);
    }
    for scheme in ["github", "gitlab", "https", "git+ssh"] {
        if uri.starts_with(&format!("{scheme}:")) {
            return Err(ResolveError::Unimplemented {
                scheme: scheme.to_string(),
                uri: uri.to_string(),
            });
        }
    }
    Err(ResolveError::UnknownScheme {
        uri: uri.to_string(),
    })
}

fn resolve_path(
    rest: &str,
    uri: &str,
    workspace_root: &Path,
) -> Result<ResolvedNamespace, ResolveError> {
    // Strip any URI fragment / query — `path:` doesn't honour `#rev`
    // or `?subdir=`, those are remote-only knobs. Surfacing as a
    // silent ignore would be confusing; instead we keep them in the
    // raw URI for diagnostics but the disk path is just the bare
    // value.
    let bare = rest
        .split_once('#')
        .map(|(p, _)| p)
        .unwrap_or(rest)
        .split_once('?')
        .map(|(p, _)| p)
        .unwrap_or(rest.split_once('#').map(|(p, _)| p).unwrap_or(rest));
    let candidate = if std::path::Path::new(bare).is_absolute() {
        PathBuf::from(bare)
    } else {
        workspace_root.join(bare)
    };
    // Lexical-only root-escape check — same invariant as the
    // includes resolver. We don't canonicalise (symlinks are the
    // loader's problem) but we do reject `../`-walks past the root.
    let normalized = lexically_normalize(&candidate);
    let normalized_root = lexically_normalize(workspace_root);
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
        source_uri: uri.to_string(),
    })
}

/// Lexical normalisation: collapse `.` and `..` components without
/// touching the filesystem. Same logic the include resolver uses.
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_yaml(dir: &tempfile::TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn path_uri_resolves_to_directory() {
        let workspace = tempfile::tempdir().unwrap();
        let labels_dir = workspace.path().join("acme-labels");
        std::fs::create_dir(&labels_dir).unwrap();
        write_yaml(
            &workspace,
            "acme-labels/task.yaml",
            "schema_version: 1\nlabel: acme.task\n",
        );

        let resolved = resolve_namespace("path:acme-labels", workspace.path()).unwrap();
        assert_eq!(resolved.schema_dir, labels_dir);
        assert_eq!(resolved.source_uri, "path:acme-labels");
    }

    #[test]
    fn path_uri_with_absolute_path_resolves() {
        let workspace = tempfile::tempdir().unwrap();
        let labels_dir = workspace.path().join("labels");
        std::fs::create_dir(&labels_dir).unwrap();
        let uri = format!("path:{}", labels_dir.display());
        let resolved = resolve_namespace(&uri, workspace.path()).unwrap();
        assert_eq!(resolved.schema_dir, labels_dir);
    }

    #[test]
    fn path_uri_root_escape_is_rejected() {
        let workspace = tempfile::tempdir().unwrap();
        let err = resolve_namespace("path:../../../etc/passwd", workspace.path()).unwrap_err();
        assert!(matches!(err, ResolveError::RootEscape { .. }));
    }

    #[test]
    fn path_uri_missing_directory_yields_path_not_a_directory() {
        let workspace = tempfile::tempdir().unwrap();
        let err = resolve_namespace("path:does-not-exist", workspace.path()).unwrap_err();
        assert!(matches!(err, ResolveError::PathNotADirectory { .. }));
    }

    #[test]
    fn path_uri_pointing_at_file_yields_path_not_a_directory() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("not-a-dir.txt"), "x").unwrap();
        let err = resolve_namespace("path:not-a-dir.txt", workspace.path()).unwrap_err();
        assert!(matches!(err, ResolveError::PathNotADirectory { .. }));
    }

    #[test]
    fn github_scheme_returns_unimplemented_with_clear_message() {
        let workspace = tempfile::tempdir().unwrap();
        let err = resolve_namespace("github:acme/lex-labels", workspace.path()).unwrap_err();
        match err {
            ResolveError::Unimplemented { scheme, .. } => assert_eq!(scheme, "github"),
            other => panic!("expected Unimplemented, got: {other}"),
        }
    }

    #[test]
    fn gitlab_https_git_ssh_all_unimplemented() {
        let workspace = tempfile::tempdir().unwrap();
        for scheme in ["gitlab", "https", "git+ssh"] {
            let uri = format!("{scheme}:foo/bar");
            let err = resolve_namespace(&uri, workspace.path()).unwrap_err();
            assert!(
                matches!(err, ResolveError::Unimplemented { .. }),
                "scheme={scheme}"
            );
        }
    }

    #[test]
    fn unknown_scheme_yields_typed_error() {
        let workspace = tempfile::tempdir().unwrap();
        let err = resolve_namespace("ftp:server/path", workspace.path()).unwrap_err();
        assert!(matches!(err, ResolveError::UnknownScheme { .. }));
    }
}
