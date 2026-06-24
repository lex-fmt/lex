//! Shared support for the `lexd` binary's command handlers.
//!
//! Cross-cutting helpers used by more than one command in [`crate::commands`]:
//! include-resolution options ([`IncludeOptions`]), path helpers
//! ([`find_nearest_lex_toml_dir`], [`absolutize_path`]), source reading
//! ([`read_source`]), the mojibake-scanning loader decorator
//! ([`MojibakeScanningLoader`]), warning gating ([`warnings_enabled`]), and
//! the config→params translators ([`build_inspect_params`],
//! [`formatting_rules_from_config`], [`pdf_params_from_config`]) plus the
//! [`format_tier`] label.

use clap::ArgMatches;
use lex_babel::formats::lex::formatting_rules::FormattingRules;
use lex_config::{resolve_include_root, LexConfig, PdfPageSize};
use lex_core::lex::includes::{LoadError, LoadedFile, Loader};
use lex_core::lex::mojibake::detect_mojibake;
use std::collections::HashMap;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// Path helpers now live in `lex-config` (deduplicated from the copies that
// were in `lex-lsp` and `check.rs`); re-exported here under their existing
// names so the command handlers' call sites are unchanged.
pub(crate) use lex_config::{
    absolutize_path, find_nearest_config_dir as find_nearest_lex_toml_dir,
};

/// Per-invocation include resolution settings derived from CLI flags +
/// `[includes]` config + the entry-file's location.
#[derive(Debug, Clone)]
pub(crate) struct IncludeOptions {
    /// `true` to expand `lex.include` annotations during conversion/inspect.
    /// Always `false` for `lex format` (per spec §11.4) and when
    /// `--no-includes` is passed.
    pub(crate) enabled: bool,
    /// Explicit root override (`--includes-root` flag or `[includes].root`
    /// in `.lex.toml`). When `None`, the resolver picks the nearest
    /// `.lex.toml` walking up from the entry file, falling back to the
    /// entry file's own directory.
    pub(crate) root_override: Option<PathBuf>,
    /// Maximum include depth, taken from `[includes].max_depth`
    /// (default 8).
    pub(crate) max_depth: usize,
    /// Maximum total include count, taken from
    /// `[includes].max_total_includes` (default 1000).
    pub(crate) max_total_includes: usize,
    /// Maximum size of any single included file in bytes, taken from
    /// `[includes].max_file_size` (default 10 MiB).
    pub(crate) max_file_size: u64,
}

impl IncludeOptions {
    /// Build options for an "expand by default" command (convert / inspect).
    pub(crate) fn for_expanding_command(matches: &ArgMatches, config: &LexConfig) -> Self {
        Self {
            enabled: !matches.get_flag("no-includes"),
            root_override: matches
                .get_one::<String>("includes-root")
                .map(PathBuf::from)
                .or_else(|| config.includes.root.as_ref().map(PathBuf::from)),
            max_depth: config.includes.max_depth,
            max_total_includes: config.includes.max_total_includes,
            max_file_size: config.includes.max_file_size,
        }
    }

    /// Disabled options for `lex format` (formatter never expands per spec §11.4).
    pub(crate) fn for_format_command() -> Self {
        Self {
            enabled: false,
            root_override: None,
            max_depth: 8,
            max_total_includes: 1000,
            max_file_size: 10 * 1024 * 1024,
        }
    }

    /// Resolution root for an entry file at `entry_path`. Thin adapter over
    /// [`lex_config::resolve_include_root`] threading `root_override` through.
    pub(crate) fn resolved_root(&self, entry_path: &Path) -> PathBuf {
        resolve_include_root(entry_path, self.root_override.as_deref())
    }
}

/// Read source content from a file path, or from stdin when the path is
/// omitted. Exits with an error if no path is given and stdin is a terminal
/// (i.e. the user forgot to pipe input).
pub(crate) fn read_source(path: Option<&str>) -> String {
    match path {
        Some(p) => fs::read_to_string(p).unwrap_or_else(|e| {
            eprintln!("Error reading file '{p}': {e}");
            std::process::exit(1);
        }),
        None => {
            if io::stdin().is_terminal() {
                eprintln!(
                    "Error: no input file provided and stdin is a terminal. \
                     Pass a file path or pipe content via stdin."
                );
                std::process::exit(1);
            }
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
                eprintln!("Error reading from stdin: {e}");
                std::process::exit(1);
            });
            buf
        }
    }
}

/// Loader decorator that records the canonical path of any file whose
/// source text trips the mojibake detector, then delegates to the
/// inner loader unchanged. The CLI uses this to surface a per-file
/// warning for content pulled in by `:: lex.include ::` — content the
/// entry-source mojibake scan can't see on its own.
pub(crate) struct MojibakeScanningLoader<L: Loader> {
    inner: L,
    scan_enabled: bool,
    findings: Arc<std::sync::Mutex<Vec<PathBuf>>>,
}

impl<L: Loader> MojibakeScanningLoader<L> {
    pub(crate) fn new(inner: L, scan_enabled: bool) -> Self {
        Self {
            inner,
            scan_enabled,
            findings: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn findings(&self) -> Arc<std::sync::Mutex<Vec<PathBuf>>> {
        Arc::clone(&self.findings)
    }
}

impl<L: Loader> Loader for MojibakeScanningLoader<L> {
    fn load(&self, path: &Path) -> Result<LoadedFile, LoadError> {
        let loaded = self.inner.load(path)?;
        if self.scan_enabled && detect_mojibake(&loaded.source).is_some() {
            let mut findings = self.findings.lock().expect("findings mutex");
            findings.push(loaded.canonical_path.clone());
        }
        Ok(loaded)
    }
}

/// Returns true when CLI warnings should be printed to stderr. Off when
/// either `--no-warnings` was passed or `LEX_QUIET` is set to a
/// non-empty, non-zero value.
pub(crate) fn warnings_enabled(matches: &ArgMatches) -> bool {
    if matches.get_flag("no-warnings") {
        return false;
    }
    !matches!(std::env::var("LEX_QUIET"), Ok(v) if !v.is_empty() && v != "0")
}

/// Returns a short tier label for a format name, used by
/// `lexd --list-transforms` to make the v1 scope visible at a glance.
/// See `comms/docs/interop-scope.lex` for the full tiering.
pub(crate) fn format_tier(name: &str) -> &'static str {
    match name {
        "lex" => "[core]",
        "markdown" => "[core, both directions]",
        "html" => "[core, export only]",
        "pdf" => "[core, export only]",
        "png" => "[core, export only]",
        "rfc_xml" => "[experimental, import only]",
        "tag" | "treeviz" | "linetreeviz" => "[diagnostic]",
        _ => "",
    }
}

pub(crate) fn formatting_rules_from_config(config: &LexConfig) -> FormattingRules {
    let cfg = &config.formatting.rules;
    FormattingRules {
        session_blank_lines_before: cfg.session_blank_lines_before,
        session_blank_lines_after: cfg.session_blank_lines_after,
        normalize_seq_markers: cfg.normalize_seq_markers,
        unordered_seq_marker: cfg.unordered_seq_marker,
        max_blank_lines: cfg.max_blank_lines,
        indent_string: cfg.indent_string.clone(),
        preserve_trailing_blanks: cfg.preserve_trailing_blanks,
        normalize_verbatim_markers: cfg.normalize_verbatim_markers,
    }
}

pub(crate) fn build_inspect_params(config: &LexConfig) -> HashMap<String, String> {
    let mut params = HashMap::new();

    if config.inspect.ast.include_all_properties {
        params.insert("ast-full".to_string(), "true".to_string());
    }

    params.insert(
        "show-linum".to_string(),
        config.inspect.ast.show_line_numbers.to_string(),
    );

    if config.inspect.nodemap.color_blocks {
        params.insert("color".to_string(), "true".to_string());
    }
    if config.inspect.nodemap.color_characters {
        params.insert("color-char".to_string(), "true".to_string());
    }
    if config.inspect.nodemap.show_summary {
        params.insert("nodesummary".to_string(), "true".to_string());
    }

    params
}

pub(crate) fn pdf_params_from_config(config: &LexConfig) -> HashMap<String, String> {
    let mut params = HashMap::new();
    match config.convert.pdf.size {
        PdfPageSize::LexEd => {
            params.insert("size-lexed".to_string(), "true".to_string());
        }
        PdfPageSize::Mobile => {
            params.insert("size-mobile".to_string(), "true".to_string());
        }
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;
    use clapfig::Clapfig;

    fn test_config() -> LexConfig {
        Clapfig::schema_builder::<LexConfig>()
            .app_name("lex")
            .no_env()
            .search_paths(vec![])
            .accept_dotted_extension_keys_in(
                lex_config::DIAGNOSTICS_RULES_PATH,
                clapfig::UnknownKeyDecision::Collect,
            )
            .load()
            .expect("defaults to load")
    }

    #[test]
    fn default_config_has_expected_values() {
        let config = test_config();
        assert_eq!(config.formatting.rules.session_blank_lines_before, 1);
        assert!(config.inspect.ast.show_line_numbers);
        assert!(!config.inspect.ast.include_all_properties);
        assert_eq!(config.convert.pdf.size, PdfPageSize::LexEd);
        assert_eq!(config.convert.html.theme, "default");
    }

    #[test]
    fn inspect_params_include_configured_defaults() {
        let config = test_config();
        let params = build_inspect_params(&config);
        assert_eq!(params.get("show-linum"), Some(&"true".to_string()));
        assert!(!params.contains_key("ast-full"));
        assert!(!params.contains_key("color"));
    }

    #[test]
    fn inspect_params_with_all_flags() {
        let mut config = test_config();
        config.inspect.ast.include_all_properties = true;
        config.inspect.nodemap.color_blocks = true;
        config.inspect.nodemap.color_characters = true;
        config.inspect.nodemap.show_summary = true;

        let params = build_inspect_params(&config);
        assert_eq!(params.get("ast-full"), Some(&"true".to_string()));
        assert_eq!(params.get("color"), Some(&"true".to_string()));
        assert_eq!(params.get("color-char"), Some(&"true".to_string()));
        assert_eq!(params.get("nodesummary"), Some(&"true".to_string()));
    }

    #[test]
    fn pdf_params_follow_configured_profile() {
        let mut config = test_config();
        config.convert.pdf.size = PdfPageSize::Mobile;
        let params = pdf_params_from_config(&config);
        assert_eq!(params.get("size-mobile"), Some(&"true".to_string()));
        assert!(!params.contains_key("size-lexed"));
    }

    #[test]
    fn pdf_params_default_lexed() {
        let config = test_config();
        let params = pdf_params_from_config(&config);
        assert_eq!(params.get("size-lexed"), Some(&"true".to_string()));
        assert!(!params.contains_key("size-mobile"));
    }
}
