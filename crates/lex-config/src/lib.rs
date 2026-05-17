//! Shared configuration for the Lex toolchain.
//!
//! Defines [`LexConfig`] — the config struct consumed by all Lex applications.
//! Defaults are compiled into the struct via `#[config(default)]`. Loading and
//! layering is handled by [clapfig](https://docs.rs/clapfig) in the CLI.

use confique::Config;
use lex_babel::formats::lex::formatting_rules::FormattingRules;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

mod rule_config;
pub use rule_config::{RuleConfig, RuleOptions, Severity};

/// Canonical config file name used by the CLI and LSP.
pub const CONFIG_FILE_NAME: &str = ".lex.toml";

// ─────────────────────────── Labels (extension namespaces) ───────────────────────────

/// `[labels]` block in `.lex.toml` — declarations of extension
/// namespaces the workspace owner wants the host to load.
///
/// Loaded outside the main `LexConfig` confique chain because the
/// shape is a free-form map keyed by namespace name, not a
/// fixed-field struct. See [`load_labels_from_toml`].
///
/// ```toml
/// [labels]
/// acme = { tap = "acme" }                                       # tap shorthand
/// foolco = "gitlab:foolco/lex-labels#main"                      # bare URI
/// custom = { uri = "github:org/repo", rev = "v1", subdir = "labels/" }
/// ```
///
/// The reserved namespace name `lex` is rejected at load time —
/// `lex.*` is owned by the core and ships compiled-in.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LabelsConfig {
    /// Namespace name → spec. Order is sorted (BTreeMap) for
    /// deterministic loading and stable diagnostics.
    pub namespaces: BTreeMap<String, NamespaceSpec>,
}

/// One namespace declaration. Three on-disk shapes parse into the
/// same logical record:
///
/// - `acme = "github:acme/lex-labels"` — bare URI string.
/// - `acme = { tap = "acme" }` — tap shorthand, expands to
///   `github:acme/lex-labels`.
/// - `acme = { uri = "...", rev = "...", subdir = "..." }` — full
///   table form.
///
/// `tap` and `uri` are mutually exclusive on the table form;
/// having both is a load-time error (see [`NamespaceSpec::validate`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NamespaceSpec {
    /// Bare URI string form.
    Uri(String),
    /// Table form. One of `tap` / `uri` must be set; both is an
    /// error.
    Table(NamespaceTable),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamespaceTable {
    /// Tap-prefix shorthand. `tap = "acme"` expands to
    /// `github:acme/lex-labels`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tap: Option<String>,
    /// Explicit URI (`github:`, `gitlab:`, `https:`, `path:`,
    /// `git+ssh:`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Branch / tag / SHA pin. Mutable refs (branches) honour the
    /// resolver's 24-hour cache TTL; tags and SHAs are cached
    /// indefinitely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// Subdirectory inside the resolved repo containing the schema
    /// files. Defaults to repo root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdir: Option<String>,
}

impl NamespaceSpec {
    /// Resolve the spec into a single canonical URI string. Tap
    /// shorthand expands to `github:<tap>/lex-labels`; the table
    /// form's `rev` and `subdir` are appended via fragment + query
    /// (`uri#rev?subdir=...`) so the resolver can parse them
    /// uniformly.
    pub fn canonical_uri(&self) -> Result<String, LabelsConfigError> {
        match self {
            NamespaceSpec::Uri(s) => Ok(s.clone()),
            NamespaceSpec::Table(t) => {
                t.validate()?;
                let base = match (&t.tap, &t.uri) {
                    (Some(tap), None) => format!("github:{tap}/lex-labels"),
                    (None, Some(uri)) => uri.clone(),
                    (Some(_), Some(_)) => {
                        return Err(LabelsConfigError::TapAndUri);
                    }
                    (None, None) => {
                        return Err(LabelsConfigError::EmptyTable);
                    }
                };
                let mut out = base;
                if let Some(rev) = &t.rev {
                    if out.contains('#') {
                        // Both the URI and the table have a rev. The
                        // tap shorthand can't reach this branch (it
                        // never sets a fragment), so this is the
                        // user-with-explicit-uri case where they wrote
                        // `uri = "github:org/repo#main", rev = "v1"`.
                        // Either is meaningful but together is
                        // ambiguous — surface as an error rather than
                        // silently drop one.
                        return Err(LabelsConfigError::RevWithExplicitFragment {
                            uri: out,
                            rev: rev.clone(),
                        });
                    }
                    out.push('#');
                    out.push_str(rev);
                }
                if let Some(subdir) = &t.subdir {
                    out.push_str(if out.contains('?') { "&" } else { "?" });
                    out.push_str("subdir=");
                    out.push_str(subdir);
                }
                Ok(out)
            }
        }
    }
}

impl NamespaceTable {
    /// Validate mutual-exclusion + non-emptiness. Surfaces as a
    /// load-time error so a bad `lex.toml` fails fast with a clear
    /// message, not at first dispatch.
    pub fn validate(&self) -> Result<(), LabelsConfigError> {
        match (&self.tap, &self.uri) {
            (Some(_), Some(_)) => Err(LabelsConfigError::TapAndUri),
            (None, None) => Err(LabelsConfigError::EmptyTable),
            _ => Ok(()),
        }
    }
}

/// Errors emitted by [`load_labels_from_toml`] and
/// [`NamespaceSpec::canonical_uri`].
#[derive(Debug)]
#[non_exhaustive]
pub enum LabelsConfigError {
    /// Reading the toml file failed.
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    /// The toml body did not parse.
    Parse {
        path: std::path::PathBuf,
        message: String,
    },
    /// `[labels]` declared the reserved `lex` namespace. The `lex.*`
    /// label space is owned by the core and ships compiled-in;
    /// re-declaring it would silently shadow core built-ins.
    ReservedNamespace,
    /// Table form had both `tap` and `uri` set. They're mutually
    /// exclusive — pick one.
    TapAndUri,
    /// Table form had neither `tap` nor `uri` set.
    EmptyTable,
    /// Both the explicit `uri` (with a `#fragment`) and a `rev`
    /// field are set. Either is meaningful but together they're
    /// ambiguous — pick one.
    RevWithExplicitFragment { uri: String, rev: String },
}

impl std::fmt::Display for LabelsConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LabelsConfigError::Io { path, source } => {
                write!(f, "{}: io error reading labels config: {source}", path.display())
            }
            LabelsConfigError::Parse { path, message } => {
                write!(f, "{}: labels config parse error: {message}", path.display())
            }
            LabelsConfigError::ReservedNamespace => f.write_str(
                "namespace `lex` is reserved for core-defined labels and cannot be declared in [labels]",
            ),
            LabelsConfigError::TapAndUri => {
                f.write_str("namespace spec sets both `tap` and `uri`; they are mutually exclusive")
            }
            LabelsConfigError::EmptyTable => f.write_str(
                "namespace spec table needs one of `tap` or `uri` set",
            ),
            LabelsConfigError::RevWithExplicitFragment { uri, rev } => write!(
                f,
                "namespace spec sets both `rev = {rev:?}` and an explicit `#fragment` in uri `{uri}`; pick one"
            ),
        }
    }
}

impl std::error::Error for LabelsConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LabelsConfigError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Load the `[labels]` block from a `.lex.toml` at `path`. Returns
/// an empty config if the file exists but has no `[labels]` block;
/// `Io::NotFound` is propagated to the caller (the CLI usually
/// treats it as "no labels configured" and continues).
///
/// Validates the reserved-key rule (`lex` is forbidden) and each
/// spec's table-form invariants. Bad config fails the load instead
/// of letting it surface at dispatch time.
pub fn load_labels_from_toml(path: impl AsRef<Path>) -> Result<LabelsConfig, LabelsConfigError> {
    let path = path.as_ref();
    let body = std::fs::read_to_string(path).map_err(|source| LabelsConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    // We only read the `[labels]` table; the rest of the file is
    // confique's territory. A `toml::Value` parse + manual lookup
    // keeps us from reaching for a separate top-level struct.
    let root: toml::Value =
        body.parse()
            .map_err(|err: toml::de::Error| LabelsConfigError::Parse {
                path: path.to_path_buf(),
                message: err.to_string(),
            })?;
    let Some(labels_value) = root.get("labels") else {
        return Ok(LabelsConfig::default());
    };
    let mut config: LabelsConfig =
        labels_value
            .clone()
            .try_into()
            .map_err(|err: toml::de::Error| LabelsConfigError::Parse {
                path: path.to_path_buf(),
                message: err.to_string(),
            })?;

    if config.namespaces.contains_key("lex") {
        return Err(LabelsConfigError::ReservedNamespace);
    }
    for spec in config.namespaces.values_mut() {
        if let NamespaceSpec::Table(t) = spec {
            t.validate()?;
        }
    }
    Ok(config)
}

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
    /// Include-resolution options.
    #[config(nested)]
    pub includes: IncludesConfig,
    /// Extension-namespace declarations. The map shape is
    /// free-form (each key is a namespace name; the value is a
    /// `NamespaceSpec`), so the field is a leaf rather than a
    /// nested confique struct — confique sees an opaque
    /// `BTreeMap<String, NamespaceSpec>`. The `lexd labels`
    /// subcommand and the boot helper read individual entries via
    /// [`load_labels_from_toml`] for richer error messages
    /// (reserved-namespace check, table-form validation, …).
    /// Declaring the field here is what makes clapfig's strict
    /// unknown-keys check accept `[labels]` blocks in `.lex.toml`.
    #[config(default = {})]
    pub labels: BTreeMap<String, NamespaceSpec>,
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
    /// Per-rule severity overrides. Each entry maps a diagnostic code
    /// to a severity ("allow", "warn", or "deny"). The defaults shown
    /// next to each rule are the intrinsic defaults — uncomment a line
    /// to override one.
    #[config(nested)]
    pub rules: DiagnosticsRulesConfig,
}

/// Per-rule severity for diagnostics.
///
/// One field per built-in diagnostic code. Each field's doc comment is
/// the description that surfaces in `lexd config gen` output, so
/// authoring conventions for these doc comments matter: write them as
/// user-facing prose, lead with what triggers the diagnostic, finish
/// with the intrinsic default. Extension-emitted codes
/// (`<namespace>.<code>`) and forward-looking prefix globs are not
/// fields on this struct — they ride in the embedded map of `extra`
/// once that surface lands.
#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct DiagnosticsRulesConfig {
    /// A footnote reference like `[42]` has no corresponding
    /// definition in the document. Intrinsic default: deny.
    #[config(default = "deny")]
    pub missing_footnote: RuleConfig,
    /// A footnote definition appears in the document but no
    /// reference points at it. Intrinsic default: warn.
    #[config(default = "warn")]
    pub unused_footnote: RuleConfig,
    /// A table row has a different number of columns than the
    /// table's header row. Intrinsic default: warn.
    #[config(default = "warn")]
    pub table_inconsistent_columns: RuleConfig,
    /// A label uses the reserved `doc.*` prefix, which is no longer
    /// valid under the current label policy. Intrinsic default: deny.
    #[config(default = "deny")]
    pub forbidden_label_prefix: RuleConfig,
    /// A `lex.*` literal references a canonical that the toolchain
    /// does not recognise — typically a typo or a label written for
    /// a future core schema. Intrinsic default: deny.
    #[config(default = "deny")]
    pub unknown_lex_canonical: RuleConfig,
    /// Spellcheck diagnostics. Set to "allow" to suppress
    /// document-wide. Intrinsic default: warn.
    #[config(default = "warn")]
    pub spellcheck: RuleConfig,
    /// Schema-validation diagnostics for extension labels.
    #[config(nested)]
    pub schema: SchemaRulesConfig,
}

/// Schema-validation diagnostics. Each field maps to one of the six
/// schema pre-validation checks the analyser performs before
/// dispatching to an extension handler. See
/// [`extending-lex.lex`](../specs/proposals/extending-lex.lex) §13.2.
#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct SchemaRulesConfig {
    /// A label is invoked whose namespace is registered, but no
    /// schema entry exists for the label itself. Typically a typo
    /// or an out-of-version label. Intrinsic default: deny.
    #[config(default = "deny")]
    pub unknown_label: RuleConfig,
    /// A label invocation omits a parameter the schema marks as
    /// required. Intrinsic default: deny.
    #[config(default = "deny")]
    pub missing_param: RuleConfig,
    /// A label parameter's value does not match the type the schema
    /// declares. Intrinsic default: deny.
    #[config(default = "deny")]
    pub param_type_mismatch: RuleConfig,
    /// A label attaches to a container shape the schema disallows
    /// (e.g. attaching a paragraph-only label to a session).
    /// Intrinsic default: deny.
    #[config(default = "deny")]
    pub bad_attachment: RuleConfig,
    /// A label body's shape (`none` / `text` / `lex`) does not match
    /// the schema's declared body kind. Intrinsic default: deny.
    #[config(default = "deny")]
    pub body_shape_mismatch: RuleConfig,
}

/// Include-resolution options consumed by `lexd convert`, `lexd inspect`,
/// and the LSP. `lexd format` never expands includes regardless.
#[derive(Debug, Clone, Config, Serialize, Deserialize)]
pub struct IncludesConfig {
    /// Resolution root for include path resolution.
    ///
    /// All include paths — relative or root-absolute (`/...`) — must
    /// lexically normalize inside this directory. Outside-the-root paths
    /// fail with a `RootEscape` error. (The resolver does not call
    /// `std::fs::canonicalize`; symlink-aware canonicalization is the
    /// loader's responsibility, not the resolver's.)
    ///
    /// When `None` (default), the CLI discovers the root by walking up
    /// from the entry-point document to find the nearest `.lex.toml`,
    /// falling back to the entry-point's own directory.
    pub root: Option<String>,
    /// Maximum include depth. Default 8. Hitting the limit is an error,
    /// not a silent truncation.
    #[config(default = 8)]
    pub max_depth: usize,
    /// Maximum total include count across the document (DoS bound).
    /// Default 1000. Caps fan-out — `max_depth` alone bounds chain
    /// length but a doc with thousands of includes at depth 1 still
    /// blows past it.
    #[config(default = 1000)]
    pub max_total_includes: usize,
    /// Maximum size of any single included file in bytes (DoS bound).
    /// Default 10 MiB (10485760). Files larger than this are rejected
    /// before any bytes hit memory. Used by `FsLoader`; the in-memory
    /// `MemoryLoader` doesn't enforce a size limit.
    #[config(default = 10485760)]
    pub max_file_size: u64,
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

    fn load_from(toml_body: &str) -> LexConfig {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, toml_body).unwrap();
        clapfig::Clapfig::builder::<LexConfig>()
            .app_name("lex")
            .file_name(CONFIG_FILE_NAME)
            .no_env()
            .search_paths(vec![clapfig::SearchPath::Path(dir.path().to_path_buf())])
            .load()
            .expect("loads")
    }

    #[test]
    fn diagnostics_rules_defaults_in_place() {
        let cfg = load_defaults();
        assert_eq!(
            cfg.diagnostics.rules.missing_footnote.severity(),
            Severity::Deny
        );
        assert_eq!(
            cfg.diagnostics.rules.unused_footnote.severity(),
            Severity::Warn
        );
        assert_eq!(
            cfg.diagnostics.rules.table_inconsistent_columns.severity(),
            Severity::Warn
        );
        assert_eq!(
            cfg.diagnostics.rules.forbidden_label_prefix.severity(),
            Severity::Deny
        );
        assert_eq!(
            cfg.diagnostics.rules.unknown_lex_canonical.severity(),
            Severity::Deny
        );
        assert_eq!(cfg.diagnostics.rules.spellcheck.severity(), Severity::Warn);
        assert_eq!(
            cfg.diagnostics.rules.schema.unknown_label.severity(),
            Severity::Deny
        );
    }

    #[test]
    fn diagnostics_rules_user_overrides_apply() {
        let cfg = load_from(
            r#"
[diagnostics.rules]
missing_footnote = "allow"
table_inconsistent_columns = "deny"

[diagnostics.rules.schema]
unknown_label = "warn"
"#,
        );
        assert_eq!(
            cfg.diagnostics.rules.missing_footnote.severity(),
            Severity::Allow
        );
        assert_eq!(
            cfg.diagnostics.rules.table_inconsistent_columns.severity(),
            Severity::Deny
        );
        assert_eq!(
            cfg.diagnostics.rules.schema.unknown_label.severity(),
            Severity::Warn
        );
        // Untouched rules retain their intrinsic defaults.
        assert_eq!(
            cfg.diagnostics.rules.forbidden_label_prefix.severity(),
            Severity::Deny
        );
    }

    #[test]
    fn diagnostics_rules_accept_array_form() {
        let cfg = load_from(
            r#"
[diagnostics.rules]
missing_footnote = ["warn", { example_option = 42 }]
"#,
        );
        let rule = &cfg.diagnostics.rules.missing_footnote;
        assert_eq!(rule.severity(), Severity::Warn);
        let opts = rule.options().expect("array form keeps options");
        assert_eq!(opts.get("example_option"), Some(&toml::Value::Integer(42)));
    }

    #[test]
    fn labels_config_bare_uri_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
foolco = "gitlab:foolco/lex-labels#main"
"#,
        )
        .unwrap();
        let labels = load_labels_from_toml(&path).expect("loads");
        let spec = labels.namespaces.get("foolco").unwrap();
        assert_eq!(
            spec.canonical_uri().unwrap(),
            "gitlab:foolco/lex-labels#main"
        );
    }

    #[test]
    fn labels_config_tap_shorthand_expands() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
acme = { tap = "acme" }
"#,
        )
        .unwrap();
        let labels = load_labels_from_toml(&path).unwrap();
        assert_eq!(
            labels
                .namespaces
                .get("acme")
                .unwrap()
                .canonical_uri()
                .unwrap(),
            "github:acme/lex-labels"
        );
    }

    #[test]
    fn labels_config_expanded_table_with_rev_and_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
custom = { uri = "github:org/repo", rev = "v1", subdir = "labels/" }
"#,
        )
        .unwrap();
        let labels = load_labels_from_toml(&path).unwrap();
        let uri = labels
            .namespaces
            .get("custom")
            .unwrap()
            .canonical_uri()
            .unwrap();
        assert!(uri.starts_with("github:org/repo"));
        assert!(uri.contains("v1"));
        assert!(uri.contains("subdir=labels/"));
    }

    #[test]
    fn labels_config_reserved_lex_namespace_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
lex = "github:fake/lex-labels"
"#,
        )
        .unwrap();
        let err = load_labels_from_toml(&path).unwrap_err();
        assert!(matches!(err, LabelsConfigError::ReservedNamespace));
    }

    #[test]
    fn labels_config_tap_and_uri_together_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
acme = { tap = "acme", uri = "github:other/repo" }
"#,
        )
        .unwrap();
        let err = load_labels_from_toml(&path).unwrap_err();
        assert!(matches!(err, LabelsConfigError::TapAndUri));
    }

    #[test]
    fn labels_config_empty_table_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
acme = { rev = "v1" }
"#,
        )
        .unwrap();
        let err = load_labels_from_toml(&path).unwrap_err();
        assert!(matches!(err, LabelsConfigError::EmptyTable));
    }

    #[test]
    fn labels_config_missing_block_yields_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(&path, "# no labels block\n").unwrap();
        let labels = load_labels_from_toml(&path).unwrap();
        assert!(labels.namespaces.is_empty());
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
