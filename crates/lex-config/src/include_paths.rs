//! Shared filesystem-path helpers for include resolution.
//!
//! The CLI (`convert` / `inspect` / `check`) and the LSP server all need to
//! turn an entry-file path + optional config root into the absolute,
//! lexically-normalized [`ResolveConfig::root`](lex-core) the resolver's
//! root-escape prefix check requires. These functions are the single source of
//! that logic — previously copy-pasted across `lex-cli` and `lex-lsp`.

use std::path::{Path, PathBuf};

use crate::CONFIG_FILE_NAME;

/// Best-effort absolutize: try [`Path::canonicalize`] first (handles symlinks
/// and resolves `..` against the real filesystem), falling back to
/// `current_dir().join(path)` when the path doesn't exist on disk yet. Always
/// returns an absolute path; the resolver requires one for its root-escape
/// prefix check to be sound.
pub fn absolutize_path(p: &Path) -> PathBuf {
    if let Ok(canon) = p.canonicalize() {
        return canon;
    }
    if p.is_absolute() {
        return p.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(p))
        .unwrap_or_else(|_| p.to_path_buf())
}

/// Walk upward from `start` looking for a directory containing
/// [`CONFIG_FILE_NAME`] (`.lex.toml`). Returns that directory, or `None` if the
/// filesystem root is reached without finding one.
pub fn find_nearest_config_dir(start: &Path) -> Option<PathBuf> {
    let mut cur: PathBuf = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    loop {
        if cur.join(CONFIG_FILE_NAME).is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Compute the include-resolution root for an entry document, applying:
/// 1. `root_override` (e.g. `[includes].root` / `--includes-root`) if set.
/// 2. Directory of the nearest `.lex.toml` walking up from the entry's dir.
/// 3. The entry document's own directory.
///
/// The result is always run through [`absolutize_path`]. A bare filename
/// (empty `parent()`) is treated as `.` so the ancestor walk still runs.
pub fn resolve_include_root(entry_path: &Path, root_override: Option<&Path>) -> PathBuf {
    let raw = if let Some(root) = root_override {
        root.to_path_buf()
    } else {
        let start = entry_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        find_nearest_config_dir(&start).unwrap_or(start)
    };
    absolutize_path(&raw)
}
