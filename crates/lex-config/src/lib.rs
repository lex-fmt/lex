//! Shared configuration for the Lex toolchain.
//!
//! Defines [`LexConfig`] — the config struct consumed by all Lex applications.
//! Defaults are compiled into the struct via `#[clapfig(default)]`. Loading and
//! layering is handled by [clapfig](https://docs.rs/clapfig) in the CLI.

use clapfig::Schema;
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
/// bigorg = { tap = "bigorg", via = "git" }                       # private repo, git clone
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
    /// Transport selector for `github:` / `gitlab:` URL templates.
    /// `"https"` (default) uses the forge's tarball/archive API over
    /// public HTTPS; `"git"` uses a `git clone`, inheriting the user's
    /// git credential setup for private repos (SSH agent, OS keychain,
    /// gh CLI, etc.). Only valid when the spec resolves to a template
    /// scheme (`tap`, or `uri` starting with `github:` / `gitlab:`);
    /// declaring it on a non-template URI is a load-time error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<Via>,
}

/// Transport selector for URL-template namespace declarations
/// (`github:`, `gitlab:`). The default, when unset, is `Https` — the
/// public tarball/archive path — to match the original (pre-`via`)
/// behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    /// Public HTTPS tarball/archive API. No auth.
    Https,
    /// `git clone` of the underlying repo. Inherits the user's
    /// existing git credentials.
    Git,
}

impl Via {
    /// The query-string value the resolver's URI parser sees, e.g.
    /// `"git"` or `"https"`. Lowercased to match
    /// [`crate::NamespaceSpec::canonical_uri`] output.
    pub fn as_query_value(self) -> &'static str {
        match self {
            Via::Https => "https",
            Via::Git => "git",
        }
    }
}

impl NamespaceSpec {
    /// Resolve the spec into a single canonical URI string. Tap
    /// shorthand expands to `github:<tap>/lex-labels`; the table
    /// form's `rev`, `subdir`, and `via` are appended via fragment +
    /// query (`uri#rev?subdir=...&via=git`) so the resolver can parse
    /// them uniformly. `via = "https"` (the default) is intentionally
    /// not encoded — omitting it keeps cache keys stable for the
    /// existing tap-shorthand form (a `.lex.toml` change from
    /// implicit-default to explicit-`https` should not invalidate
    /// caches).
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
                // Only `Git` is encoded — `Https` is the default and
                // omitting it keeps cache keys stable for existing
                // configs that never declared `via`.
                if t.via == Some(Via::Git) {
                    out.push_str(if out.contains('?') { "&" } else { "?" });
                    out.push_str("via=");
                    out.push_str(Via::Git.as_query_value());
                }
                Ok(out)
            }
        }
    }
}

impl NamespaceTable {
    /// Validate mutual-exclusion + non-emptiness, plus that `via` is
    /// only declared on URL-template-shaped specs (`tap`, or `uri`
    /// using `github:` / `gitlab:`). `via` on a `path:` / `https:` /
    /// `git+ssh:` / `git:` URI is meaningless — the transport is
    /// already fully determined — so reject it at load time rather
    /// than letting it silently no-op.
    pub fn validate(&self) -> Result<(), LabelsConfigError> {
        match (&self.tap, &self.uri) {
            (Some(_), Some(_)) => return Err(LabelsConfigError::TapAndUri),
            (None, None) => return Err(LabelsConfigError::EmptyTable),
            _ => {}
        }
        if self.via.is_some() {
            let on_template =
                self.tap.is_some() || self.uri.as_deref().is_some_and(is_template_scheme_uri);
            if !on_template {
                return Err(LabelsConfigError::ViaOnNonTemplateScheme {
                    uri: self.uri.clone().unwrap_or_default(),
                });
            }
        }
        Ok(())
    }
}

/// True when `uri` starts with a URL-template scheme (`github:` or
/// `gitlab:`). Used by [`NamespaceTable::validate`] to gate the `via`
/// knob.
fn is_template_scheme_uri(uri: &str) -> bool {
    let Some((scheme, _)) = uri.split_once(':') else {
        return false;
    };
    matches!(scheme.to_ascii_lowercase().as_str(), "github" | "gitlab")
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
    /// `via` was declared on a spec whose URI scheme is not a URL
    /// template (`github:` / `gitlab:`). The transport is already
    /// fully determined by the scheme, so `via` would silently no-op
    /// — reject it instead.
    ViaOnNonTemplateScheme { uri: String },
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
            LabelsConfigError::ViaOnNonTemplateScheme { uri } => write!(
                f,
                "`via` is only valid on `tap` shorthand or `github:` / `gitlab:` URIs; got `{uri}`"
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
#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
pub struct LexConfig {
    /// Formatting rules.
    pub formatting: FormattingConfig,
    /// Inspect output options.
    pub inspect: InspectConfig,
    /// Format-specific conversion options.
    pub convert: ConvertConfig,
    /// Diagnostics options.
    pub diagnostics: DiagnosticsConfig,
    /// Include-resolution options.
    pub includes: IncludesConfig,
    /// Extension-namespace declarations. The map shape is
    /// free-form (each key is a namespace name; the value is a
    /// sum-typed `NamespaceSpec`), so the field is declared as a
    /// `LeafType::Value` escape hatch via `#[clapfig(value)]` —
    /// clapfig accepts any TOML at this key and serde does the
    /// shape-validation on the deserialize side. The `lexd labels`
    /// subcommand and the boot helper read individual entries via
    /// [`load_labels_from_toml`] for richer error messages
    /// (reserved-namespace check, table-form validation, …).
    #[clapfig(value, optional)]
    #[serde(default)]
    pub labels: BTreeMap<String, NamespaceSpec>,
}

/// Formatting-related configuration groups.
#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
pub struct FormattingConfig {
    /// Formatting rules for lex output.
    pub rules: FormattingRulesConfig,
    /// Automatically format documents on save (consumed by editors).
    #[clapfig(default = false)]
    pub format_on_save: bool,
}

/// Mirrors the knobs exposed by the Lex formatter.
#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
pub struct FormattingRulesConfig {
    /// Number of blank lines inserted before a session title.
    #[clapfig(default = 1)]
    pub session_blank_lines_before: usize,
    /// Number of blank lines inserted after a session title.
    #[clapfig(default = 1)]
    pub session_blank_lines_after: usize,
    /// Normalize list markers to predictable markers.
    #[clapfig(default = true)]
    pub normalize_seq_markers: bool,
    /// Character for unordered list items when normalization is enabled.
    /// `char` isn't in clapfig's native leaf-type vocabulary, so this is
    /// a `LeafType::Value` escape hatch — serde converts the single-char
    /// TOML string on deserialize.
    #[clapfig(value, default = "-")]
    pub unordered_seq_marker: char,
    /// Maximum consecutive blank lines kept in output.
    #[clapfig(default = 2)]
    pub max_blank_lines: usize,
    /// Whitespace string for each indentation level.
    #[clapfig(default = "    ")]
    pub indent_string: String,
    /// Preserve trailing blank lines at the end of a document.
    #[clapfig(default = false)]
    pub preserve_trailing_blanks: bool,
    /// Normalize verbatim fences back to canonical :: form.
    #[clapfig(default = true)]
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
#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
pub struct InspectConfig {
    /// AST visualization options.
    pub ast: InspectAstConfig,
    /// Nodemap visualization options.
    pub nodemap: NodemapConfig,
}

#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
pub struct InspectAstConfig {
    /// Include annotations, titles, markers, and other metadata in AST visualizations.
    #[clapfig(default = false)]
    pub include_all_properties: bool,
    /// Show line numbers next to AST entries.
    #[clapfig(default = true)]
    pub show_line_numbers: bool,
}

#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
pub struct NodemapConfig {
    /// Render ANSI-colored blocks instead of Base2048 glyphs.
    #[clapfig(default = false)]
    pub color_blocks: bool,
    /// Render Base2048 glyphs but color them with ANSI codes.
    #[clapfig(default = false)]
    pub color_characters: bool,
    /// Append high-level summary statistics under the node map output.
    #[clapfig(default = false)]
    pub show_summary: bool,
}

/// Format-specific conversion knobs.
#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
pub struct ConvertConfig {
    /// PDF export options.
    pub pdf: PdfConfig,
    /// HTML export options.
    pub html: HtmlConfig,
}

#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
pub struct PdfConfig {
    /// Page profile used when exporting to PDF ("lexed" or "mobile").
    #[clapfig(default = "lexed")]
    pub size: PdfPageSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum PdfPageSize {
    #[serde(rename = "lexed")]
    LexEd,
    #[serde(rename = "mobile")]
    Mobile,
}

#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
pub struct HtmlConfig {
    /// Theme for HTML export.
    #[clapfig(default = "default")]
    pub theme: String,
    /// Optional path to a custom CSS file to append after the baseline CSS.
    pub custom_css: Option<String>,
}

/// Diagnostics options.
#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
pub struct DiagnosticsConfig {
    /// Per-rule severity overrides. Each entry takes either a bare
    /// severity string (`"allow"`, `"warn"`, `"deny"`) or an array
    /// form (`["warn", { option = value }]`) carrying rule-specific
    /// options — see [`RuleConfig`]. The defaults shown next to each
    /// rule are the intrinsic defaults; uncomment a line to override.
    pub rules: DiagnosticsRulesConfig,
}

/// Per-rule severity for diagnostics.
///
/// One field per built-in diagnostic code. Each field's doc comment is
/// the description that surfaces in `lexd config gen` output, so
/// authoring conventions for these doc comments matter: write them as
/// user-facing prose, lead with what triggers the diagnostic, finish
/// with the intrinsic default.
///
/// `RuleConfig` is a serde-untagged sum (`"warn"` *or*
/// `["warn", { max = 80 }]`), so every field carries `#[clapfig(value)]`
/// — the leaf is a `LeafType::Value` escape hatch in the schema, and
/// serde does the shape validation on deserialize. clapfig's typo
/// detection on the *field names themselves* still applies: a
/// misspelled built-in like `missing_footote` is rejected as an unknown
/// key. Extension-emitted codes (`<namespace>.<code>`) ride in via
/// `on_unknown_key` and a side-channel populated by the loader — they
/// are not fields on this struct.
///
/// `Default` returns every field at [`Severity::Warn`] regardless of
/// each rule's *intrinsic* default. Real production loads run through
/// clapfig and honour the `#[clapfig(default = "...")]` annotations.
/// `Default` is here so tests and ad-hoc embedders can construct an
/// instance without going through the clapfig pipeline.
#[derive(Debug, Clone, Default, Schema, Serialize, Deserialize)]
pub struct DiagnosticsRulesConfig {
    /// A footnote reference like `[42]` has no corresponding
    /// definition in the document. Intrinsic default: deny.
    #[clapfig(value, default = "deny")]
    pub missing_footnote: RuleConfig,
    /// A footnote definition appears in the document but no
    /// reference points at it. Intrinsic default: warn.
    #[clapfig(value, default = "warn")]
    pub unused_footnote: RuleConfig,
    /// A table row has a different number of columns than the
    /// table's header row. Intrinsic default: warn.
    #[clapfig(value, default = "warn")]
    pub table_inconsistent_columns: RuleConfig,
    /// A label uses the reserved `doc.*` prefix, which is no longer
    /// valid under the current label policy. Intrinsic default: deny.
    #[clapfig(value, default = "deny")]
    pub forbidden_label_prefix: RuleConfig,
    /// A `lex.*` literal references a canonical that the toolchain
    /// does not recognise — typically a typo or a label written for
    /// a future core schema. Intrinsic default: deny.
    #[clapfig(value, default = "deny")]
    pub unknown_lex_canonical: RuleConfig,
    /// Spellcheck diagnostics. Intrinsic default: warn. **Currently
    /// not consulted at emission time** — spellcheck emits through a
    /// separate path in `lex-analysis::spellcheck`. The field is
    /// present so the value lives in the user-facing config schema;
    /// runtime wiring lands when spellcheck moves over to the
    /// `AnalysisDiagnostic` / `DiagnosticKind` pipeline.
    #[clapfig(value, default = "warn")]
    pub spellcheck: RuleConfig,
    /// A session reference (`[#2.1]`) points at a session identifier
    /// that exists nowhere in the merged document. Opt-in: emitted only
    /// by `check --references`. Intrinsic default: warn.
    #[clapfig(value, default = "warn")]
    pub missing_session_target: RuleConfig,
    /// A definition reference (`[Title]`) has no matching definition in
    /// the merged document. Opt-in (`check --references`). Intrinsic
    /// default: warn — every `[...]` is a reference, so this can be
    /// chatty in prose-heavy docs and is trivially silenced per-rule.
    #[clapfig(value, default = "warn")]
    pub missing_definition_target: RuleConfig,
    /// An annotation reference (`[::label]`) points at a label that no
    /// annotation declares. Opt-in (`check --references`). Intrinsic
    /// default: warn.
    #[clapfig(value, default = "warn")]
    pub missing_annotation_target: RuleConfig,
    /// A citation (`[@key]`) has no matching annotation label or
    /// definition subject in the merged document. Opt-in
    /// (`check --references`). Intrinsic default: warn.
    #[clapfig(value, default = "warn")]
    pub missing_citation_target: RuleConfig,
    /// Schema-validation diagnostics for extension labels.
    pub schema: SchemaRulesConfig,
}

impl DiagnosticsRulesConfig {
    /// Look up a rule by its on-the-wire code (e.g. `"missing-footnote"`
    /// or `"schema.unknown-label"`).
    ///
    /// Only consults named built-in fields. Extension-emitted codes
    /// (`<namespace>.<code>`) live in a separate side-channel map
    /// populated by the loader's `on_unknown_key` callback and stored
    /// on [`LoadedLexConfig::extension_diagnostic_rules`]; use
    /// [`LoadedLexConfig::lookup_diagnostic_rule`] to consult both
    /// surfaces with a single call.
    pub fn lookup_by_code(&self, code: &str) -> Option<&RuleConfig> {
        match code {
            "missing-footnote" => Some(&self.missing_footnote),
            "unused-footnote" => Some(&self.unused_footnote),
            "table-inconsistent-columns" => Some(&self.table_inconsistent_columns),
            "forbidden-label-prefix" => Some(&self.forbidden_label_prefix),
            "unknown-lex-canonical" => Some(&self.unknown_lex_canonical),
            "missing-session-target" => Some(&self.missing_session_target),
            "missing-definition-target" => Some(&self.missing_definition_target),
            "missing-annotation-target" => Some(&self.missing_annotation_target),
            "missing-citation-target" => Some(&self.missing_citation_target),
            // `spellcheck` is intentionally absent: spellcheck
            // diagnostics emit through a separate path (see
            // `lex-analysis/src/spellcheck.rs`) and do not flow
            // through `DiagnosticKind` / `apply_rules` today.
            // Returning `Some(&self.spellcheck)` here would falsely
            // suggest the rule is wired. Joining the registry is
            // tracked alongside the spellcheck refactor.
            "schema.unknown-label" => Some(&self.schema.unknown_label),
            "schema.missing-param" => Some(&self.schema.missing_param),
            "schema.param-type-mismatch" => Some(&self.schema.param_type_mismatch),
            "schema.bad-attachment" => Some(&self.schema.bad_attachment),
            "schema.body-shape-mismatch" => Some(&self.schema.body_shape_mismatch),
            _ => None,
        }
    }
}

/// Schema-validation diagnostics. Each field maps to one of the
/// schema pre-validation checks the analyser performs before
/// dispatching to an extension handler. See the Extending Lex
/// proposal (`comms/specs/proposals/extending-lex.lex`) §13.2.
#[derive(Debug, Clone, Default, Schema, Serialize, Deserialize)]
pub struct SchemaRulesConfig {
    /// A label is invoked whose namespace is registered, but no
    /// schema entry exists for the label itself. Typically a typo
    /// or an out-of-version label. Intrinsic default: deny.
    #[clapfig(value, default = "deny")]
    pub unknown_label: RuleConfig,
    /// A label invocation omits a parameter the schema marks as
    /// required. Intrinsic default: deny.
    #[clapfig(value, default = "deny")]
    pub missing_param: RuleConfig,
    /// A label parameter's value does not match the type the schema
    /// declares. Intrinsic default: deny.
    #[clapfig(value, default = "deny")]
    pub param_type_mismatch: RuleConfig,
    /// A label attaches to a container shape the schema disallows
    /// (e.g. attaching a paragraph-only label to a session).
    /// Intrinsic default: deny.
    #[clapfig(value, default = "deny")]
    pub bad_attachment: RuleConfig,
    /// A label body's shape (`none` / `text` / `lex`) does not match
    /// the schema's declared body kind. Intrinsic default: deny.
    #[clapfig(value, default = "deny")]
    pub body_shape_mismatch: RuleConfig,
}

/// Include-resolution options consumed by `lexd convert`, `lexd inspect`,
/// and the LSP. `lexd format` never expands includes regardless.
#[derive(Debug, Clone, Schema, Serialize, Deserialize)]
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
    #[clapfig(default = 8)]
    pub max_depth: usize,
    /// Maximum total include count across the document (DoS bound).
    /// Default 1000. Caps fan-out — `max_depth` alone bounds chain
    /// length but a doc with thousands of includes at depth 1 still
    /// blows past it.
    #[clapfig(default = 1000)]
    pub max_total_includes: usize,
    /// Maximum size of any single included file in bytes (DoS bound).
    /// Default 10 MiB (10485760). Files larger than this are rejected
    /// before any bytes hit memory. Used by `FsLoader`; the in-memory
    /// `MemoryLoader` doesn't enforce a size limit.
    #[clapfig(default = 10485760)]
    pub max_file_size: u64,
}

/// Typed configuration plus the side-channel map of extension-emitted
/// diagnostic rules. Returned by [`apply_extension_rules_callback`]-aware
/// loaders.
///
/// The typed [`LexConfig`] holds built-in fields exactly. Extension
/// diagnostic codes (`<namespace>.<code>` entries under
/// `[diagnostics.rules]`) live in `extension_diagnostic_rules` so that
/// dropping the previous `#[serde(flatten)]` catch-all restored typo
/// detection on built-in field names while still accepting the open-
/// ended extension key space.
#[derive(Debug, Clone)]
pub struct LoadedLexConfig {
    pub config: LexConfig,
    /// Side-channel map of extension-emitted diagnostic rules keyed by
    /// their on-the-wire code (`<namespace>.<code>`). Populated from
    /// `[diagnostics.rules]` entries accepted by the loader's
    /// `on_unknown_key` callback.
    pub extension_diagnostic_rules: BTreeMap<String, RuleConfig>,
}

impl LoadedLexConfig {
    /// Look up a `[diagnostics.rules]` entry by its on-the-wire code,
    /// consulting built-in fields first and the extension side-channel
    /// second. Built-ins always win — a stray extension key that
    /// happens to spell a built-in code does not override the typed
    /// surface.
    pub fn lookup_diagnostic_rule(&self, code: &str) -> Option<&RuleConfig> {
        self.config
            .diagnostics
            .rules
            .lookup_by_code(code)
            .or_else(|| self.extension_diagnostic_rules.get(code))
    }
}

/// Dotted-path prefix under which extension-emitted diagnostic codes
/// live (`<namespace>.<code>` style keys, e.g.
/// `"acme.task-due-date-missing" = "warn"`). Used with clapfig's
/// [`accept_dotted_extension_keys_in`](clapfig::SchemaConfigBuilder::accept_dotted_extension_keys_in)
/// helper: any unknown key in this subtree whose remainder contains a
/// `.` is treated as an extension code and routed to the collected-
/// unknowns list; bare typos at this level or typos inside
/// `[diagnostics.rules.schema]` still fail strict-mode validation.
pub const DIAGNOSTICS_RULES_PATH: &str = "diagnostics.rules";

/// Drain clapfig's [`CollectedUnknown`](clapfig::CollectedUnknown) list
/// into a [`BTreeMap<String, RuleConfig>`] suitable for
/// [`LoadedLexConfig::extension_diagnostic_rules`].
///
/// Only paths under [`DIAGNOSTICS_RULES_PATH`] are kept; the dotted
/// remainder becomes the map key (e.g.
/// `diagnostics.rules.acme.task-due-date-missing` →
/// `acme.task-due-date-missing`). Entries whose `value` is `None` or
/// which fail to deserialize as a [`RuleConfig`] are silently dropped
/// — the clapfig accept decision has already been made and the load
/// succeeded; this finalization step is best-effort.
pub fn collect_extension_diagnostic_rules(
    unknowns: Vec<clapfig::CollectedUnknown>,
) -> BTreeMap<String, RuleConfig> {
    let prefix = format!("{DIAGNOSTICS_RULES_PATH}.");
    let mut out = BTreeMap::new();
    for u in unknowns {
        let Some(key) = u.path.strip_prefix(&prefix) else {
            continue;
        };
        let Some(value) = u.value else { continue };
        if let Ok(rule) = value.try_into() {
            out.insert(key.to_string(), rule);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_defaults() -> LexConfig {
        let (config, _unknowns) = clapfig::Clapfig::schema_builder::<LexConfig>()
            .app_name("lex")
            .no_env()
            .search_paths(vec![])
            .accept_dotted_extension_keys_in(
                DIAGNOSTICS_RULES_PATH,
                clapfig::UnknownKeyDecision::Collect,
            )
            .load_with_unknowns()
            .expect("defaults to load");
        config
    }

    #[test]
    fn loads_default_config() {
        let config = load_defaults();
        assert_eq!(config.formatting.rules.session_blank_lines_before, 1);
        assert!(config.inspect.ast.show_line_numbers);
        assert_eq!(config.convert.pdf.size, PdfPageSize::LexEd);
    }

    fn load_from(toml_body: &str) -> LexConfig {
        load_wrapper_from(toml_body).config
    }

    fn load_wrapper_from(toml_body: &str) -> LoadedLexConfig {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, toml_body).unwrap();
        let (config, unknowns) = clapfig::Clapfig::schema_builder::<LexConfig>()
            .app_name("lex")
            .file_name(CONFIG_FILE_NAME)
            .no_env()
            .search_paths(vec![clapfig::SearchPath::Path(dir.path().to_path_buf())])
            .accept_dotted_extension_keys_in(
                DIAGNOSTICS_RULES_PATH,
                clapfig::UnknownKeyDecision::Collect,
            )
            .load_with_unknowns()
            .expect("loads");
        LoadedLexConfig {
            config,
            extension_diagnostic_rules: collect_extension_diagnostic_rules(unknowns),
        }
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
    fn diagnostics_rules_extension_codes_land_in_side_channel() {
        // Extension codes (`<namespace>.<code>`) under [diagnostics.rules]
        // are no longer struct fields — they ride in the
        // `LoadedLexConfig::extension_diagnostic_rules` side-channel
        // populated by the loader's `on_unknown_key` callback. Built-in
        // fields next to them keep their intrinsic typing.
        let loaded = load_wrapper_from(
            r#"
[diagnostics.rules]
missing_footnote = "allow"
"acme.task-due-date-missing" = "deny"
"foolco.bar" = ["warn", { max = 80 }]
"#,
        );
        assert_eq!(
            loaded.config.diagnostics.rules.missing_footnote.severity(),
            Severity::Allow
        );
        let acme = loaded
            .extension_diagnostic_rules
            .get("acme.task-due-date-missing")
            .expect("extension code captured in side-channel");
        assert_eq!(acme.severity(), Severity::Deny);
        let foolco = loaded
            .extension_diagnostic_rules
            .get("foolco.bar")
            .expect("array-form extension code captured");
        assert_eq!(foolco.severity(), Severity::Warn);
        assert_eq!(
            foolco.options().and_then(|o| o.get("max")),
            Some(&toml::Value::Integer(80))
        );
    }

    #[test]
    fn loaded_lookup_diagnostic_rule_consults_both_surfaces() {
        // Drift coverage: built-ins win over a same-named side-channel
        // entry; an extension code only the side-channel knows about
        // resolves through the second path.
        let loaded = LoadedLexConfig {
            config: LexConfig {
                formatting: FormattingConfig {
                    rules: FormattingRulesConfig::default_for_tests(),
                    format_on_save: false,
                },
                inspect: InspectConfig::default_for_tests(),
                convert: ConvertConfig::default_for_tests(),
                diagnostics: DiagnosticsConfig {
                    rules: DiagnosticsRulesConfig {
                        missing_footnote: RuleConfig::Bare(Severity::Deny),
                        ..Default::default()
                    },
                },
                includes: IncludesConfig::default_for_tests(),
                labels: BTreeMap::new(),
            },
            extension_diagnostic_rules: [
                (
                    "missing-footnote".to_string(),
                    RuleConfig::Bare(Severity::Allow),
                ),
                ("acme.foo".to_string(), RuleConfig::Bare(Severity::Allow)),
            ]
            .into_iter()
            .collect(),
        };
        // Built-in wins over the same-named side-channel entry.
        assert_eq!(
            loaded
                .lookup_diagnostic_rule("missing-footnote")
                .map(|r| r.severity()),
            Some(Severity::Deny)
        );
        // Side-channel entries resolve through the second path.
        assert_eq!(
            loaded
                .lookup_diagnostic_rule("acme.foo")
                .map(|r| r.severity()),
            Some(Severity::Allow)
        );
        assert!(loaded.lookup_diagnostic_rule("acme.unknown").is_none());
    }

    fn load_expecting_error(toml_body: &str) -> clapfig::ClapfigError {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, toml_body).unwrap();
        clapfig::Clapfig::schema_builder::<LexConfig>()
            .app_name("lex")
            .file_name(CONFIG_FILE_NAME)
            .no_env()
            .search_paths(vec![clapfig::SearchPath::Path(dir.path().to_path_buf())])
            .accept_dotted_extension_keys_in(
                DIAGNOSTICS_RULES_PATH,
                clapfig::UnknownKeyDecision::Collect,
            )
            .load_with_unknowns()
            .expect_err("typo must surface as an unknown-key error")
    }

    #[test]
    fn diagnostics_rules_typo_in_builtin_errors() {
        // Headline behaviour change vs PR #658: dropping the
        // `#[serde(flatten)] extra` catch-all means typos in built-in
        // field names are rejected by clapfig's strict-mode validator
        // again — they no longer land silently in `extra`.
        let err = load_expecting_error(
            r#"
[diagnostics.rules]
missing_footote = "warn"
"#,
        );
        let keys = err.unknown_keys().expect("UnknownKeys variant");
        assert!(
            keys.iter().any(|k| k.key.ends_with("missing_footote")),
            "the misspelled key is reported: {keys:?}"
        );
    }

    #[test]
    fn diagnostics_rules_typo_inside_nested_section_errors() {
        // `accept_dotted_extension_keys_in` must NOT accept a typo
        // inside a nested-built-in section (`[diagnostics.rules.schema]`)
        // just because the dotted path has more than two segments. The
        // clapfig-side predicate honours the schema's nested-field
        // shape — `schema` is a typed nested object, not an open-ended
        // extension namespace — so an unknown key under it stays a
        // strict-mode violation. This is the failure mode Gemini
        // flagged on the original PR #664 plan.
        let err = load_expecting_error(
            r#"
[diagnostics.rules.schema]
unkown_label = "warn"
"#,
        );
        let keys = err.unknown_keys().expect("UnknownKeys variant");
        assert!(
            keys.iter().any(|k| k.key.ends_with("unkown_label")),
            "the misspelled nested key is reported: {keys:?}"
        );
    }

    // Per-config-struct `default_for_tests` helpers — the new macro
    // doesn't auto-`Default` like confique did, and we want one place
    // each struct can be constructed for unit tests that don't go
    // through the full clapfig pipeline. Production loads always run
    // through clapfig and pick up the `#[clapfig(default = ...)]`
    // annotations.
    impl FormattingRulesConfig {
        fn default_for_tests() -> Self {
            FormattingRulesConfig {
                session_blank_lines_before: 1,
                session_blank_lines_after: 1,
                normalize_seq_markers: true,
                unordered_seq_marker: '-',
                max_blank_lines: 2,
                indent_string: "    ".to_string(),
                preserve_trailing_blanks: false,
                normalize_verbatim_markers: true,
            }
        }
    }

    impl InspectConfig {
        fn default_for_tests() -> Self {
            InspectConfig {
                ast: InspectAstConfig {
                    include_all_properties: false,
                    show_line_numbers: true,
                },
                nodemap: NodemapConfig {
                    color_blocks: false,
                    color_characters: false,
                    show_summary: false,
                },
            }
        }
    }

    impl ConvertConfig {
        fn default_for_tests() -> Self {
            ConvertConfig {
                pdf: PdfConfig {
                    size: PdfPageSize::LexEd,
                },
                html: HtmlConfig {
                    theme: "default".to_string(),
                    custom_css: None,
                },
            }
        }
    }

    impl IncludesConfig {
        fn default_for_tests() -> Self {
            IncludesConfig {
                root: None,
                max_depth: 8,
                max_total_includes: 1000,
                max_file_size: 10_485_760,
            }
        }
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
    fn labels_config_tap_with_via_git_encodes_query() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
bigorg = { tap = "bigorg", via = "git" }
"#,
        )
        .unwrap();
        let labels = load_labels_from_toml(&path).unwrap();
        assert_eq!(
            labels
                .namespaces
                .get("bigorg")
                .unwrap()
                .canonical_uri()
                .unwrap(),
            "github:bigorg/lex-labels?via=git"
        );
    }

    #[test]
    fn labels_config_default_via_https_is_not_encoded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
explicit_https = { tap = "acme", via = "https" }
implicit = { tap = "acme" }
"#,
        )
        .unwrap();
        let labels = load_labels_from_toml(&path).unwrap();
        // Both produce the bare template URI — encoding the implicit
        // default would needlessly diverge cache keys.
        let explicit = labels
            .namespaces
            .get("explicit_https")
            .unwrap()
            .canonical_uri()
            .unwrap();
        let implicit = labels
            .namespaces
            .get("implicit")
            .unwrap()
            .canonical_uri()
            .unwrap();
        assert_eq!(explicit, "github:acme/lex-labels");
        assert_eq!(implicit, "github:acme/lex-labels");
    }

    #[test]
    fn labels_config_via_combines_with_subdir_and_rev() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
foolco = { uri = "gitlab:foolco/lex-labels", rev = "v2.1.0", subdir = "labels/", via = "git" }
"#,
        )
        .unwrap();
        let labels = load_labels_from_toml(&path).unwrap();
        assert_eq!(
            labels
                .namespaces
                .get("foolco")
                .unwrap()
                .canonical_uri()
                .unwrap(),
            "gitlab:foolco/lex-labels#v2.1.0?subdir=labels/&via=git"
        );
    }

    #[test]
    fn labels_config_via_on_https_uri_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
weird = { uri = "https://example.com/labels.tar.gz", via = "git" }
"#,
        )
        .unwrap();
        let err = load_labels_from_toml(&path).unwrap_err();
        assert!(matches!(
            err,
            LabelsConfigError::ViaOnNonTemplateScheme { .. }
        ));
    }

    #[test]
    fn labels_config_via_on_path_uri_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lex.toml");
        std::fs::write(
            &path,
            r#"
[labels]
local = { uri = "path:./labels", via = "git" }
"#,
        )
        .unwrap();
        let err = load_labels_from_toml(&path).unwrap_err();
        assert!(matches!(
            err,
            LabelsConfigError::ViaOnNonTemplateScheme { .. }
        ));
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
