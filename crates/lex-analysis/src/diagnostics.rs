use crate::inline::extract_references;
use crate::utils::for_each_text_content;
use lex_core::lex::ast::{ContentItem, Document, Range, Session, Table};
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
    // 1. Collect all footnote references (both numbered and labeled)
    let mut numbered_refs = Vec::new();
    let mut labeled_refs = Vec::new();
    for_each_text_content(document, &mut |text| {
        for reference in extract_references(text) {
            match &reference.reference_type {
                ReferenceType::FootnoteNumber { number } => {
                    numbered_refs.push((*number, reference.range));
                }
                ReferenceType::AnnotationReference { label } => {
                    labeled_refs.push((label.clone(), reference.range));
                }
                _ => {}
            }
        }
    });

    // 2. Collect all footnote definitions (annotations and list items)
    let definitions_list = crate::utils::collect_footnote_definitions(document);
    let mut numeric_definitions = std::collections::HashSet::new();
    let mut label_definitions = std::collections::HashSet::new();

    for (label, _) in &definitions_list {
        label_definitions.insert(label.to_lowercase());
        if let Ok(number) = label.parse::<u32>() {
            numeric_definitions.insert(number);
        }
    }

    // 3. Check for missing definitions (numbered)
    for (number, range) in &numbered_refs {
        if !numeric_definitions.contains(number) {
            diagnostics.push(AnalysisDiagnostic {
                range: range.clone(),
                kind: DiagnosticKind::MissingFootnoteDefinition,
                message: format!("Footnote [{number}] is referenced but not defined"),
            });
        }
    }

    // 4. Check for missing definitions (labeled)
    for (label, range) in &labeled_refs {
        if !label_definitions.contains(&label.to_lowercase()) {
            diagnostics.push(AnalysisDiagnostic {
                range: range.clone(),
                kind: DiagnosticKind::MissingFootnoteDefinition,
                message: format!("Footnote [^{label}] is referenced but not defined"),
            });
        }
    }

    // Note: Unused definitions (footnotes without references) are intentionally not flagged
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
/// The effective width of a row is the sum of colspans across its cells.
/// Rows with different effective widths indicate a structural error (missing
/// or extra cells).
fn check_table_columns(table: &Table, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    let rows: Vec<_> = table.all_rows().collect();
    if rows.len() < 2 {
        return;
    }

    // Compute effective width per row (sum of colspans)
    let widths: Vec<usize> = rows
        .iter()
        .map(|row| row.cells.iter().map(|c| c.colspan).sum())
        .collect();

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
    fn ignores_valid_footnote() {
        let doc = parse("Text [1].\n\n:: 1 ::\nNote.\n::\n");
        let diags = analyze(&doc);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn ignores_valid_list_footnote() {
        // "Notes" session with indented list item "1."
        let doc = parse("Text [1].\n\nNotes\n\n    1. Note.\n    2. Another.\n");
        let diags = analyze(&doc);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn detects_missing_labeled_footnote_definition() {
        let doc = parse("Text with [^source] reference.");
        let diags = analyze(&doc);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::MissingFootnoteDefinition);
        assert!(diags[0].message.contains("[^source]"));
    }

    #[test]
    fn ignores_valid_labeled_footnote() {
        let doc = parse("Text [^source].\n\n:: source :: The source material.\n");
        let diags = analyze(&doc);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn labeled_footnote_match_is_case_insensitive() {
        let doc = parse("Text [^Source].\n\n:: source :: The source material.\n");
        let diags = analyze(&doc);
        assert_eq!(diags.len(), 0);
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
