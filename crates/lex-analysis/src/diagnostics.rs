use crate::inline::extract_references;
use lex_config::{DiagnosticsRulesConfig, Severity};
use lex_core::lex::ast::{
    Annotation, ContentItem, Document, Range, Session, Table, TableRow, TextContent,
};
use lex_core::lex::inlines::ReferenceType;
use lex_extension_host::Registry;
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
    /// editors can filter by extension. `code` mirrors the wire
    /// `Diagnostic.code` field.
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
    pub fn code(&self) -> String {
        match self {
            DiagnosticKind::MissingFootnoteDefinition => "missing-footnote".to_string(),
            DiagnosticKind::UnusedFootnoteDefinition => "unused-footnote".to_string(),
            DiagnosticKind::TableInconsistentColumns => "table-inconsistent-columns".to_string(),
            DiagnosticKind::SchemaValidation(kind) => kind.code().to_string(),
            DiagnosticKind::Handler { namespace, code } => match code {
                Some(c) => format!("{namespace}.{c}"),
                None => format!("{namespace}.diagnostic"),
            },
            DiagnosticKind::ForbiddenLabelPrefix => "forbidden-label-prefix".to_string(),
            DiagnosticKind::UnknownLexCanonical => "unknown-lex-canonical".to_string(),
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
/// Diagnostics whose code is unknown to the rules config (extension-
/// emitted codes from handlers, until the `extra` map ships) are
/// passed through untouched at their intrinsic severity.
pub fn apply_rules(diagnostics: &mut Vec<AnalysisDiagnostic>, rules: &DiagnosticsRulesConfig) {
    diagnostics.retain_mut(|diag| {
        let code = diag.kind.code();
        let Some(rule) = rules.lookup_by_code(&code) else {
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
    apply_rules(&mut diagnostics, rules);
    diagnostics
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
    use lex_core::lex::parsing::process_full_permissive;
    use lex_core::lex::testing::lexplore::Lexplore;

    fn footnote_diags(doc: &Document) -> Vec<AnalysisDiagnostic> {
        analyze(doc)
            .into_iter()
            .filter(|d| d.kind == DiagnosticKind::MissingFootnoteDefinition)
            .collect()
    }

    fn label_diags(source: &str) -> Vec<AnalysisDiagnostic> {
        let doc = process_full_permissive(source).expect("permissive parse");
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
    fn apply_rules_matches_extension_code_via_extra() {
        // End-to-end: handler emits `acme.foo`, user configured
        // `"acme.foo" = "allow"` in `[diagnostics.rules]` (landing in
        // `extra`); diagnostic gets dropped.
        let mut diags = vec![dummy_diag(
            DiagnosticKind::Handler {
                namespace: "acme".into(),
                code: Some("foo".into()),
            },
            DiagnosticSeverity::Error,
        )];
        let rules = DiagnosticsRulesConfig {
            extra: [(
                "acme.foo".to_string(),
                lex_config::RuleConfig::Bare(Severity::Allow),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        apply_rules(&mut diags, &rules);
        assert!(diags.is_empty(), "allow drops the extension diagnostic");

        // `warn` keeps the intrinsic severity (Error stays Error).
        let mut diags = vec![dummy_diag(
            DiagnosticKind::Handler {
                namespace: "acme".into(),
                code: Some("foo".into()),
            },
            DiagnosticSeverity::Error,
        )];
        let rules = DiagnosticsRulesConfig {
            extra: [(
                "acme.foo".to_string(),
                lex_config::RuleConfig::Bare(Severity::Warn),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        apply_rules(&mut diags, &rules);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            DiagnosticSeverity::Error,
            "warn preserves the handler's intrinsic severity"
        );

        // `deny` is a no-op when the intrinsic is already Error, but
        // still keeps the diagnostic — symmetry with built-ins.
        let mut diags = vec![dummy_diag(
            DiagnosticKind::Handler {
                namespace: "acme".into(),
                code: Some("foo".into()),
            },
            DiagnosticSeverity::Error,
        )];
        let rules = DiagnosticsRulesConfig {
            extra: [(
                "acme.foo".to_string(),
                lex_config::RuleConfig::Bare(Severity::Deny),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        apply_rules(&mut diags, &rules);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);

        // A configured rule whose code doesn't match the emitted one
        // passes the diagnostic through untouched.
        let mut diags = vec![dummy_diag(
            DiagnosticKind::Handler {
                namespace: "acme".into(),
                code: Some("foo".into()),
            },
            DiagnosticSeverity::Warning,
        )];
        let rules = DiagnosticsRulesConfig {
            extra: [(
                "acme.other".to_string(),
                lex_config::RuleConfig::Bare(Severity::Allow),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        apply_rules(&mut diags, &rules);
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
        apply_rules(&mut diags, &rules);
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
        apply_rules(&mut diags, &rules);
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
        apply_rules(&mut diags, &rules);
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
        apply_rules(&mut diags, &rules);
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
        apply_rules(&mut diags, &rules);
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].kind, DiagnosticKind::UnusedFootnoteDefinition);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
        assert_eq!(diags[1].kind, DiagnosticKind::TableInconsistentColumns);
        assert_eq!(diags[1].severity, DiagnosticSeverity::Error);
    }
}
