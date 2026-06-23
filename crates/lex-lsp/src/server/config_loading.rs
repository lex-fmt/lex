//! Configuration + path resolution for the language server.
//!
//! Loads [`LoadedLexConfig`] via clapfig (workspace-root-aware) and
//! computes the filesystem anchors include resolution needs: the nearest
//! `.lex.toml` directory, the include root for an entry document, and the
//! best-matching workspace root for a given file. All path helpers return
//! absolute, lexically-normalized paths so the resolver's root-escape
//! prefix check stays sound.

use std::path::{Path, PathBuf};

use clapfig::{Boundary, Clapfig, SearchPath};
use lex_config::{
    collect_extension_diagnostic_rules, LexConfig, LoadedLexConfig, CONFIG_FILE_NAME,
    DIAGNOSTICS_RULES_PATH,
};

/// Load a [`LoadedLexConfig`] via clapfig, searching from an optional
/// workspace root. The wrapper carries both the typed [`LexConfig`] and
/// the side-channel map of extension-emitted diagnostic rules captured
/// from `[diagnostics.rules]` via the `on_unknown_key` callback.
pub(crate) fn load_config(workspace_root: Option<&Path>) -> LoadedLexConfig {
    let mut search_paths = vec![SearchPath::Platform];
    if let Some(root) = workspace_root {
        search_paths.push(SearchPath::Path(root.to_path_buf()));
    } else {
        search_paths.push(SearchPath::Ancestors(Boundary::Marker(".git")));
        search_paths.push(SearchPath::Cwd);
    }
    load_with(search_paths, false).unwrap_or_else(|_| {
        // Fall back to compiled defaults if config loading fails.
        load_with(vec![], true).expect("compiled defaults must load")
    })
}

pub(crate) fn load_with(
    search_paths: Vec<SearchPath>,
    no_env: bool,
) -> std::result::Result<LoadedLexConfig, clapfig::ClapfigError> {
    let mut builder = Clapfig::schema_builder::<LexConfig>()
        .app_name("lex")
        .file_name(CONFIG_FILE_NAME)
        .search_paths(search_paths)
        .accept_dotted_extension_keys_in(
            DIAGNOSTICS_RULES_PATH,
            clapfig::UnknownKeyDecision::Collect,
        );
    if no_env {
        builder = builder.no_env();
    }
    let (config, unknowns) = builder.load_with_unknowns()?;
    Ok(LoadedLexConfig {
        config,
        extension_diagnostic_rules: collect_extension_diagnostic_rules(unknowns),
    })
}

pub(crate) fn best_matching_root(roots: &[PathBuf], document_path: &Path) -> Option<PathBuf> {
    roots
        .iter()
        .filter(|root| document_path.starts_with(root))
        .max_by_key(|root| root.components().count())
        .cloned()
}

/// Compute the include-resolution root for an entry document.
///
/// Order:
/// 1. `[includes].root` from `LexConfig` if set.
/// 2. Directory of the nearest `.lex.toml` walking upward from the
///    entry document's directory.
/// 3. The entry document's own directory.
///
/// Always returns an absolute, lexically-normalized path so the
/// resolver's root-escape prefix check is sound.
pub(crate) fn inc_root_for(entry_path: &Path, cfg: &LexConfig) -> PathBuf {
    let raw = if let Some(root) = cfg.includes.root.as_ref() {
        PathBuf::from(root)
    } else {
        let start = entry_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        find_nearest_config_dir(&start).unwrap_or(start)
    };
    absolutize_path(&raw)
}

/// Walk upward from `start` looking for a directory that contains
/// `.lex.toml`. Returns that directory, or `None` if we hit the
/// filesystem root without finding one.
pub(crate) fn find_nearest_config_dir(start: &Path) -> Option<PathBuf> {
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

/// Best-effort absolutize: try `Path::canonicalize` first (handles
/// symlinks + resolves `..` against the real filesystem), falling back
/// to `current_dir().join(path)` if the path doesn't exist on disk.
/// Always returns an absolute path; `ResolveConfig::root` requires one
/// for the root-escape prefix check to be sound.
pub(crate) fn absolutize_path(p: &Path) -> PathBuf {
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
