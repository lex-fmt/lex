//! Table column-consistency diagnostics.
//!
//! Walks every table in the document and flags rows whose effective column
//! count (accounting for colspan and rowspan carry-over) differs from the
//! first row's. Has its own structural walk (`visit_tables_in_*`) rather than
//! sharing the footnote traversal — it carries no footnote-definition scope
//! and only needs to reach `Table` nodes.

use super::{AnalysisDiagnostic, DiagnosticKind, DiagnosticSeverity};
use lex_core::lex::ast::{ContentItem, Document, Session, Table, TableRow};

pub(super) fn check_tables(document: &Document, diagnostics: &mut Vec<AnalysisDiagnostic>) {
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
