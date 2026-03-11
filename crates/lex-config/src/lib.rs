//! Shared configuration loader for the Lex toolchain.
//!
//! `defaults/lex.default.toml` is embedded into every binary so that docs and
//! runtime behavior stay in sync. Applications layer user-specific files on top
//! of those defaults via [`Loader`] before deserializing into [`LexConfig`].

use config::builder::DefaultState;
use config::{Config, ConfigBuilder, ConfigError, File, FileFormat, ValueKind};
use lex_babel::formats::lex::formatting_rules::FormattingRules;
use serde::Deserialize;
use std::path::Path;

const DEFAULT_TOML: &str = include_str!("../defaults/lex.default.toml");

/// Top-level configuration consumed by Lex applications.
#[derive(Debug, Clone, Deserialize)]
pub struct LexConfig {
    pub formatting: FormattingConfig,
    pub inspect: InspectConfig,
    pub convert: ConvertConfig,
}

/// Formatting-related configuration groups.
#[derive(Debug, Clone, Deserialize)]
pub struct FormattingConfig {
    pub rules: FormattingRulesConfig,
}

/// Mirrors the knobs exposed by the Lex formatter.
#[derive(Debug, Clone, Deserialize)]
pub struct FormattingRulesConfig {
    pub session_blank_lines_before: usize,
    pub session_blank_lines_after: usize,
    pub normalize_seq_markers: bool,
    pub unordered_seq_marker: char,
    pub max_blank_lines: usize,
    pub indent_string: String,
    pub preserve_trailing_blanks: bool,
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
#[derive(Debug, Clone, Deserialize)]
pub struct InspectConfig {
    pub ast: InspectAstConfig,
    pub nodemap: NodemapConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InspectAstConfig {
    pub include_all_properties: bool,
    pub show_line_numbers: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodemapConfig {
    pub color_blocks: bool,
    pub color_characters: bool,
    pub show_summary: bool,
}

/// Format-specific conversion knobs.
#[derive(Debug, Clone, Deserialize)]
pub struct ConvertConfig {
    pub pdf: PdfConfig,
    pub html: HtmlConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PdfConfig {
    pub size: PdfPageSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum PdfPageSize {
    #[serde(rename = "lexed")]
    LexEd,
    #[serde(rename = "mobile")]
    Mobile,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HtmlConfig {
    pub theme: String,
    /// Optional path to a custom CSS file to append after the baseline CSS.
    #[serde(default)]
    pub custom_css: Option<String>,
}

/// Helper for layering user overrides over the built-in defaults.
#[derive(Debug, Clone)]
pub struct Loader {
    builder: ConfigBuilder<DefaultState>,
}

impl Loader {
    /// Start a loader seeded with the embedded defaults.
    pub fn new() -> Self {
        let builder = Config::builder().add_source(File::from_str(DEFAULT_TOML, FileFormat::Toml));
        Self { builder }
    }

    /// Layer a configuration file. Missing files trigger an error.
    pub fn with_file(mut self, path: impl AsRef<Path>) -> Self {
        let source = File::from(path.as_ref())
            .format(FileFormat::Toml)
            .required(true);
        self.builder = self.builder.add_source(source);
        self
    }

    /// Layer an optional configuration file (ignored if the file is absent).
    pub fn with_optional_file(mut self, path: impl AsRef<Path>) -> Self {
        let source = File::from(path.as_ref())
            .format(FileFormat::Toml)
            .required(false);
        self.builder = self.builder.add_source(source);
        self
    }

    /// Apply a single key/value override (useful for CLI settings).
    pub fn set_override<I>(mut self, key: &str, value: I) -> Result<Self, ConfigError>
    where
        I: Into<ValueKind>,
    {
        self.builder = self.builder.set_override(key, value)?;
        Ok(self)
    }

    /// Finalize the builder and deserialize the resulting configuration.
    pub fn build(self) -> Result<LexConfig, ConfigError> {
        self.builder.build()?.try_deserialize()
    }
}

impl Default for Loader {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience helper for callers that only need the defaults.
pub fn load_defaults() -> Result<LexConfig, ConfigError> {
    Loader::new().build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_default_config() {
        let config = load_defaults().expect("defaults to deserialize");
        assert_eq!(config.formatting.rules.session_blank_lines_before, 1);
        assert!(config.inspect.ast.show_line_numbers);
        assert_eq!(config.convert.pdf.size, PdfPageSize::LexEd);
    }

    #[test]
    fn supports_overrides() {
        let config = Loader::new()
            .set_override("convert.pdf.size", "mobile")
            .expect("override to apply")
            .build()
            .expect("config to build");
        assert_eq!(config.convert.pdf.size, PdfPageSize::Mobile);
    }

    #[test]
    fn formatting_rules_config_converts_to_formatting_rules() {
        let config = load_defaults().expect("defaults to deserialize");
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
