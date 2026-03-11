//! Shared configuration for the Lex toolchain.
//!
//! Defines [`LexConfig`] — the config struct consumed by all Lex applications.
//! Defaults are compiled into the struct via `#[config(default)]`. Loading and
//! layering is handled by [clapfig](https://docs.rs/clapfig) in the CLI.

use confique::Config;
use lex_babel::formats::lex::formatting_rules::FormattingRules;
use serde::{Deserialize, Serialize};

/// Canonical config file name used by the CLI and LSP.
pub const CONFIG_FILE_NAME: &str = ".lex.toml";

/// Top-level configuration consumed by Lex applications.
#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct LexConfig {
    /// Formatting rules.
    #[config(nested)]
    pub formatting: FormattingConfig,
    /// Inspect output options.
    #[config(nested)]
    pub inspect: InspectConfig,
    /// Format-specific conversion options.
    #[config(nested)]
    pub convert: ConvertConfig,
    /// Diagnostics options.
    #[config(nested)]
    pub diagnostics: DiagnosticsConfig,
}

/// Formatting-related configuration groups.
#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct FormattingConfig {
    /// Formatting rules for lex output.
    #[config(nested)]
    pub rules: FormattingRulesConfig,
    /// Automatically format documents on save (consumed by editors).
    #[config(default = false)]
    pub format_on_save: bool,
}

/// Mirrors the knobs exposed by the Lex formatter.
#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct FormattingRulesConfig {
    /// Number of blank lines inserted before a session title.
    #[config(default = 1)]
    pub session_blank_lines_before: usize,
    /// Number of blank lines inserted after a session title.
    #[config(default = 1)]
    pub session_blank_lines_after: usize,
    /// Normalize list markers to predictable markers.
    #[config(default = true)]
    pub normalize_seq_markers: bool,
    /// Character for unordered list items when normalization is enabled.
    #[config(default = "-")]
    pub unordered_seq_marker: char,
    /// Maximum consecutive blank lines kept in output.
    #[config(default = 2)]
    pub max_blank_lines: usize,
    /// Whitespace string for each indentation level.
    #[config(default = "    ")]
    pub indent_string: String,
    /// Preserve trailing blank lines at the end of a document.
    #[config(default = false)]
    pub preserve_trailing_blanks: bool,
    /// Normalize verbatim fences back to canonical :: form.
    #[config(default = true)]
    pub normalize_verbatim_markers: bool,
}

impl From<FormattingRulesConfig> for FormattingRules {
    fn from(config: FormattingRulesConfig) -> Self {
        FormattingRules {
            session_blank_lines_before: config.session_blank_lines_before,
            session_blank_lines_after: config.session_blank_lines_after,
            normalize_seq_markers: config.normalize_seq_markers,
            unordered_seq_marker: config.unordered_seq_marker,
            max_blank_lines: config.max_blank_lines,
            indent_string: config.indent_string,
            preserve_trailing_blanks: config.preserve_trailing_blanks,
            normalize_verbatim_markers: config.normalize_verbatim_markers,
        }
    }
}

impl From<&FormattingRulesConfig> for FormattingRules {
    fn from(config: &FormattingRulesConfig) -> Self {
        FormattingRules {
            session_blank_lines_before: config.session_blank_lines_before,
            session_blank_lines_after: config.session_blank_lines_after,
            normalize_seq_markers: config.normalize_seq_markers,
            unordered_seq_marker: config.unordered_seq_marker,
            max_blank_lines: config.max_blank_lines,
            indent_string: config.indent_string.clone(),
            preserve_trailing_blanks: config.preserve_trailing_blanks,
            normalize_verbatim_markers: config.normalize_verbatim_markers,
        }
    }
}

/// Controls AST-related inspect output.
#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct InspectConfig {
    /// AST visualization options.
    #[config(nested)]
    pub ast: InspectAstConfig,
    /// Nodemap visualization options.
    #[config(nested)]
    pub nodemap: NodemapConfig,
}

#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct InspectAstConfig {
    /// Include annotations, titles, markers, and other metadata in AST visualizations.
    #[config(default = false)]
    pub include_all_properties: bool,
    /// Show line numbers next to AST entries.
    #[config(default = true)]
    pub show_line_numbers: bool,
}

#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct NodemapConfig {
    /// Render ANSI-colored blocks instead of Base2048 glyphs.
    #[config(default = false)]
    pub color_blocks: bool,
    /// Render Base2048 glyphs but color them with ANSI codes.
    #[config(default = false)]
    pub color_characters: bool,
    /// Append high-level summary statistics under the node map output.
    #[config(default = false)]
    pub show_summary: bool,
}

/// Format-specific conversion knobs.
#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct ConvertConfig {
    /// PDF export options.
    #[config(nested)]
    pub pdf: PdfConfig,
    /// HTML export options.
    #[config(nested)]
    pub html: HtmlConfig,
}

#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct PdfConfig {
    /// Page profile used when exporting to PDF ("lexed" or "mobile").
    #[config(default = "lexed")]
    pub size: PdfPageSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PdfPageSize {
    #[serde(rename = "lexed")]
    LexEd,
    #[serde(rename = "mobile")]
    Mobile,
}

#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct HtmlConfig {
    /// Theme for HTML export.
    #[config(default = "default")]
    pub theme: String,
    /// Optional path to a custom CSS file to append after the baseline CSS.
    pub custom_css: Option<String>,
}

/// Diagnostics options.
#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct DiagnosticsConfig {
    /// Enable spellcheck diagnostics.
    #[config(default = true)]
    pub spellcheck: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_defaults() -> LexConfig {
        clapfig::Clapfig::builder::<LexConfig>()
            .app_name("lex")
            .no_env()
            .search_paths(vec![])
            .load()
            .expect("defaults to load")
    }

    #[test]
    fn loads_default_config() {
        let config = load_defaults();
        assert_eq!(config.formatting.rules.session_blank_lines_before, 1);
        assert!(config.inspect.ast.show_line_numbers);
        assert_eq!(config.convert.pdf.size, PdfPageSize::LexEd);
    }

    #[test]
    fn formatting_rules_config_converts_to_formatting_rules() {
        let config = load_defaults();
        let rules: FormattingRules = config.formatting.rules.into();
        assert_eq!(rules.session_blank_lines_before, 1);
        assert_eq!(rules.session_blank_lines_after, 1);
        assert!(rules.normalize_seq_markers);
        assert_eq!(rules.unordered_seq_marker, '-');
        assert_eq!(rules.max_blank_lines, 2);
        assert_eq!(rules.indent_string, "    ");
        assert!(!rules.preserve_trailing_blanks);
        assert!(rules.normalize_verbatim_markers);
    }
}
