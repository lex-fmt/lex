//! Document diagnostics: the always-on analyser and the opt-in
//! reference/file-path passes.
//!
//! This entry module owns the core diagnostic vocabulary
//! ([`DiagnosticKind`], [`DiagnosticSeverity`], [`SchemaValidationKind`],
//! [`AnalysisDiagnostic`]), the rule-application step ([`apply_rules`]), and
//! the orchestration entry points ([`analyze`], [`analyze_with_registry`],
//! [`analyze_with_rules`]). Each diagnostic *category* lives in its own
//! submodule, called into from [`analyze_with_registry`]:
//!
//! - `annotations` — unclosed-annotation warning (lex#700).
//! - `footnotes` — missing-footnote-definition checks, plus the
//!   footnote-scoped document traversal those checks depend on.
//! - `labels` — forbidden `doc.*` / unknown `lex.*` label policy.
//! - `tables` — table column-consistency checks.
//! - `references` — the opt-in `check --references` passes
//!   (cross-reference resolution, URL well-formedness, file-path collection).
//!
//! The public items from `references` are re-exported here, so their
//! `lex_analysis::diagnostics::` paths are unchanged.

mod annotations;
mod footnotes;
mod labels;
mod references;
mod tables;

#[cfg(test)]
mod tests;

pub use references::{analyze_references, collect_file_references, FileReference};

use lex_config::{DiagnosticsRulesConfig, RuleConfig, Severity};
use lex_core::lex::ast::{Document, Range};
use lex_extension_host::Registry;
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    MissingFootnoteDefinition,
    UnusedFootnoteDefinition,
    TableInconsistentColumns,
    /// A label invocation failed schema pre-validation before the
    /// handler was dispatched. The variant carries which of the
    /// pre-validation checks tripped.
    SchemaValidation(SchemaValidationKind),
    /// A diagnostic emitted by a registered extension handler. The
    /// `namespace` field is the namespace name (the part before the
    /// first `.`, e.g., `"acme"` for label `"acme.task"`) — `lex-lsp`
    /// surfaces it as the diagnostic `source: "lex:<namespace>"` so
    /// editors can filter by extension.
    ///
    /// `code` carries the **bare leaf** the handler supplied (the
    /// `code` field on `lex_extension::Diagnostic`), *not* the wire
    /// form. The analyser glues on the namespace prefix in
    /// [`DiagnosticKind::code`] to produce the wire shape per spec §9
    /// (`<namespace>.<leaf>`, e.g. `"acme.foo"`; or the per-namespace
    /// fallback `"acme.diagnostic"` when the handler set `None`).
    /// Passing an already-prefixed value here would produce a
    /// double-prefixed wire code (`"acme.acme.foo"`) — handlers should
    /// supply just the leaf.
    Handler {
        namespace: String,
        code: Option<String>,
    },
    /// A label uses the reserved `doc.*` prefix (forbidden under
    /// `comms/specs/general.lex` §4.1). PR 4 of #584 emits this when
    /// permissive-mode parse lets the label flow through; the LSP
    /// then offers a quickfix to rewrite to the blessed shortcut
    /// (`doc.table` → `table`, `doc.image` → `image`, etc.).
    ForbiddenLabelPrefix,
    /// A `lex.*` literal that doesn't match any registered canonical
    /// in [`lex_core::lex::builtins::CANONICAL_LABELS`]. Typically a
    /// typo (`lex.fooar`) or a label authored against a future
    /// version of the core schemas.
    UnknownLexCanonical,
    /// A paragraph line that looks like an annotation header (`:: label`)
    /// but has no closing `::`. There is no "open form" — such a line is
    /// kept as paragraph text rather than dropped (lex#700) — so this
    /// warns the author that what looks like metadata is being treated as
    /// content. The fix is to close the marker: `:: label ::`.
    UnclosedAnnotation,
    /// A session reference (`[#2.1]`) whose identifier matches no session
    /// in the merged document. Emitted only by the opt-in
    /// [`analyze_references`] pass (`check --references`), never by the
    /// always-on analyser.
    MissingSessionTarget,
    /// A definition reference (`[Title]`) whose subject matches no
    /// definition in the merged document. Opt-in (`check --references`).
    MissingDefinitionTarget,
    /// An annotation reference (`[::label]`) whose label matches no
    /// annotation in the merged document. Opt-in (`check --references`).
    MissingAnnotationTarget,
    /// A citation reference (`[@key]`) whose key matches no annotation
    /// label or definition subject in the merged document. Opt-in
    /// (`check --references`).
    MissingCitationTarget,
    /// A URL reference (`[http://…]`, `[https://…]`, `[mailto:…]`) that
    /// is not well-formed (embedded space, empty host, otherwise
    /// unparseable).
    /// Opt-in (`check --references`); a pure parse check — network
    /// reachability is out of scope. Emitted by [`analyze_references`].
    MalformedUrl,
    /// A file-path reference — an inline `ReferenceType::File`
    /// (`[./x.txt]`, `[../y]`, `[/abs]`) or a verbatim block's `src=`
    /// parameter — that points at no file on disk, or whose target escapes
    /// the resolution root / is a platform-absolute path. Opt-in:
    /// emitted only by `check --references` (the existence check is
    /// IO-bearing, so it runs in the CLI seam, not the pure analyser).
    /// `lex.include src=` is excluded — its path is validated by the
    /// base command via include expansion.
    MissingFileTarget,
}

/// Severity for analysis-emitted diagnostics. The analyser populates
/// it for every diagnostic — `lex-lsp` reads `diag.severity`
/// directly when mapping onto the LSP wire. (Earlier the LSP layer
/// derived severity from `DiagnosticKind`; that mapping moved
/// upstream once the extension-emitted diagnostics needed
/// per-instance severities.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// One of the schema pre-validation checks the analyser owns before
/// dispatching to a handler. Wire spec / proposal §13.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaValidationKind {
    /// The namespace is registered but the schema set for that
    /// namespace doesn't declare this exact label. The walker emits
    /// this when `Registry::schema_for(label)` returns `None` while
    /// `is_namespace_healthy(<ns prefix>)` is `true`. Distinguishes
    /// "typo / out-of-version label" (this variant, surfaced as a
    /// document error) from "unknown namespace" (silent pass-through
    /// per the bounded-extensibility rule).
    UnknownLabel,
    MissingParam,
    ParamTypeMismatch,
    BadAttachment,
    BodyShapeMismatch,
}

impl SchemaValidationKind {
    /// The on-the-wire code for this schema-validation kind. Matches
    /// the `[diagnostics.rules.schema]` field name in `.lex.toml`.
    pub fn code(&self) -> &'static str {
        match self {
            SchemaValidationKind::UnknownLabel => "schema.unknown-label",
            SchemaValidationKind::MissingParam => "schema.missing-param",
            SchemaValidationKind::ParamTypeMismatch => "schema.param-type-mismatch",
            SchemaValidationKind::BadAttachment => "schema.bad-attachment",
            SchemaValidationKind::BodyShapeMismatch => "schema.body-shape-mismatch",
        }
    }
}

impl DiagnosticKind {
    /// The on-the-wire code for this diagnostic kind. The same value
    /// travels in `lsp_types::Diagnostic.code` and is the key the
    /// `[diagnostics.rules]` block in `.lex.toml` matches against
    /// (see [`DiagnosticsRulesConfig::lookup_by_code`]).
    ///
    /// For the `Handler` variant — extension-emitted diagnostics —
    /// this returns the namespace-prefixed code: `"acme.foo"` for
    /// `Handler { namespace: "acme", code: Some("foo") }`, or
    /// `"acme.diagnostic"` when the handler omitted a code. The
    /// namespace prefix is what `[diagnostics.rules]` keys match
    /// against (spec §9), and the per-namespace `.diagnostic` fallback
    /// gives users one knob per namespace for code-less handler
    /// diagnostics rather than a single global `"handler.diagnostic"`.
    ///
    /// Returns `Cow<'static, str>` so built-in variants borrow a
    /// static string (no allocation) while the `Handler` variant owns
    /// the `format!`-produced result. `apply_rules` runs on every
    /// document change in the LSP, so avoiding per-built-in allocations
    /// matters.
    pub fn code(&self) -> Cow<'static, str> {
        match self {
            DiagnosticKind::MissingFootnoteDefinition => "missing-footnote".into(),
            DiagnosticKind::UnusedFootnoteDefinition => "unused-footnote".into(),
            DiagnosticKind::TableInconsistentColumns => "table-inconsistent-columns".into(),
            DiagnosticKind::SchemaValidation(kind) => kind.code().into(),
            DiagnosticKind::Handler { namespace, code } => match code {
                Some(c) => format!("{namespace}.{c}").into(),
                None => format!("{namespace}.diagnostic").into(),
            },
            DiagnosticKind::ForbiddenLabelPrefix => "forbidden-label-prefix".into(),
            DiagnosticKind::UnknownLexCanonical => "unknown-lex-canonical".into(),
            DiagnosticKind::UnclosedAnnotation => "unclosed-annotation".into(),
            DiagnosticKind::MissingSessionTarget => "missing-session-target".into(),
            DiagnosticKind::MissingDefinitionTarget => "missing-definition-target".into(),
            DiagnosticKind::MissingAnnotationTarget => "missing-annotation-target".into(),
            DiagnosticKind::MissingCitationTarget => "missing-citation-target".into(),
            DiagnosticKind::MalformedUrl => "malformed-url".into(),
            DiagnosticKind::MissingFileTarget => "missing-file-target".into(),
        }
    }
}

/// Apply a `[diagnostics.rules]` configuration to a stream of analyser
/// diagnostics in place. Drops diagnostics whose resolved severity is
/// `allow`, and remaps the remaining diagnostics' `severity` field:
///
/// - `warn` → the diagnostic's intrinsic severity stays unchanged.
/// - `deny` → severity is upgraded to `Error`.
///
/// `lookup_rule` is the resolution function — typically
/// [`LoadedLexConfig::lookup_diagnostic_rule`](lex_config::LoadedLexConfig::lookup_diagnostic_rule),
/// which consults the named built-in fields first and the
/// extension-rules side-channel second. Diagnostics whose code has no
/// matching entry on either surface pass through untouched at their
/// intrinsic severity.
pub fn apply_rules<F>(diagnostics: &mut Vec<AnalysisDiagnostic>, lookup_rule: F)
where
    F: Fn(&str) -> Option<RuleConfig>,
{
    diagnostics.retain_mut(|diag| {
        let code = diag.kind.code();
        let Some(rule) = lookup_rule(&code) else {
            return true;
        };
        match rule.severity() {
            Severity::Allow => false,
            Severity::Warn => true,
            Severity::Deny => {
                diag.severity = DiagnosticSeverity::Error;
                true
            }
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisDiagnostic {
    pub range: Range,
    /// Severity, set by the analyser for every diagnostic it
    /// produces. `lex-lsp` reads this directly when mapping onto LSP
    /// wire severities; the kind-to-severity mapping that lived in
    /// `to_lsp_diagnostic` is no longer authoritative.
    pub severity: DiagnosticSeverity,
    pub kind: DiagnosticKind,
    pub message: String,
}

/// Run the analyser without an extension registry — equivalent to
/// running with an empty registry. Provided for callers that haven't
/// adopted the extension system yet.
pub fn analyze(document: &Document) -> Vec<AnalysisDiagnostic> {
    let registry = Registry::new();
    analyze_with_registry(document, &registry)
}

/// Run the analyser with a populated extension registry. Labels whose
/// namespace is registered get pre-validated against their schema and,
/// if pre-validation passes, dispatched to the handler's `on_validate`
/// hook. Handler-emitted diagnostics are merged into the same stream as
/// the built-in checks.
pub fn analyze_with_registry(document: &Document, registry: &Registry) -> Vec<AnalysisDiagnostic> {
    let mut diagnostics = Vec::new();
    footnotes::check_footnotes(document, &mut diagnostics);
    tables::check_tables(document, &mut diagnostics);
    labels::check_labels(document, &mut diagnostics);
    annotations::check_unclosed_annotations(document, &mut diagnostics);
    crate::label_dispatch::dispatch_labels(document, registry, &mut diagnostics);
    diagnostics
}

/// Run the analyser with both an extension registry and a
/// `[diagnostics.rules]` configuration. The configuration is applied
/// after all checks run, so rule overrides ([`Severity::Allow`] /
/// [`Severity::Deny`]) take effect uniformly across the diagnostic
/// stream.
pub fn analyze_with_rules(
    document: &Document,
    registry: &Registry,
    rules: &DiagnosticsRulesConfig,
) -> Vec<AnalysisDiagnostic> {
    let mut diagnostics = analyze_with_registry(document, registry);
    apply_rules(&mut diagnostics, |code| rules.lookup_by_code(code).cloned());
    diagnostics
}
