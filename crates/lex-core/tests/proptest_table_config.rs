//! Property-based tests for table configuration via annotations
//!
//! Tests that :: table :: annotations (both internal and external) correctly
//! configure header count and column alignment. Also tests robustness of the
//! table config pipeline against edge cases and invalid inputs.

use lex_core::lex::ast::elements::content_item::ContentItem;
use lex_core::lex::ast::TableCellAlignment;
use lex_core::lex::parsing::parse_document;
use proptest::prelude::*;

// =============================================================================
// Strategies
// =============================================================================

fn word() -> impl Strategy<Value = String> {
    "[A-Z][a-z]{2,8}"
}

fn subject() -> impl Strategy<Value = String> {
    word().prop_map(|w| format!("{w} Data"))
}

/// Generate a table with N rows (all pipe rows, no blank lines = compact mode)
fn table_rows(row_count: usize) -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(word(), row_count).prop_map(|words| {
        words
            .into_iter()
            .map(|w| format!("    | {w} | value |"))
            .collect()
    })
}

/// Build a table source string from subject, rows, and optional config params
fn build_table_source(subject: &str, rows: &[String], config: Option<&str>) -> String {
    let mut lines = vec![format!("{subject}:")];
    lines.extend(rows.iter().cloned());
    if let Some(cfg) = config {
        lines.push(String::new()); // blank line before annotation
        lines.push(format!("    :: table {cfg} ::"));
    }
    lines.push(String::new());
    lines.join("\n")
}

// =============================================================================
// 1. Header count: header=N splits correctly for any valid N
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn header_count_splits_correctly(
        subj in subject(),
        row_count in 2..8usize,
        header_count in 0..10usize,
    ) {
        let rows: Vec<String> = (0..row_count)
            .map(|i| format!("    | row{i} | val{i} |"))
            .collect();
        let source = build_table_source(&subj, &rows, Some(&format!("header={header_count}")));
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}"));

        let table = doc.root.children.iter().find_map(|i| {
            if let ContentItem::Table(t) = i { Some(t) } else { None }
        });
        prop_assert!(table.is_some(), "No table found");
        let table = table.unwrap();

        let expected_header = header_count.min(row_count);
        let expected_body = row_count - expected_header;

        prop_assert_eq!(
            table.header_rows.len(), expected_header,
            "header_rows mismatch"
        );
        prop_assert_eq!(
            table.body_rows.len(), expected_body,
            "body_rows mismatch"
        );

        // Header cells should be marked as header
        for row in &table.header_rows {
            for cell in &row.cells {
                prop_assert!(cell.header, "Header cell not marked as header");
            }
        }
        // Body cells should NOT be marked as header
        for row in &table.body_rows {
            for cell in &row.cells {
                prop_assert!(!cell.header, "Body cell marked as header");
            }
        }
    }
}

// =============================================================================
// 2. Alignment: align=XYZ applies correctly per column
// =============================================================================

fn alignment_string(col_count: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(prop_oneof![Just('l'), Just('c'), Just('r')], col_count)
        .prop_map(|chars| chars.into_iter().collect::<String>())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn alignment_applies_to_all_cells(
        subj in subject(),
        col_count in 2..6usize,
        align_str in alignment_string(4), // always 4 chars, may be fewer or more than cols
    ) {
        let row = (0..col_count)
            .map(|i| format!("c{i}"))
            .collect::<Vec<_>>()
            .join(" | ");
        let rows = vec![
            format!("    | {row} |"),
            format!("    | {row} |"),
        ];
        let source = build_table_source(&subj, &rows, Some(&format!("align={align_str}")));
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}"));

        let table = doc.root.children.iter().find_map(|i| {
            if let ContentItem::Table(t) = i { Some(t) } else { None }
        });
        prop_assert!(table.is_some(), "No table found");
        let table = table.unwrap();

        let expected: Vec<TableCellAlignment> = align_str.chars().map(|c| match c {
            'l' => TableCellAlignment::Left,
            'c' => TableCellAlignment::Center,
            'r' => TableCellAlignment::Right,
            _ => TableCellAlignment::None,
        }).collect();

        for row in table.all_rows() {
            for (i, cell) in row.cells.iter().enumerate() {
                if let Some(expected_align) = expected.get(i) {
                    prop_assert_eq!(
                        cell.align, *expected_align,
                        "Column alignment mismatch"
                    );
                }
                // Columns beyond align string length keep default (None)
            }
        }
    }
}

// =============================================================================
// 3. No annotation = defaults (header=1, no alignment)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn table_without_annotation_uses_defaults(
        subj in subject(),
        row_count in 2..6usize,
    ) {
        let rows: Vec<String> = (0..row_count)
            .map(|i| format!("    | row{i} | val{i} |"))
            .collect();
        let source = build_table_source(&subj, &rows, None);
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}"));

        let table = doc.root.children.iter().find_map(|i| {
            if let ContentItem::Table(t) = i { Some(t) } else { None }
        });
        prop_assert!(table.is_some(), "No table found");
        let table = table.unwrap();

        // Default: 1 header row
        prop_assert_eq!(table.header_rows.len(), 1, "Expected default 1 header row");
        prop_assert_eq!(table.body_rows.len(), row_count - 1);

        // Default: no alignment (None)
        for row in table.all_rows() {
            for cell in &row.cells {
                prop_assert_eq!(cell.align, TableCellAlignment::None, "Expected default None alignment");
            }
        }
    }
}

// =============================================================================
// 4. External annotation (before table) configures it
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn external_annotation_configures_table(
        subj in subject(),
        row_count in 3..7usize,
        header_count in 0..4usize,
    ) {
        let rows: Vec<String> = (0..row_count)
            .map(|i| format!("    | row{i} | val{i} |"))
            .collect();
        // External annotation before the table (not inside the block)
        let annotation = format!(":: table header={header_count} ::");
        let table_body = build_table_source(&subj, &rows, None);
        let source = format!("{annotation}\n{table_body}");

        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}"));

        let table = doc.root.children.iter().find_map(|i| {
            if let ContentItem::Table(t) = i { Some(t) } else { None }
        });
        prop_assert!(table.is_some(), "No table found");
        let table = table.unwrap();

        let expected_header = header_count.min(row_count);
        prop_assert_eq!(
            table.header_rows.len(), expected_header,
            "External annotation header split incorrect"
        );
    }
}

// =============================================================================
// 5. Robustness: invalid/random align strings don't panic
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn random_align_string_never_panics(
        subj in subject(),
        align_str in "[a-zA-Z0-9]{0,20}",
    ) {
        let rows = vec![
            "    | A | B | C |".to_string(),
            "    | 1 | 2 | 3 |".to_string(),
        ];
        let source = build_table_source(&subj, &rows, Some(&format!("align={align_str}")));
        let result = parse_document(&source);
        prop_assert!(result.is_ok(), "Parse failed on random align string");
    }

    #[test]
    fn random_header_value_never_panics(
        subj in subject(),
        header_val in "[a-zA-Z0-9_-]{0,10}",
    ) {
        let rows = vec![
            "    | A | B |".to_string(),
            "    | 1 | 2 |".to_string(),
        ];
        let source = build_table_source(&subj, &rows, Some(&format!("header={header_val}")));
        let result = parse_document(&source);
        prop_assert!(result.is_ok(), "Parse failed on random header value");
    }
}

// =============================================================================
// 6. Row count invariant: total rows = header + body (always)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn total_row_count_preserved(
        subj in subject(),
        row_count in 1..10usize,
        header_count in 0..15usize,
    ) {
        let rows: Vec<String> = (0..row_count)
            .map(|i| format!("    | row{i} | val{i} |"))
            .collect();
        let source = build_table_source(&subj, &rows, Some(&format!("header={header_count}")));
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}"));

        let table = doc.root.children.iter().find_map(|i| {
            if let ContentItem::Table(t) = i { Some(t) } else { None }
        });
        prop_assert!(table.is_some());
        let table = table.unwrap();

        prop_assert_eq!(
            table.header_rows.len() + table.body_rows.len(),
            row_count,
            "Total rows changed"
        );
    }
}

// =============================================================================
// 7. Blank lines interspersed: table still parses
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn blank_lines_in_table_dont_panic(
        subj in subject(),
        blanks_before in 0..3usize,
        blanks_between in 0..3usize,
    ) {
        let mut lines = vec![format!("{subj}:")];
        for _ in 0..blanks_before {
            lines.push(String::new());
        }
        lines.push("    | A | B |".to_string());
        for _ in 0..blanks_between {
            lines.push(String::new());
        }
        lines.push("    | 1 | 2 |".to_string());
        lines.push(String::new());

        let source = lines.join("\n");
        let result = parse_document(&source);
        prop_assert!(result.is_ok(), "Parse failed with blanks");
    }
}
