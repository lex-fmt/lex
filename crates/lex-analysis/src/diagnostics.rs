use crate::inline::extract_references;
use crate::utils::for_each_text_content;
use lex_core::lex::ast::{Document, Range};
use lex_core::lex::inlines::ReferenceType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    MissingFootnoteDefinition,
    UnusedFootnoteDefinition,
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
    diagnostics
}

fn check_footnotes(document: &Document, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    // 1. Collect all footnote references
    let mut references = Vec::new();
    for_each_text_content(document, &mut |text| {
        for reference in extract_references(text) {
            if let ReferenceType::FootnoteNumber { number } = reference.reference_type {
                references.push((number, reference.range));
            }
        }
    });

    // 2. Collect all footnote definitions (annotations and list items)
    let definitions_list = crate::utils::collect_footnote_definitions(document);
    let mut definitions = std::collections::HashSet::new();

    for (label, _) in definitions_list {
        if let Ok(number) = label.parse::<u32>() {
            definitions.insert(number);
        }
    }

    // 3. Check for missing definitions
    for (number, range) in &references {
        if !definitions.contains(number) {
            diagnostics.push(AnalysisDiagnostic {
                range: range.clone(),
                kind: DiagnosticKind::MissingFootnoteDefinition,
                message: format!("Footnote [{number}] is referenced but not defined"),
            });
        }
    }

    // Note: Unused definitions (footnotes without references) are intentionally not flagged
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
        // "Notes" session with list item "1."
        let doc = parse("Text [1].\n\nNotes\n\n1. Note.\n");
        let diags = analyze(&doc);
        assert_eq!(diags.len(), 0);
    }
}
