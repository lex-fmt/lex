//! Path resolution for include sites and node-attached file references.
//!
//! Resolution is purely lexical — no filesystem access. The resolver only
//! needs a stable identity for cycle detection and a uniform shape for the
//! root-escape prefix check; the [`FsLoader`](super::FsLoader) does the
//! real post-canonicalization security gate. Root-absolute paths (leading
//! `/`) resolve under the configured root; everything else resolves
//! relative to the authoring file's directory.

use super::errors::IncludeError;
use std::path::{Path, PathBuf};

/// Resolve a file-reference target string the same way the include
/// resolver resolves include paths.
///
/// Use this when consuming `ReferenceType::File { target }` (or any other
/// node-attached path) so that relative paths resolve from the *authoring*
/// file's directory, not from wherever the merged document happens to be
/// rooted. Pass `ref_origin` as the [`Range::origin_path`](crate::lex::ast::range::Range::origin_path)
/// of the inline's containing node (or `None` if the node was never
/// stamped — in that case the path is treated as if authored at the root).
///
/// Behaviour matches the include resolver:
/// - Root-absolute targets (leading `/`) resolve under `root`.
/// - Other targets resolve relative to `ref_origin`'s parent (or `root`
///   when `ref_origin` is `None`).
/// - The result is lexically normalized and checked against `root` —
///   paths that escape it return `RootEscape`.
///
/// This is a sister to the resolver's internal `resolve_path` and shares
/// the same lexical-normalization caveat: it does not touch the filesystem.
pub fn resolve_file_reference(
    target: &str,
    ref_origin: Option<&Path>,
    root: &Path,
) -> Result<PathBuf, IncludeError> {
    let host_dir: PathBuf = ref_origin
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());
    resolve_path(target, &host_dir, root)
}

pub(super) fn resolve_path(
    src: &str,
    host_dir: &Path,
    root: &Path,
) -> Result<PathBuf, IncludeError> {
    let candidate = if let Some(rel) = src.strip_prefix('/') {
        // Root-absolute (Lex spec convention): leading `/` means "from
        // the resolution root", not "filesystem root".
        root.join(rel)
    } else {
        // Anything else must be a relative path. Reject inputs the
        // host platform would treat as absolute (Windows `C:\foo`,
        // `\\server\share`, `\foo`) up front: the spec forbids
        // platform-absolute paths from entering the resolution
        // pipeline. Without this, `host_dir.join(src)` would silently
        // discard `host_dir` because Rust's `PathBuf::join` replaces
        // the base when the joined path is absolute. The downstream
        // root-escape check would still catch the security side, but
        // we'd surface a misleading "escapes root" error instead of
        // "absolute paths not allowed", and we'd be relying on
        // `PathBuf::join`'s override semantics for the security
        // outcome rather than holding the line at the input boundary.
        if Path::new(src).is_absolute() {
            return Err(IncludeError::AbsolutePath {
                path: PathBuf::from(src),
            });
        }
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
/// path to exist, which breaks tests that use [`MemoryLoader`](super::MemoryLoader).
/// The lexical version is sufficient for include-site path resolution because
/// the resolver only needs a stable identity for cycle detection and a uniform
/// shape for the root-escape prefix check.
///
/// `..` is collapsed only when the *last* component in the buffer is a
/// real directory name (`Component::Normal`). When the buffer is empty
/// or its last component is itself `..` (or a root marker), the new `..`
/// is *preserved* in the buffer.
///
/// This is what defeats `../../etc/passwd` from collapsing to
/// `etc/passwd` and bypassing the root-escape check — `PathBuf::pop`
/// would happily strip a `..` (since `Path::new("..").parent()` returns
/// `Some("")`), silently losing the second `..` and producing a path
/// that falsely starts with the root prefix. Each unmatched `..` in the
/// preserved form keeps the normalized path outside any sane root, so
/// the escape check fires correctly.
pub(super) fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                let can_pop = matches!(
                    out.components().next_back(),
                    Some(std::path::Component::Normal(_))
                );
                if can_pop {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
