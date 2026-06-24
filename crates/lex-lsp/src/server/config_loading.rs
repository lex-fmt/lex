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
    collect_extension_diagnostic_rules, resolve_include_root, LexConfig, LoadedLexConfig,
    CONFIG_FILE_NAME, DIAGNOSTICS_RULES_PATH,
};

// Re-export the shared `absolutize_path` under this module's path so existing
// `config_loading::absolutize_path` call sites keep working; the implementation
// now lives in `lex-config` (deduplicated from the copies in `lex-cli`).
pub(crate) use lex_config::absolutize_path;

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

/// Compute the include-resolution root for an entry document from the LSP's
/// [`LexConfig`]. Thin adapter over [`lex_config::resolve_include_root`] that
/// threads `[includes].root` through as the override.
pub(crate) fn inc_root_for(entry_path: &Path, cfg: &LexConfig) -> PathBuf {
    resolve_include_root(entry_path, cfg.includes.root.as_deref().map(Path::new))
}
