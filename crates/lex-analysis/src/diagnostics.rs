use crate::inline::extract_references;
use lex_core::lex::ast::{
    Annotation, ContentItem, Document, Range, Session, Table, TableRow, TextContent,
};
use lex_core::lex::inlines::ReferenceType;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    MissingFootnoteDefinition,
    UnusedFootnoteDefinition,
    TableInconsistentColumns,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisDiagnostic {
    pub range: Range,
    pub kind: DiagnosticKind,
    pub message: String,
}

pub fn analyze(document: &Document) -> Vec<AnalysisDiagnostic> {
    let mut diagnostics = Vec::new();
    check_footnotes(document, &mut diagnostics);
    check_tables(document, &mut diagnostics);
    diagnostics
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
    use lex_core::lex::testing::lexplore::Lexplore;

    fn footnote_diags(doc: &Document) -> Vec<AnalysisDiagnostic> {
        analyze(doc)
            .into_iter()
            .filter(|d| d.kind == DiagnosticKind::MissingFootnoteDefinition)
            .collect()
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
