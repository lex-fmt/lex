use crate::inline::extract_references;
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
    /// handler was dispatched. The variant carries which of the six
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

/// One of the six schema pre-validation checks the analyser owns
/// before dispatching to a handler. Wire spec / proposal §13.2.
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
    use lex_core::lex::ast::{Label, Verbatim};

    fn emit(label: &Label, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        if let Resolution::Rejected(reason) = classify_label(&label.value) {
            let (kind, message) = match reason {
                RejectReason::Forbidden { input } => (
                    DiagnosticKind::ForbiddenLabelPrefix,
                    format!(
                        "label `{input}` uses the reserved `doc.*` prefix (forbidden under \
                         namespace policy; see general.lex §4.1)"
                    ),
                ),
                RejectReason::UnknownCanonical { input } => (
                    DiagnosticKind::UnknownLexCanonical,
                    format!("label `{input}` is not a registered `lex.*` canonical"),
                ),
            };
            diagnostics.push(AnalysisDiagnostic {
                range: label.location.clone(),
                severity: DiagnosticSeverity::Error,
                kind,
                message,
            });
        }
    }

    fn walk_annotation(annotation: &Annotation, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        emit(&annotation.data.label, diagnostics);
        for child in annotation.children.iter() {
            walk_item(child, diagnostics);
        }
    }

    fn walk_verbatim(verbatim: &Verbatim, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        emit(&verbatim.closing_data.label, diagnostics);
        for annotation in verbatim.annotations() {
            walk_annotation(annotation, diagnostics);
        }
    }

    fn walk_table(table: &Table, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        for annotation in table.annotations() {
            walk_annotation(annotation, diagnostics);
        }
        for row in table.header_rows.iter().chain(table.body_rows.iter()) {
            for cell in &row.cells {
                for child in cell.children.iter() {
                    walk_item(child, diagnostics);
                }
            }
        }
        if let Some(footnotes) = table.footnotes.as_ref() {
            for annotation in footnotes.annotations() {
                walk_annotation(annotation, diagnostics);
            }
            for item in footnotes.items.iter() {
                walk_item(item, diagnostics);
            }
        }
    }

    fn walk_item(item: &ContentItem, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        match item {
            ContentItem::Annotation(a) => walk_annotation(a, diagnostics),
            ContentItem::VerbatimBlock(v) => walk_verbatim(v, diagnostics),
            ContentItem::Table(t) => walk_table(t, diagnostics),
            _ => {}
        }
        // Walk attached annotations (sessions, paragraphs, lists, etc.).
        if let Some(attached) = attached_annotations(item) {
            for annotation in attached {
                walk_annotation(annotation, diagnostics);
            }
        }
        if let Some(children) = item.children() {
            for child in children {
                walk_item(child, diagnostics);
            }
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
}
