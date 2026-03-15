//! Property-based tests for table cell block content
//!
//! Tests that multi-line tables with block content parse correctly
//! and maintain structural invariants.

use lex_core::lex::ast::ContentItem;
use lex_core::lex::parsing::parse_document;
use proptest::prelude::*;

fn word() -> impl Strategy<Value = String> {
    "[A-Z][a-z]{2,8}"
}

fn list_cell() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(word(), 2..=4)
        .prop_map(|words| words.into_iter().map(|w| format!("| - {w:<13}|")).collect())
}

/// Generate a multi-line table with optional list content in cells
fn table_with_list_strategy() -> impl Strategy<Value = String> {
    (word(), word(), list_cell()).prop_map(|(subject, header, list_lines)| {
        let mut lines = Vec::new();
        lines.push(format!("{subject}:"));
        lines.push("    | Name | Items           |".to_string());
        lines.push(String::new()); // blank line for multi-line mode
        lines.push(format!("    | {:<4} {}", header, list_lines[0]));
        for line in &list_lines[1..] {
            lines.push(format!("    |      {line}"));
        }
        lines.push(String::new()); // blank line
        lines.push("    | Solo | Plain text      |".to_string());
        lines.push(":: table ::".to_string());
        lines.push(String::new());
        lines.join("\n")
    })
}

/// Generate a simple multi-line table with paragraph content
fn table_with_paragraphs_strategy() -> impl Strategy<Value = String> {
    (word(), word(), word()).prop_map(|(subject, w1, w2)| {
        format!(
            "{subject}:\n    | Key | Value |\n\n    | {w1} | Line one. |\n    |     | Line two. |\n\n    | {w2} | Single.   |\n:: table ::\n"
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn table_with_list_parses_successfully(source in table_with_list_strategy()) {
        let result = parse_document(&source);
        prop_assert!(result.is_ok(), "Failed to parse: {:?}\nSource:\n{}", result.err(), source);
    }

    #[test]
    fn table_with_paragraphs_parses_successfully(source in table_with_paragraphs_strategy()) {
        let result = parse_document(&source);
        prop_assert!(result.is_ok(), "Failed to parse: {:?}\nSource:\n{}", result.err(), source);
    }

    #[test]
    fn table_with_list_has_block_content(source in table_with_list_strategy()) {
        let doc = parse_document(&source).unwrap();
        let items: Vec<&ContentItem> = doc.root.children.iter()
            .filter(|i| !matches!(i, ContentItem::BlankLineGroup(_)))
            .collect();

        // Should have at least one table
        let has_table = items.iter().any(|i| matches!(i, ContentItem::Table(_)));
        prop_assert!(has_table, "No table found in parsed document\nSource:\n{}", source);

        // The first body row's second cell should have block content (list)
        for item in &items {
            if let ContentItem::Table(t) = item {
                if let Some(row) = t.body_rows.first() {
                    if row.cells.len() > 1 {
                        let cell = &row.cells[1];
                        prop_assert!(
                            cell.has_block_content(),
                            "Expected block content in first body row cell 1\nSource:\n{}",
                            source
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn no_sessions_in_cell_children(source in table_with_list_strategy()) {
        let doc = parse_document(&source).unwrap();
        for item in doc.root.children.iter() {
            if let ContentItem::Table(t) = item {
                for row in t.all_rows() {
                    for cell in &row.cells {
                        for child in cell.children.iter() {
                            prop_assert!(
                                !matches!(child, ContentItem::Session(_)),
                                "Session found in cell children (should be impossible)"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn parse_is_deterministic_for_tables(source in table_with_paragraphs_strategy()) {
        let doc1 = parse_document(&source).unwrap();
        let doc2 = parse_document(&source).unwrap();

        let count1 = doc1.root.children.iter().count();
        let count2 = doc2.root.children.iter().count();
        prop_assert_eq!(count1, count2, "Different child counts on same source");
    }
}
