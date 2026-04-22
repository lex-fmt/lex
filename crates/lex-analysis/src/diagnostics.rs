use crate::inline::extract_references;
use crate::utils::for_each_text_content;
use lex_core::lex::ast::{ContentItem, Document, Range, Session, Table, TableRow};
use lex_core::lex::inlines::ReferenceType;

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
    // 1. Collect all numbered footnote references
    let mut numbered_refs = Vec::new();
    for_each_text_content(document, &mut |text| {
        for reference in extract_references(text) {
            if let ReferenceType::FootnoteNumber { number } = reference.reference_type {
                numbered_refs.push((number, reference.range));
            }
        }
    });

    // 2. Collect footnote definitions from :: notes ::-annotated lists
    let definitions_list = crate::utils::collect_footnote_definitions(document);
    let mut numeric_definitions = std::collections::HashSet::new();
    for (label, _) in &definitions_list {
        if let Ok(number) = label.parse::<u32>() {
            numeric_definitions.insert(number);
        }
    }

    // 3. Check for missing definitions
    for (number, range) in &numbered_refs {
        if !numeric_definitions.contains(number) {
            diagnostics.push(AnalysisDiagnostic {
                range: range.clone(),
                kind: DiagnosticKind::MissingFootnoteDefinition,
                message: format!("Footnote [{number}] has no matching item in a :: notes :: list"),
            });
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
    use lex_core::lex::parsing;

    fn parse(source: &str) -> Document {
        parsing::parse_document(source).expect("parse failed")
    }

    #[test]
    fn detects_missing_footnote_definition() {
        let doc = parse("Text with [1] reference.");
        let diags = analyze(&doc);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::MissingFootnoteDefinition);
    }

    #[test]
    fn ignores_valid_footnote_with_notes_annotation() {
        // :: notes :: annotated list provides the definitions
        let doc = parse("Text [1].\n\n:: notes ::\n1. Note.\n2. Another.\n");
        let diags = analyze(&doc);
        let footnote_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::MissingFootnoteDefinition)
            .collect();
        assert!(footnote_diags.is_empty());
    }

    #[test]
    fn ignores_valid_list_footnote_in_session() {
        // :: notes :: inside a session
        let doc = parse("Text [1].\n\nNotes\n\n    :: notes ::\n\n    1. Note.\n    2. Another.\n");
        let diags = analyze(&doc);
        let footnote_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::MissingFootnoteDefinition)
            .collect();
        assert!(footnote_diags.is_empty());
    }

    #[test]
    fn list_without_notes_annotation_is_not_footnotes() {
        // A "Notes" session without :: notes :: does NOT define footnotes
        let doc = parse("Text [1].\n\nNotes\n\n    1. Note.\n    2. Another.\n");
        let diags = analyze(&doc);
        let footnote_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::MissingFootnoteDefinition)
            .collect();
        assert_eq!(footnote_diags.len(), 1);
    }

    #[test]
    fn detects_inconsistent_table_columns() {
        let doc = parse("Data:\n    | A | B | C |\n    | 1 | 2 |\n:: table ::\n");
        let diags = analyze(&doc);
        let table_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::TableInconsistentColumns)
            .collect();
        assert_eq!(table_diags.len(), 1);
        assert!(table_diags[0].message.contains("2 columns"));
        assert!(table_diags[0].message.contains("expected 3"));
    }

    #[test]
    fn consistent_table_no_diagnostic() {
        let doc = parse("Data:\n    | A | B |\n    | 1 | 2 |\n:: table ::\n");
        let diags = analyze(&doc);
        let table_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::TableInconsistentColumns)
            .collect();
        assert!(table_diags.is_empty());
    }

    #[test]
    fn table_with_rowspan_counts_carry_over() {
        // Row 0: A | B | C           → 3 cells, widths all 1 → effective width 3
        // Row 1: D | ^^ | E          → ^^ is absorbed into B (B gets rowspan=2),
        //                              leaving row 1 with 2 cells [D, E]. But the
        //                              column occupied by B's rowspan means row 1's
        //                              effective width is still 3.
        let doc = parse("Data:\n    | A | B  | C |\n    | D | ^^ | E |\n:: table ::\n");
        let diags = analyze(&doc);
        let table_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::TableInconsistentColumns)
            .collect();
        assert!(
            table_diags.is_empty(),
            "rowspan carry-over should not trigger inconsistent-columns, got: {table_diags:?}"
        );
    }

    #[test]
    fn table_with_colspan_and_rowspan_mixed() {
        // Mirrors the "Conference Schedule" pattern from benchmark/080-gentle-introduction.lex:
        //   | Time  | Room A          | Room B     |
        //   | 9:00  | Opening Keynote | >>         |   (Opening Keynote colspan=2)
        //   | 10:00 | Workshop        | Panel      |   (Workshop rowspan=2, via ^^ below)
        //   | 11:00 | ^^              | Discussion |
        let doc = parse(
            "Data:\n    | Time  | Room A          | Room B     |\n    | 9:00  | Opening Keynote | >>         |\n    | 10:00 | Workshop        | Panel      |\n    | 11:00 | ^^              | Discussion |\n:: table ::\n",
        );
        let diags = analyze(&doc);
        let table_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::TableInconsistentColumns)
            .collect();
        assert!(
            table_diags.is_empty(),
            "mixed colspan/rowspan should not trigger inconsistent-columns, got: {table_diags:?}"
        );
    }

    #[test]
    fn table_with_colspan_counts_effective_width() {
        // Row 1: A + >> = 2 effective columns (colspan=2)
        // Row 2: B + C = 2 columns
        // After merge resolution: row 1 has 1 cell (colspan=2), row 2 has 2 cells (colspan=1 each)
        // Effective widths: 2 and 2 — consistent
        let doc = parse("Data:\n    | A  | >> |\n    | B  | C  |\n:: table ::\n");
        let diags = analyze(&doc);
        let table_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::TableInconsistentColumns)
            .collect();
        assert!(table_diags.is_empty());
    }

    #[test]
    fn footnote_ref_in_table_cell_is_checked() {
        // Table cell contains [1] but no footnote definition exists
        let doc = parse("Data:\n    | Item  | Note |\n    | Alpha | [1]  |\n:: table ::\n");
        let diags = analyze(&doc);
        let footnote_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::MissingFootnoteDefinition)
            .collect();
        assert_eq!(footnote_diags.len(), 1);
        assert!(footnote_diags[0].message.contains("[1]"));
    }
}
