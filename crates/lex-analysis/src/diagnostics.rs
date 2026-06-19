use crate::inline::extract_references;
use lex_config::{DiagnosticsRulesConfig, RuleConfig, Severity};
use lex_core::lex::ast::{
    Annotation, ContentItem, Document, Range, Session, Table, TableRow, TextContent,
};
use lex_core::lex::inlines::ReferenceType;
use lex_extension_host::Registry;
use std::borrow::Cow;
use std::collections::HashSet;

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
    /// (`[./x.txt]`, `[../y]`, `[/abs]`) or an image/data verbatim
    /// `src=` — that points at no file on disk, or whose target escapes
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
    check_footnotes(document, &mut diagnostics);
    check_tables(document, &mut diagnostics);
    check_labels(document, &mut diagnostics);
    check_unclosed_annotations(document, &mut diagnostics);
    crate::label_dispatch::dispatch_labels(document, registry, &mut diagnostics);
    diagnostics
}

/// Warn on paragraph lines that look like an annotation header but never close
/// the `:: ::` marker (lex#700). There is no "open form": `:: label` with no
/// closing `::` is not a recognized element, so the parser keeps it as paragraph
/// text rather than dropping it. This surfaces that — the author likely meant an
/// annotation and forgot the trailing `::`.
fn check_unclosed_annotations(document: &Document, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    fn emit(
        tl: &lex_core::lex::ast::elements::paragraph::TextLine,
        out: &mut Vec<AnalysisDiagnostic>,
    ) {
        if looks_like_unclosed_annotation(tl.text()) {
            out.push(AnalysisDiagnostic {
                range: tl.location.clone(),
                severity: DiagnosticSeverity::Warning,
                kind: DiagnosticKind::UnclosedAnnotation,
                message: "this line looks like an annotation but has no closing `::`, \
                          so it is treated as text. Close the marker to make it an \
                          annotation, e.g. `:: label ::`."
                    .to_string(),
            });
        }
    }

    fn walk(item: &ContentItem, out: &mut Vec<AnalysisDiagnostic>) {
        if let ContentItem::Paragraph(p) = item {
            for line in &p.lines {
                if let ContentItem::TextLine(tl) = line {
                    emit(tl, out);
                }
            }
        }
        if let Some(children) = item.children() {
            for child in children {
                walk(child, out);
            }
        }
    }

    for child in &document.root.children {
        walk(child, diagnostics);
    }
}

/// True when a line is shaped like an annotation header (`:: label …`) but has no
/// closing `::`. Detection is intentionally a lightweight text heuristic — by the
/// time content reaches the analyser, a *closed* annotation is already its own
/// node, so any `::`-leading paragraph line is the unclosed shape.
fn looks_like_unclosed_annotation(text: &str) -> bool {
    let Some(rest) = text.trim().strip_prefix("::") else {
        return false;
    };
    // A second *structural* `::` means a closed marker — not the unclosed shape.
    // Scan quote-aware so a `::` inside a quoted parameter value (e.g.
    // `:: note foo=":: value"`) does not count as a close, matching how the
    // lexer's structural-marker detection treats it.
    let mut in_quotes = false;
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes && chars.peek() == Some(&':') => return false,
            _ => {}
        }
    }
    // Require whitespace after the opening marker, then a label-shaped token
    // (label.lex: a letter, then letters/digits/`_`/`-`/`.`).
    let label = rest.trim_start();
    rest.len() != label.len() && label.chars().next().is_some_and(|c| c.is_alphabetic())
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

/// Opt-in pass: validate internal cross-references over the (merged)
/// document and emit a `missing-*-target` diagnostic for each dangling
/// in-document reference.
///
/// **Deliberately separate from [`analyze_with_registry`]** so the
/// always-on analyser (and thus the LSP, which calls
/// [`analyze_with_rules`] on every keystroke) does *not* emit these.
/// `check --references` calls this explicitly; the LSP can opt in later.
///
/// Resolution runs over the single merged tree, so it is bidirectional:
/// a reference resolves against targets defined anywhere in the document
/// — any included fragment or the master — and a `missing-*` fires only
/// when the target is absent from the *whole* tree. Each finding's range
/// carries the reference's origin (via [`extract_references`]), so the
/// caller blames it on the file the reference was authored in.
///
/// Checked kinds and their codes:
///
/// - [`ReferenceType::Session`] → `missing-session-target`
/// - [`ReferenceType::General`] → `missing-definition-target`
/// - [`ReferenceType::AnnotationReference`] → `missing-annotation-target`
/// - [`ReferenceType::Citation`] → `missing-citation-target`
/// - [`ReferenceType::Url`] → `malformed-url` (well-formedness only)
///
/// The `Url` arm is *not* a cross-reference check: it validates the URL
/// is well-formed (a pure, IO-free parse — **no network**, by design;
/// reachability is out of scope, issue #762). It runs in this pass
/// because well-formedness is pure and `--references` already gates it.
///
/// `ToCome` / `NotSure` are intentional placeholders and never flagged;
/// `FootnoteNumber` is validated by the always-on analyser
/// ([`check_footnotes`]); `File` is out of scope here (issue #761). All
/// emitted diagnostics default to [`DiagnosticSeverity::Warning`] —
/// callers apply `[diagnostics.rules]` via [`apply_rules`] for per-kind
/// overrides.
pub fn analyze_references(document: &Document) -> Vec<AnalysisDiagnostic> {
    use crate::reference_targets::{targets_from_reference_type, ReferenceTarget};
    use crate::references::target_resolves;

    let mut diagnostics = Vec::new();
    crate::utils::for_each_text_content(document, &mut |text| {
        for reference in extract_references(text) {
            let (kind, render): (DiagnosticKind, String) = match &reference.reference_type {
                ReferenceType::Session { target } if !target.trim().is_empty() => (
                    DiagnosticKind::MissingSessionTarget,
                    format!(
                        "Session reference [#{}] has no matching session",
                        target.trim()
                    ),
                ),
                ReferenceType::General { target } if !target.trim().is_empty() => (
                    DiagnosticKind::MissingDefinitionTarget,
                    format!("Reference [{}] has no matching definition", target.trim()),
                ),
                ReferenceType::AnnotationReference { label } if !label.trim().is_empty() => (
                    DiagnosticKind::MissingAnnotationTarget,
                    format!(
                        "Annotation reference [::{}] has no matching annotation",
                        label.trim()
                    ),
                ),
                ReferenceType::Url { target } if !target.trim().is_empty() => {
                    // URL references are validated for well-formedness
                    // only — a pure, IO-free parse check (no network: see
                    // [`url_is_malformed`]). This is self-contained (no
                    // document resolution), so it emits inline and
                    // `continue`s like the citation arm rather than
                    // falling through to the target-resolution tail.
                    let target = target.trim();
                    if url_is_malformed(target) {
                        diagnostics.push(AnalysisDiagnostic {
                            range: reference.range.clone(),
                            severity: DiagnosticSeverity::Warning,
                            kind: DiagnosticKind::MalformedUrl,
                            message: format!("URL [{target}] is malformed"),
                        });
                    }
                    continue;
                }
                ReferenceType::Citation(data) => {
                    // A citation may carry multiple keys; each is its own
                    // potential dangling target. Emit per unresolved key.
                    for key in &data.keys {
                        if key.trim().is_empty() {
                            continue;
                        }
                        let target = ReferenceTarget::CitationKey(key.trim().to_string());
                        if !target_resolves(document, &target) {
                            diagnostics.push(AnalysisDiagnostic {
                                range: reference.range.clone(),
                                severity: DiagnosticSeverity::Warning,
                                kind: DiagnosticKind::MissingCitationTarget,
                                message: format!(
                                    "Citation [@{}] has no matching annotation or definition",
                                    key.trim()
                                ),
                            });
                        }
                    }
                    continue;
                }
                // Placeholders, footnotes (always-on), URL/File (out of
                // scope), and empty-target references: skip.
                _ => continue,
            };

            // Non-citation kinds: resolve via the reference's targets and
            // emit when none match anywhere in the merged tree.
            let resolves = targets_from_reference_type(&reference.reference_type)
                .iter()
                .any(|t| target_resolves(document, t));
            if !resolves {
                diagnostics.push(AnalysisDiagnostic {
                    range: reference.range.clone(),
                    severity: DiagnosticSeverity::Warning,
                    kind,
                    message: render,
                });
            }
        }
    });
    diagnostics
}

/// Is `target` a malformed URL? Pure, IO-free well-formedness check —
/// **never opens a connection**. Classification (`ReferenceType::Url`)
/// already guarantees one of the `http://` / `https://` / `mailto:`
/// scheme prefixes, so this catches what classification can't: embedded
/// spaces, an empty host, and otherwise-unparseable targets.
///
/// A bare `url::Url::parse(...).is_err()` is sufficient: under the WHATWG
/// URL standard the `url` crate implements, the special schemes we
/// validate (`http`/`https`) require a non-empty host, so a missing host
/// (`https://`) already parse-fails with `EmptyHost`; `mailto:` is
/// host-less and parses fine — exactly the behavior we want, with no
/// scheme-specific host check needed.
///
/// A future opt-in `--check-urls-online` would layer network
/// reachability *on top* of this — deliberately unimplemented here
/// (issue #762: reachability out of scope).
fn url_is_malformed(target: &str) -> bool {
    url::Url::parse(target).is_err()
}

/// A non-include file-path reference and the range to blame it on.
///
/// Produced by [`collect_file_references`] for the opt-in
/// `check --references` *file-path* pass. The range is origin-stamped
/// (it comes from the reference's authoring file, via
/// [`extract_references`] for inline refs or the verbatim node's own
/// range), so a consumer that resolves `target` relative to that origin
/// — and blames findings on it — stays origin-faithful across an include
/// merge.
#[derive(Debug, Clone)]
pub struct FileReference {
    /// The raw path target as authored (`./x.txt`, `../y`, `/abs`).
    pub target: String,
    /// Origin-stamped range to resolve against and blame.
    pub range: Range,
}

/// Collect every **non-include** file-path reference in the (merged)
/// `document`: inline [`ReferenceType::File`] (`[./x.txt]`, `[../y]`,
/// `[/abs]`) and image/data verbatim `src=` parameters.
///
/// This is the pure (no-IO) half of the `check --references` file-path
/// check: it gathers the targets and their origin-stamped ranges; the
/// caller performs filesystem resolution + existence (which needs a
/// resolution root and disk access, neither of which belongs in a pure
/// `&Document` analysis).
///
/// `lex.include src=` is intentionally **not** collected: it is an
/// *annotation*, not a verbatim block, so it never matches the verbatim
/// `src=` arm — and after include expansion it has been spliced out
/// entirely (its path already validated by the base command, #759).
///
/// Inline refs reuse [`extract_references`], whose ranges are already
/// origin-stamped (see `inline::ReferenceWalker::make_range`). Verbatim
/// `src=` carries the verbatim node's own range, which the include
/// resolver stamps with the authoring file's origin.
pub fn collect_file_references(document: &Document) -> Vec<FileReference> {
    use lex_core::lex::ast::traits::AstNode;

    let mut refs = Vec::new();

    // Inline `[./x]` file references — origin-stamped via extract_references.
    crate::utils::for_each_text_content(document, &mut |text| {
        for reference in extract_references(text) {
            if let ReferenceType::File { target } = &reference.reference_type {
                if !target.trim().is_empty() {
                    refs.push(FileReference {
                        target: target.clone(),
                        range: reference.range.clone(),
                    });
                }
            }
        }
    });

    // Image/data verbatim `src=` parameters. The verbatim's own range
    // carries its origin; `lex.include` is an annotation, not a verbatim
    // block, so it is structurally excluded here.
    for item in document.root.iter_all_nodes() {
        if let ContentItem::VerbatimBlock(verbatim) = item {
            if let Some(src) = verbatim.src_parameter() {
                if !src.trim().is_empty() {
                    refs.push(FileReference {
                        target: src.to_string(),
                        range: verbatim.range().clone(),
                    });
                }
            }
        }
    }

    refs
}

/// Walk every label site in the document and re-classify via
/// [`classify_label`](lex_core::lex::assembling::stages::normalize_labels::classify_label).
/// Emits diagnostics for sites that strict-mode parsing would have
/// rejected — `doc.*` (forbidden) and unknown `lex.*` (not a
/// registered canonical). The LSP-side permissive parse keeps the
/// AST building so these surface as in-place diagnostics rather than
/// as a wholesale parse failure.
fn check_labels(document: &Document, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    use lex_core::lex::assembling::stages::normalize_labels::{
        classify_label, RejectReason, Resolution,
    };
    use lex_core::lex::ast::Label;

    fn emit(label: &Label, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        if let Resolution::Rejected(reason) = classify_label(&label.value) {
            // Reuse the normative wording from `RejectReason::message()`
            // so the strict-mode parser error and the permissive-mode
            // analysis diagnostic stay literally identical — no chance
            // of wording drift between the two surfaces.
            let message = reason.message();
            let kind = match reason {
                RejectReason::Forbidden { .. } => DiagnosticKind::ForbiddenLabelPrefix,
                RejectReason::UnknownCanonical { .. } => DiagnosticKind::UnknownLexCanonical,
            };
            diagnostics.push(AnalysisDiagnostic {
                range: label.location.clone(),
                severity: DiagnosticSeverity::Error,
                kind,
                message,
            });
        }
    }

    // Unified dispatch: every ContentItem flows through `walk_item`,
    // which emits the type-specific label sites (annotation label,
    // verbatim closer label, table cells/footnotes) exactly once and
    // then defers to `attached_annotations` + `item.children()` for
    // the uniform recursion. The earlier shape had type-specific
    // walkers (`walk_annotation`, `walk_verbatim`, `walk_table`) that
    // descended on their own and then `walk_item` descended again —
    // duplicate-walk regression caught by Copilot's review on PR 589.
    fn walk_item(item: &ContentItem, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        match item {
            ContentItem::Annotation(a) => emit(&a.data.label, diagnostics),
            ContentItem::VerbatimBlock(v) => emit(&v.closing_data.label, diagnostics),
            ContentItem::Table(t) => {
                for row in t.header_rows.iter().chain(t.body_rows.iter()) {
                    for cell in &row.cells {
                        for child in cell.children.iter() {
                            walk_item(child, diagnostics);
                        }
                    }
                }
                if let Some(footnotes) = t.footnotes.as_ref() {
                    for ann in footnotes.annotations() {
                        walk_annotation(ann, diagnostics);
                    }
                    for fn_item in footnotes.items.iter() {
                        walk_item(fn_item, diagnostics);
                    }
                }
            }
            _ => {}
        }
        // Attached annotations (sessions, paragraphs, lists, list
        // items, verbatim blocks, tables — see `attached_annotations`).
        if let Some(attached) = attached_annotations(item) {
            for annotation in attached {
                walk_annotation(annotation, diagnostics);
            }
        }
        // Generic child descent. For ContentItem::Annotation,
        // `item.children()` returns the annotation's body children, so
        // type-specific walking of nested annotations is not needed.
        if let Some(children) = item.children() {
            for child in children {
                walk_item(child, diagnostics);
            }
        }
    }

    fn walk_annotation(annotation: &Annotation, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        emit(&annotation.data.label, diagnostics);
        for child in annotation.children.iter() {
            walk_item(child, diagnostics);
        }
    }

    fn walk_session(session: &Session, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        for annotation in session.annotations() {
            walk_annotation(annotation, diagnostics);
        }
        for child in &session.children {
            walk_item(child, diagnostics);
        }
    }

    fn attached_annotations(item: &ContentItem) -> Option<&[Annotation]> {
        match item {
            ContentItem::Session(s) => Some(s.annotations()),
            ContentItem::Paragraph(p) => Some(p.annotations()),
            ContentItem::Definition(d) => Some(d.annotations()),
            ContentItem::List(l) => Some(l.annotations()),
            ContentItem::ListItem(li) => Some(li.annotations()),
            ContentItem::VerbatimBlock(v) => Some(v.annotations()),
            ContentItem::Table(t) => Some(t.annotations()),
            _ => None,
        }
    }

    // Document-level annotations.
    for annotation in document.annotations() {
        walk_annotation(annotation, diagnostics);
    }
    // Root session walks.
    walk_session(&document.root, diagnostics);
}

fn check_footnotes(document: &Document, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    // Numbered definitions reachable from outside any table: :: notes ::
    // annotated lists at document or session scope.
    let outer_defs: HashSet<u32> = crate::utils::collect_footnote_definitions(document)
        .into_iter()
        .filter_map(|(label, _)| label.parse::<u32>().ok())
        .collect();

    // References outside tables resolve to `outer_defs`; references inside a
    // table resolve first to that table's own positional footnote list
    // (`table.footnotes`) and then fall back to `outer_defs`.
    if let Some(title) = &document.title {
        check_text(&title.content, &outer_defs, diagnostics);
    }
    for annotation in document.annotations() {
        check_annotation(annotation, &outer_defs, diagnostics);
    }
    check_session(&document.root, &outer_defs, diagnostics);
}

fn check_session(
    session: &Session,
    defs: &HashSet<u32>,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
    check_text(&session.title, defs, diagnostics);
    for annotation in session.annotations() {
        check_annotation(annotation, defs, diagnostics);
    }
    for child in session.children.iter() {
        check_content(child, defs, diagnostics);
    }
}

fn check_content(
    item: &ContentItem,
    defs: &HashSet<u32>,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
    match item {
        ContentItem::Paragraph(p) => {
            for line in &p.lines {
                if let ContentItem::TextLine(tl) = line {
                    check_text(&tl.content, defs, diagnostics);
                }
            }
            for annotation in p.annotations() {
                check_annotation(annotation, defs, diagnostics);
            }
        }
        ContentItem::Session(s) => check_session(s, defs, diagnostics),
        ContentItem::List(list) => {
            for annotation in list.annotations() {
                check_annotation(annotation, defs, diagnostics);
            }
            for entry in &list.items {
                if let ContentItem::ListItem(li) = entry {
                    for text in &li.text {
                        check_text(text, defs, diagnostics);
                    }
                    for annotation in li.annotations() {
                        check_annotation(annotation, defs, diagnostics);
                    }
                    for child in li.children.iter() {
                        check_content(child, defs, diagnostics);
                    }
                }
            }
        }
        ContentItem::Definition(def) => {
            check_text(&def.subject, defs, diagnostics);
            for annotation in def.annotations() {
                check_annotation(annotation, defs, diagnostics);
            }
            for child in def.children.iter() {
                check_content(child, defs, diagnostics);
            }
        }
        ContentItem::Annotation(a) => check_annotation(a, defs, diagnostics),
        ContentItem::VerbatimBlock(v) => {
            check_text(&v.subject, defs, diagnostics);
            for annotation in v.annotations() {
                check_annotation(annotation, defs, diagnostics);
            }
        }
        ContentItem::Table(table) => check_table(table, defs, diagnostics),
        _ => {}
    }
}

fn check_annotation(
    annotation: &Annotation,
    defs: &HashSet<u32>,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
    for child in annotation.children.iter() {
        check_content(child, defs, diagnostics);
    }
}

fn check_table(
    table: &Table,
    outer_defs: &HashSet<u32>,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
    // Extend the in-scope definitions with the table's positional footnote
    // list. The table's own numbered items shadow nothing — they just add
    // table-local numbers that references inside this table may resolve to.
    // Fast path: most tables have no footnotes, so reuse `outer_defs` rather
    // than cloning it into a new `HashSet` for every such table.
    let table_defs = table_footnote_numbers(table);
    if table_defs.is_empty() {
        check_table_text(table, outer_defs, diagnostics);
        return;
    }
    let mut scope = outer_defs.clone();
    scope.extend(table_defs);
    check_table_text(table, &scope, diagnostics);
}

fn check_table_text(table: &Table, defs: &HashSet<u32>, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    check_text(&table.subject, defs, diagnostics);
    for row in table.all_rows() {
        for cell in &row.cells {
            check_text(&cell.content, defs, diagnostics);
        }
    }
    for annotation in table.annotations() {
        check_annotation(annotation, defs, diagnostics);
    }
}

fn table_footnote_numbers(table: &Table) -> HashSet<u32> {
    let Some(list) = &table.footnotes else {
        return HashSet::new();
    };
    let mut numbers = HashSet::new();
    for entry in &list.items {
        if let ContentItem::ListItem(li) = entry {
            let label = li
                .marker()
                .trim()
                .trim_end_matches(['.', ')', ':'].as_ref())
                .trim();
            if let Ok(n) = label.parse::<u32>() {
                numbers.insert(n);
            }
        }
    }
    numbers
}

fn check_text(text: &TextContent, defs: &HashSet<u32>, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    for reference in extract_references(text) {
        if let ReferenceType::FootnoteNumber { number } = reference.reference_type {
            if !defs.contains(&number) {
                diagnostics.push(AnalysisDiagnostic {
                    range: reference.range,
                    severity: DiagnosticSeverity::Error,
                    kind: DiagnosticKind::MissingFootnoteDefinition,
                    message: format!(
                        "Footnote [{number}] has no matching footnote definition in scope"
                    ),
                });
            }
        }
    }
}

fn check_tables(document: &Document, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    visit_tables_in_session(&document.root, diagnostics);
}

fn visit_tables_in_session(session: &Session, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    for child in session.children.iter() {
        visit_tables_in_content(child, diagnostics);
    }
}

fn visit_tables_in_content(item: &ContentItem, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    match item {
        ContentItem::Table(table) => check_table_columns(table, diagnostics),
        ContentItem::Session(session) => visit_tables_in_session(session, diagnostics),
        ContentItem::Definition(def) => {
            for child in def.children.iter() {
                visit_tables_in_content(child, diagnostics);
            }
        }
        ContentItem::List(list) => {
            for entry in &list.items {
                if let ContentItem::ListItem(li) = entry {
                    for child in li.children.iter() {
                        visit_tables_in_content(child, diagnostics);
                    }
                }
            }
        }
        ContentItem::Annotation(ann) => {
            for child in ann.children.iter() {
                visit_tables_in_content(child, diagnostics);
            }
        }
        _ => {}
    }
}

/// Check that all rows in a table have the same effective column count.
///
/// The effective width of a row accounts for both colspans of its own cells
/// and rowspan carry-over from cells in prior rows that extend into it.
/// Rows with different effective widths indicate a structural error (missing
/// or extra cells).
fn check_table_columns(table: &Table, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    let rows: Vec<_> = table.all_rows().collect();
    if rows.len() < 2 {
        return;
    }

    let widths = compute_row_widths(&rows);
    let expected = widths[0];
    for (i, &width) in widths.iter().enumerate().skip(1) {
        if width != expected {
            diagnostics.push(AnalysisDiagnostic {
                range: rows[i].location.clone(),
                severity: DiagnosticSeverity::Warning,
                kind: DiagnosticKind::TableInconsistentColumns,
                message: format!(
                    "Row has {width} columns, expected {expected} (matching first row)"
                ),
            });
        }
    }
}

/// Simulate the virtual table grid to compute each row's effective width.
///
/// `carry[col]` tracks how many more rows (including the current one) a cell
/// placed in a prior row still occupies column `col`. Own cells skip columns
/// where `carry[col] > 0` (those are held by a cell from above via rowspan).
fn compute_row_widths(rows: &[&TableRow]) -> Vec<usize> {
    let mut carry: Vec<usize> = Vec::new();
    let mut widths = Vec::with_capacity(rows.len());

    for row in rows {
        let mut col = 0;
        for cell in &row.cells {
            while col < carry.len() && carry[col] > 0 {
                col += 1;
            }
            let end = col + cell.colspan;
            if end > carry.len() {
                carry.resize(end, 0);
            }
            for slot in carry.iter_mut().take(end).skip(col) {
                *slot = cell.rowspan;
            }
            col = end;
        }

        let width = carry
            .iter()
            .rposition(|&r| r > 0)
            .map(|i| i + 1)
            .unwrap_or(0);
        widths.push(width);

        // Columns at or beyond `width` are guaranteed 0 (that's how width is
        // defined), so limit the decrement to the active range and drop the
        // trailing zeros to keep `carry` proportional to the live grid.
        for c in carry.iter_mut().take(width) {
            if *c > 0 {
                *c -= 1;
            }
        }
        carry.truncate(width);
    }

    widths
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::parsing::parse_document_permissive;
    use lex_core::lex::testing::lexplore::Lexplore;

    fn unclosed_annotation_diags(source: &str) -> Vec<AnalysisDiagnostic> {
        let doc = parse_document_permissive(source).expect("permissive parse");
        analyze(&doc)
            .into_iter()
            .filter(|d| d.kind == DiagnosticKind::UnclosedAnnotation)
            .collect()
    }

    #[test]
    fn unclosed_annotation_warns_on_open_form() {
        // `:: note severity=high` (no closing `::`) parses as a paragraph; the
        // analyser flags it so the author knows it isn't an annotation (lex#700).
        let diags = unclosed_annotation_diags("Open form:\n\t:: note severity=high\n");
        assert_eq!(diags.len(), 1, "expected one unclosed-annotation warning");
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
        assert_eq!(diags[0].kind.code(), "unclosed-annotation");
    }

    #[test]
    fn unclosed_annotation_silent_on_closed_form_and_prose() {
        // A properly closed annotation is its own node, not a flagged paragraph.
        assert!(unclosed_annotation_diags(":: note severity=high ::\n\nBody.\n").is_empty());
        // Prose that merely mentions `::` is not flagged.
        assert!(unclosed_annotation_diags("Use :: to start a marker.\n").is_empty());
    }

    #[test]
    fn looks_like_unclosed_annotation_heuristic() {
        assert!(looks_like_unclosed_annotation(":: note"));
        assert!(looks_like_unclosed_annotation("    :: note severity=high"));
        // A `::` inside a quoted value is not a structural close, so this is still
        // an unclosed annotation (lex#704 review).
        assert!(looks_like_unclosed_annotation(":: note foo=\":: value\""));
        assert!(!looks_like_unclosed_annotation(":: note ::"));
        assert!(!looks_like_unclosed_annotation(
            ":: note foo=\":: value\" ::"
        )); // real close
        assert!(!looks_like_unclosed_annotation("::note")); // no whitespace after marker
        assert!(!looks_like_unclosed_annotation("::")); // no label
        assert!(!looks_like_unclosed_annotation("just prose"));
    }

    fn footnote_diags(doc: &Document) -> Vec<AnalysisDiagnostic> {
        analyze(doc)
            .into_iter()
            .filter(|d| d.kind == DiagnosticKind::MissingFootnoteDefinition)
            .collect()
    }

    fn label_diags(source: &str) -> Vec<AnalysisDiagnostic> {
        let doc = parse_document_permissive(source).expect("permissive parse");
        analyze(&doc)
            .into_iter()
            .filter(|d| {
                matches!(
                    d.kind,
                    DiagnosticKind::ForbiddenLabelPrefix | DiagnosticKind::UnknownLexCanonical
                )
            })
            .collect()
    }

    #[test]
    fn check_labels_emits_for_doc_prefix() {
        let diags = label_diags(":: doc.table :: x\n\nBody.\n");
        assert_eq!(diags.len(), 1, "expected 1 forbidden-prefix diagnostic");
        assert_eq!(diags[0].kind, DiagnosticKind::ForbiddenLabelPrefix);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
        assert!(
            diags[0].message.contains("doc.table") && diags[0].message.contains("reserved"),
            "message names the offending prefix; got: {}",
            diags[0].message
        );
    }

    #[test]
    fn check_labels_emits_for_unknown_lex_canonical() {
        let diags = label_diags(":: lex.foobar :: x\n\nBody.\n");
        assert_eq!(diags.len(), 1, "expected 1 unknown-canonical diagnostic");
        assert_eq!(diags[0].kind, DiagnosticKind::UnknownLexCanonical);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
        assert!(
            diags[0].message.contains("lex.foobar"),
            "message names the offending label; got: {}",
            diags[0].message
        );
    }

    #[test]
    fn check_labels_silent_on_accepted_forms() {
        // Shortcut, prefix-stripped, canonical, and community labels
        // all accept silently — analysis only flags the two reject
        // categories from `classify_label`.
        let sources = [
            ":: author :: Alice\n\nBody.\n",
            ":: metadata.author :: Alice\n\nBody.\n",
            ":: lex.metadata.author :: Alice\n\nBody.\n",
            ":: acme.task :: x\n\nBody.\n",
        ];
        for src in sources {
            let diags = label_diags(src);
            assert!(
                diags.is_empty(),
                "no label diagnostics expected for {src:?}; got {diags:?}"
            );
        }
    }

    #[test]
    fn check_labels_finds_verbatim_closer_violations() {
        let diags =
            label_diags("Table:\n    | a | b |\n    |---|---|\n    | 1 | 2 |\n:: doc.table ::\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::ForbiddenLabelPrefix);
    }

    #[test]
    fn check_labels_emits_each_offending_site_exactly_once() {
        // Regression for Copilot's PR 589 callout: the earlier
        // walker shape descended into a node's children twice (once
        // via the type-specific helper, once via the generic
        // `walk_item` fallback), which produced duplicate
        // diagnostics for any forbidden label nested inside another
        // label-bearing site. Three nested + adjacent forbidden
        // labels should produce exactly three diagnostics, not six.
        let src = ":: doc.outer ::\n    :: doc.inner :: nested body\n\n:: doc.sibling :: x\n";
        let diags = label_diags(src);
        assert_eq!(
            diags.len(),
            3,
            "exactly one diagnostic per offending site: {diags:?}"
        );
        for d in &diags {
            assert_eq!(d.kind, DiagnosticKind::ForbiddenLabelPrefix);
        }
    }

    #[test]
    fn detects_missing_footnote_definition() {
        let doc = Lexplore::footnotes(1).parse().unwrap();
        let diags = analyze(&doc);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::MissingFootnoteDefinition);
    }

    #[test]
    fn ignores_valid_footnote_with_notes_annotation() {
        // :: notes :: annotated list at the document root provides the definitions
        let doc = Lexplore::footnotes(2).parse().unwrap();
        assert!(footnote_diags(&doc).is_empty());
    }

    #[test]
    fn ignores_valid_list_footnote_in_session() {
        // :: notes :: inside a session
        let doc = Lexplore::footnotes(3).parse().unwrap();
        assert!(footnote_diags(&doc).is_empty());
    }

    #[test]
    fn list_without_notes_annotation_is_not_footnotes() {
        // A "Notes" session without :: notes :: does NOT define footnotes
        let doc = Lexplore::footnotes(4).parse().unwrap();
        assert_eq!(footnote_diags(&doc).len(), 1);
    }

    fn table_diags(doc: &Document) -> Vec<AnalysisDiagnostic> {
        analyze(doc)
            .into_iter()
            .filter(|d| d.kind == DiagnosticKind::TableInconsistentColumns)
            .collect()
    }

    #[test]
    fn detects_inconsistent_table_columns() {
        // table-13: 3-col header, 2-col row, 3-col row — middle row is short.
        let doc = Lexplore::table(13).parse().unwrap();
        let diags = table_diags(&doc);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("2 columns"));
        assert!(diags[0].message.contains("expected 3"));
    }

    #[test]
    fn consistent_table_no_diagnostic() {
        // table-01: minimal 2-column table, all rows consistent.
        let doc = Lexplore::table(1).parse().unwrap();
        assert!(table_diags(&doc).is_empty());
    }

    #[test]
    fn table_with_rowspan_counts_carry_over() {
        // table-17: rowspan via ^^ — effective widths remain consistent across rows.
        let doc = Lexplore::table(17).parse().unwrap();
        let diags = table_diags(&doc);
        assert!(
            diags.is_empty(),
            "rowspan carry-over should not trigger inconsistent-columns, got: {diags:?}"
        );
    }

    #[test]
    fn table_with_colspan_and_rowspan_mixed() {
        // table-18: combined >> colspan and ^^ rowspan; effective widths stay consistent.
        let doc = Lexplore::table(18).parse().unwrap();
        let diags = table_diags(&doc);
        assert!(
            diags.is_empty(),
            "mixed colspan/rowspan should not trigger inconsistent-columns, got: {diags:?}"
        );
    }

    #[test]
    fn table_with_colspan_counts_effective_width() {
        // table-04: colspan via >> contributes to effective width; all rows consistent.
        let doc = Lexplore::table(4).parse().unwrap();
        assert!(table_diags(&doc).is_empty());
    }

    #[test]
    fn footnote_ref_in_table_cell_is_checked() {
        // footnotes-09: table cell contains [1] but no footnote definition
        // anywhere in scope — document, session, or table-local.
        let doc = Lexplore::footnotes(9).parse().unwrap();
        let diags = footnote_diags(&doc);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("[1]"));
    }

    #[test]
    fn table_scoped_footnotes_resolve_cell_refs() {
        // footnotes-11: cell refs [1] and [2] resolve to the table's own
        // positional footnote list (no :: notes :: annotation needed).
        let doc = Lexplore::footnotes(11).parse().unwrap();
        let diags = footnote_diags(&doc);
        assert!(
            diags.is_empty(),
            "table-scoped cell refs should resolve to table.footnotes, got: {diags:?}"
        );
    }

    #[test]
    fn table_scoped_footnotes_do_not_leak_out() {
        // footnotes-12: a [1] ref in body text outside the table must NOT
        // resolve to the table's own positional footnote list even when the
        // numbers happen to match. The table's list is table-local.
        let doc = Lexplore::footnotes(12).parse().unwrap();
        let diags = footnote_diags(&doc);
        assert_eq!(
            diags.len(),
            1,
            "only the paragraph ref [1] should be unresolved, got: {diags:?}"
        );
        assert!(diags[0].message.contains("[1]"));
    }

    // ─────────────── apply_rules / DiagnosticKind::code ───────────────

    fn dummy_diag(kind: DiagnosticKind, severity: DiagnosticSeverity) -> AnalysisDiagnostic {
        AnalysisDiagnostic {
            range: Range::default(),
            severity,
            kind,
            message: "test".into(),
        }
    }

    #[test]
    fn diagnostic_kind_code_matches_lookup_for_every_builtin() {
        // Drift test: every built-in DiagnosticKind variant must have a
        // matching entry in DiagnosticsRulesConfig::lookup_by_code so
        // configuration overrides reach every rule.
        let rules = DiagnosticsRulesConfig::default();
        for kind in [
            DiagnosticKind::MissingFootnoteDefinition,
            DiagnosticKind::UnusedFootnoteDefinition,
            DiagnosticKind::TableInconsistentColumns,
            DiagnosticKind::ForbiddenLabelPrefix,
            DiagnosticKind::UnknownLexCanonical,
            DiagnosticKind::SchemaValidation(SchemaValidationKind::UnknownLabel),
            DiagnosticKind::SchemaValidation(SchemaValidationKind::MissingParam),
            DiagnosticKind::SchemaValidation(SchemaValidationKind::ParamTypeMismatch),
            DiagnosticKind::SchemaValidation(SchemaValidationKind::BadAttachment),
            DiagnosticKind::SchemaValidation(SchemaValidationKind::BodyShapeMismatch),
        ] {
            let code = kind.code();
            assert!(
                rules.lookup_by_code(&code).is_some(),
                "DiagnosticsRulesConfig is missing a field for built-in code {code:?} \
                 — add it to lookup_by_code (and likely as a struct field too)"
            );
        }
    }

    #[test]
    fn handler_code_carries_namespace_prefix() {
        // Wire-shape contract (spec §9): the wire `code` is the
        // namespace-prefixed form so a `.lex.toml` rule like
        // `"acme.task-stuck" = "deny"` actually matches what the
        // handler emitted. The handler supplies the bare leaf (`code`
        // field on `Diagnostic`); the analyser glues on the namespace.
        let with_code = DiagnosticKind::Handler {
            namespace: "acme".into(),
            code: Some("task-stuck".into()),
        };
        assert_eq!(with_code.code(), "acme.task-stuck");
        // Code-less handler diagnostic gets a per-namespace fallback
        // — users can target it as `"acme.diagnostic" = "warn"` rather
        // than a single global literal.
        let without_code = DiagnosticKind::Handler {
            namespace: "acme".into(),
            code: None,
        };
        assert_eq!(without_code.code(), "acme.diagnostic");
    }

    #[test]
    fn apply_rules_matches_extension_code_via_side_channel() {
        // End-to-end: handler emits `acme.foo`, user configured
        // `"acme.foo" = "allow"` in `[diagnostics.rules]` (now
        // captured into the LSP's `extension_diagnostic_rules`
        // side-channel by the `on_unknown_key` callback rather than
        // landing in a `#[serde(flatten)] extra` map); diagnostic
        // gets dropped.
        use std::collections::BTreeMap;
        // The closure mirrors `LoadedLexConfig::lookup_diagnostic_rule`:
        // built-in first, side-channel second.
        let lookup = |code: &str, side: &BTreeMap<String, lex_config::RuleConfig>| {
            DiagnosticsRulesConfig::default()
                .lookup_by_code(code)
                .cloned()
                .or_else(|| side.get(code).cloned())
        };

        let side: BTreeMap<String, lex_config::RuleConfig> = [(
            "acme.foo".to_string(),
            lex_config::RuleConfig::Bare(Severity::Allow),
        )]
        .into_iter()
        .collect();
        let mut diags = vec![dummy_diag(
            DiagnosticKind::Handler {
                namespace: "acme".into(),
                code: Some("foo".into()),
            },
            DiagnosticSeverity::Error,
        )];
        apply_rules(&mut diags, |code| lookup(code, &side));
        assert!(diags.is_empty(), "allow drops the extension diagnostic");

        // `warn` keeps the intrinsic severity (Error stays Error).
        let side: BTreeMap<String, lex_config::RuleConfig> = [(
            "acme.foo".to_string(),
            lex_config::RuleConfig::Bare(Severity::Warn),
        )]
        .into_iter()
        .collect();
        let mut diags = vec![dummy_diag(
            DiagnosticKind::Handler {
                namespace: "acme".into(),
                code: Some("foo".into()),
            },
            DiagnosticSeverity::Error,
        )];
        apply_rules(&mut diags, |code| lookup(code, &side));
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            DiagnosticSeverity::Error,
            "warn preserves the handler's intrinsic severity"
        );

        // `deny` is a no-op when the intrinsic is already Error, but
        // still keeps the diagnostic — symmetry with built-ins.
        let side: BTreeMap<String, lex_config::RuleConfig> = [(
            "acme.foo".to_string(),
            lex_config::RuleConfig::Bare(Severity::Deny),
        )]
        .into_iter()
        .collect();
        let mut diags = vec![dummy_diag(
            DiagnosticKind::Handler {
                namespace: "acme".into(),
                code: Some("foo".into()),
            },
            DiagnosticSeverity::Error,
        )];
        apply_rules(&mut diags, |code| lookup(code, &side));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);

        // A configured rule whose code doesn't match the emitted one
        // passes the diagnostic through untouched.
        let side: BTreeMap<String, lex_config::RuleConfig> = [(
            "acme.other".to_string(),
            lex_config::RuleConfig::Bare(Severity::Allow),
        )]
        .into_iter()
        .collect();
        let mut diags = vec![dummy_diag(
            DiagnosticKind::Handler {
                namespace: "acme".into(),
                code: Some("foo".into()),
            },
            DiagnosticSeverity::Warning,
        )];
        apply_rules(&mut diags, |code| lookup(code, &side));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn apply_rules_allow_drops_diagnostic() {
        let mut diags = vec![dummy_diag(
            DiagnosticKind::MissingFootnoteDefinition,
            DiagnosticSeverity::Error,
        )];
        let rules = DiagnosticsRulesConfig {
            missing_footnote: lex_config::RuleConfig::Bare(Severity::Allow),
            ..Default::default()
        };
        apply_rules(&mut diags, |code| rules.lookup_by_code(code).cloned());
        assert!(diags.is_empty(), "allow should drop the diagnostic");
    }

    #[test]
    fn apply_rules_deny_upgrades_to_error() {
        let mut diags = vec![dummy_diag(
            DiagnosticKind::TableInconsistentColumns,
            DiagnosticSeverity::Warning,
        )];
        let rules = DiagnosticsRulesConfig {
            table_inconsistent_columns: lex_config::RuleConfig::Bare(Severity::Deny),
            ..Default::default()
        };
        apply_rules(&mut diags, |code| rules.lookup_by_code(code).cloned());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
    }

    #[test]
    fn apply_rules_warn_keeps_intrinsic_severity() {
        let mut diags = vec![dummy_diag(
            DiagnosticKind::TableInconsistentColumns,
            DiagnosticSeverity::Warning,
        )];
        let rules = DiagnosticsRulesConfig {
            table_inconsistent_columns: lex_config::RuleConfig::Bare(Severity::Warn),
            ..Default::default()
        };
        apply_rules(&mut diags, |code| rules.lookup_by_code(code).cloned());
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            DiagnosticSeverity::Warning,
            "warn should not change the intrinsic severity"
        );
    }

    #[test]
    fn apply_rules_unknown_code_is_passthrough() {
        // An extension-emitted diagnostic with a code the registry
        // does not know about must pass through unmodified. The
        // handler's `code` is the bare leaf — the analyser glues on
        // `acme.` to produce wire `acme.unknown`.
        let mut diags = vec![dummy_diag(
            DiagnosticKind::Handler {
                namespace: "acme".into(),
                code: Some("unknown".into()),
            },
            DiagnosticSeverity::Warning,
        )];
        let rules = DiagnosticsRulesConfig::default();
        apply_rules(&mut diags, |code| rules.lookup_by_code(code).cloned());
        assert_eq!(diags.len(), 1, "unknown codes should pass through");
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn apply_rules_preserves_order_of_kept_diagnostics() {
        // Mixed stream: one to drop, one to keep, one to upgrade.
        let mut diags = vec![
            dummy_diag(
                DiagnosticKind::MissingFootnoteDefinition,
                DiagnosticSeverity::Error,
            ),
            dummy_diag(
                DiagnosticKind::UnusedFootnoteDefinition,
                DiagnosticSeverity::Warning,
            ),
            dummy_diag(
                DiagnosticKind::TableInconsistentColumns,
                DiagnosticSeverity::Warning,
            ),
        ];
        let rules = DiagnosticsRulesConfig {
            missing_footnote: lex_config::RuleConfig::Bare(Severity::Allow),
            table_inconsistent_columns: lex_config::RuleConfig::Bare(Severity::Deny),
            ..Default::default()
        };
        apply_rules(&mut diags, |code| rules.lookup_by_code(code).cloned());
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].kind, DiagnosticKind::UnusedFootnoteDefinition);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
        assert_eq!(diags[1].kind, DiagnosticKind::TableInconsistentColumns);
        assert_eq!(diags[1].severity, DiagnosticSeverity::Error);
    }

    // ========================================================================
    // analyze_references (opt-in `check --references`) unit tests
    // ========================================================================

    fn reference_diags(source: &str) -> Vec<AnalysisDiagnostic> {
        let doc = parse_document_permissive(source).expect("permissive parse");
        analyze_references(&doc)
    }

    fn ref_codes(source: &str) -> Vec<String> {
        let mut codes: Vec<String> = reference_diags(source)
            .into_iter()
            .map(|d| d.kind.code().into_owned())
            .collect();
        codes.sort();
        codes
    }

    #[test]
    fn references_pass_is_not_run_by_the_always_on_analyser() {
        // A dangling definition reference produces nothing from `analyze`
        // (the always-on path) — only the opt-in pass flags it. This pins
        // the separation that keeps the LSP from emitting these unasked.
        let doc = parse_document_permissive("Body with a [Dangling] reference.\n")
            .expect("permissive parse");
        let always_on = analyze(&doc);
        assert!(
            always_on
                .iter()
                .all(|d| !d.kind.code().starts_with("missing-")
                    || d.kind == DiagnosticKind::MissingFootnoteDefinition),
            "always-on analyser must not emit reference-target diagnostics"
        );
    }

    #[test]
    fn dangling_definition_reference_flagged() {
        let codes = ref_codes("1. Intro\n\n    See [Nope].\n");
        assert_eq!(codes, vec!["missing-definition-target"]);
    }

    #[test]
    fn dangling_session_reference_flagged() {
        let codes = ref_codes("1. Intro\n\n    See [#9.9].\n");
        assert_eq!(codes, vec!["missing-session-target"]);
    }

    #[test]
    fn dangling_annotation_reference_flagged() {
        let codes = ref_codes("1. Intro\n\n    See [::ghost].\n");
        assert_eq!(codes, vec!["missing-annotation-target"]);
    }

    #[test]
    fn dangling_citation_flagged() {
        let codes = ref_codes("1. Intro\n\n    See [@missing2024].\n");
        assert_eq!(codes, vec!["missing-citation-target"]);
    }

    #[test]
    fn resolved_references_are_clean() {
        // Definition + annotation + session all defined; references to
        // each resolve and produce no findings.
        let source = ":: mynote ::\n\
             \x20   Note body.\n\
             \n\
             Cache:\n\
             \x20   Definition body.\n\
             \n\
             2. Topic\n\
             \n\
             \x20   See [Cache] and [::mynote] and [#2].\n";
        assert!(
            reference_diags(source).is_empty(),
            "resolved references must be clean: {:?}",
            reference_diags(source)
        );
    }

    #[test]
    fn citation_resolves_via_annotation_label() {
        // `[@spec]` resolves to a `:: spec ::` annotation (its label is a
        // citation key too).
        let source = ":: spec ::\n    Body.\n\n1. Intro\n\n    See [@spec].\n";
        assert!(reference_diags(source).is_empty());
    }

    #[test]
    fn annotation_matching_is_case_insensitive() {
        // `[::MyNote]` resolves to `:: mynote ::` — resolution is
        // case-insensitive, mirroring `references::reference_matches`.
        let source = ":: mynote ::\n    Body.\n\n1. Intro\n\n    See [::MyNote].\n";
        assert!(reference_diags(source).is_empty());
    }

    #[test]
    fn placeholders_never_flagged() {
        // `[TK]` / `[TK-id]` and an unclassifiable reference are
        // intentional placeholders — never flagged.
        assert!(reference_diags("1. Intro\n\n    A [TK] and [TK-later].\n").is_empty());
    }

    #[test]
    fn each_unresolved_citation_key_is_flagged() {
        // A multi-key citation flags each unresolved key independently —
        // both `@a` and `@b`, not just the first. Exactly two pins the
        // per-key behaviour against a regression that reports only one.
        let diags = reference_diags("1. Intro\n\n    See [@a; @b].\n");
        let citation = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::MissingCitationTarget)
            .count();
        assert_eq!(
            citation, 2,
            "both unresolved keys must be flagged: {diags:?}"
        );
    }

    #[test]
    fn reference_findings_default_to_warning() {
        let diags = reference_diags("1. Intro\n\n    See [Nope].\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
    }

    // ========================================================================
    // URL well-formedness (issue #762). Validated inside analyze_references;
    // pure parse, no network.
    // ========================================================================

    #[test]
    fn malformed_url_embedded_space_flagged() {
        // An embedded space makes the URL unparseable.
        let codes = ref_codes("1. Intro\n\n    See [https://exa mple.com].\n");
        assert_eq!(codes, vec!["malformed-url"]);
    }

    #[test]
    fn malformed_url_empty_host_flagged() {
        // `https://` with no host is well-formed-prefix but empty-host.
        let codes = ref_codes("1. Intro\n\n    See [https:// ].\n");
        assert_eq!(codes, vec!["malformed-url"]);
    }

    #[test]
    fn well_formed_https_url_not_flagged() {
        assert!(
            reference_diags("1. Intro\n\n    See [https://example.com/path?q=1].\n").is_empty(),
            "a well-formed https URL must not be flagged"
        );
    }

    #[test]
    fn well_formed_http_url_not_flagged() {
        assert!(reference_diags("1. Intro\n\n    See [http://example.com].\n").is_empty());
    }

    #[test]
    fn well_formed_mailto_not_flagged() {
        // `mailto:` has no host component — an empty host is expected and
        // must not be flagged.
        assert!(
            reference_diags("1. Intro\n\n    Write [mailto:hi@example.com].\n").is_empty(),
            "a well-formed mailto must not be flagged"
        );
    }

    #[test]
    fn malformed_url_defaults_to_warning() {
        let diags = reference_diags("1. Intro\n\n    See [https://exa mple.com].\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::MalformedUrl);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn url_check_makes_no_network_calls_by_construction() {
        // `url_is_malformed` is a pure parse over a string — it borrows no
        // socket, client, or runtime, so the default check path cannot
        // make a network call. Exercising both a well-formed and a
        // malformed URL here documents that the only work done is parsing.
        assert!(!url_is_malformed("https://example.com"));
        assert!(url_is_malformed("https://exa mple.com"));
        assert!(!url_is_malformed("mailto:a@b.com"));
    }

    // ========================================================================
    // collect_file_references (file-path pass, #761) unit tests
    // ========================================================================

    fn file_ref_targets(source: &str) -> Vec<String> {
        let doc = parse_document_permissive(source).expect("permissive parse");
        let mut targets: Vec<String> = collect_file_references(&doc)
            .into_iter()
            .map(|r| r.target)
            .collect();
        targets.sort();
        targets
    }

    #[test]
    fn collects_inline_file_references() {
        // The three inline file-reference shapes (`./`, `../`, `/`) are
        // all collected; a non-file `[General]` reference is not.
        let source = "1. Intro\n\n    See [./a.txt] and [../b] and [/c] but not [Nope].\n";
        assert_eq!(
            file_ref_targets(source),
            vec!["../b".to_string(), "./a.txt".to_string(), "/c".to_string()]
        );
    }

    #[test]
    fn collects_verbatim_src_but_not_lex_include() {
        // An image verbatim `src=` is collected. `lex.include` is an
        // annotation (not a verbatim block) and is structurally excluded
        // — collecting it here would double-validate a path the base
        // command already checks via expansion.
        let source = "Photo:\n    Caption.\n:: image src=./diagram.png ::\n\n";
        assert_eq!(file_ref_targets(source), vec!["./diagram.png".to_string()]);
    }

    #[test]
    fn ignores_url_references() {
        // URLs are out of scope for the file-path pass (#762 owns them).
        assert!(file_ref_targets("1. Intro\n\n    See [https://example.com].\n").is_empty());
    }
}
