//! Footnote-definition diagnostics (always-on).
//!
//! Walks the document for footnote references (`[1]`) and flags any whose
//! number resolves to no definition in scope. Definitions come from
//! `:: notes ::`-annotated lists at document or session scope; references
//! inside a table additionally resolve against that table's own positional
//! footnote list before falling back to the outer scope.
//!
//! The traversal helpers (`check_session` / `check_content` /
//! `check_annotation` / `check_table`) live here rather than in a shared
//! module because they are footnote-specific: each threads the in-scope
//! definition set (`defs: &HashSet<u32>`) and exists only to reach the
//! leaf [`check_text`]. The label and table-column passes have their own
//! independent walkers, so there is no cross-category traversal to share.

use super::{AnalysisDiagnostic, DiagnosticKind, DiagnosticSeverity};
use crate::inline::extract_references;
use lex_core::lex::ast::{Annotation, ContentItem, Document, Session, Table, TextContent};
use lex_core::lex::inlines::ReferenceType;
use std::collections::HashSet;

pub(super) fn check_footnotes(document: &Document, diagnostics: &mut Vec<AnalysisDiagnostic>) {
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
