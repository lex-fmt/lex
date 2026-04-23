//! Table element tests
//!
//! Tests the Table element using Lexplore fixtures from `table.docs/`.
//! Covers: basic tables, alignment, header count, cell merging (colspan/rowspan),
//! separator lines, fullwidth mode, empty cells, no-header mode, and combined spans.

use lex_core::lex::ast::elements::verbatim::VerbatimBlockMode;
use lex_core::lex::ast::TableCellAlignment;
use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;

// ============================================================================
// Basic Tables
// ============================================================================

#[test]
fn test_table_01_flat_minimal() {
    // table-01: Simple 3-row table with 2 columns, default header=1
    let doc = Lexplore::table(1).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Favorite Pets")
            .annotation_count(0)
            .mode(VerbatimBlockMode::Inflow)
            .header_row_count(1)
            .body_row_count(2)
            .column_count(2)
            .header_cells(0, &["Person", "Pet"])
            .body_cells(0, &["Bob", "Cats"])
            .body_cells(1, &["Alice", "Dogs"]);
    });
}

#[test]
fn test_table_02_flat_with_alignment() {
    // table-02: 4-row table with align=lcr parameter
    let doc = Lexplore::table(2).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Test Scores")
            .has_annotation_parameter_with_value("align", "lcr")
            .header_row_count(1)
            .body_row_count(3)
            .column_count(3)
            .header_row(0, |row| {
                row.cell_align(0, TableCellAlignment::Left)
                    .cell_align(1, TableCellAlignment::Center)
                    .cell_align(2, TableCellAlignment::Right);
            });
    });
}

#[test]
fn test_table_03_flat_header_count() {
    // table-03: header=2 with merge markers in header rows
    let doc = Lexplore::table(3).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Quarterly Revenue")
            .has_annotation_parameter_with_value("header", "2")
            .header_row_count(2)
            .body_row_count(2);
    });
}

// ============================================================================
// Footnotes
// ============================================================================

#[test]
fn test_table_06_flat_with_footnotes() {
    // table-06: Table with trailing numbered footnotes
    let doc = Lexplore::table(6).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Comparative Analysis of Retrieval Methods")
            .header_row_count(1)
            .body_row_count(3)
            .has_footnotes()
            .footnote_count(2)
            .footnote_text(0, "Precision measured at k=10")
            .footnote_text(1, "Weighted combination of BM25 and Dense");
    });
}

#[test]
fn test_table_no_footnotes() {
    // table-01: Simple table should have no footnotes
    let doc = Lexplore::table(1).parse().unwrap();

    assert_ast(&doc).item(0, |item| {
        item.assert_table().no_footnotes();
    });
}

// ============================================================================
// Cell Merging
// ============================================================================

#[test]
fn test_table_04_flat_cell_merging() {
    // table-04: Colspan (>>) merge markers in header
    let doc = Lexplore::table(4).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Experiment Summary")
            .header_row_count(2)
            .body_row_count(2)
            .header_row(0, |row| {
                // "Experiment Results | >> | Control" -> first cell has colspan=2
                row.cell_text(0, "Experiment Results")
                    .cell_colspan(0, 2)
                    .cell_text(1, "Control");
            });
    });
}

#[test]
fn test_table_17_flat_rowspan() {
    // table-17: Rowspan (^^) merge markers
    let doc = Lexplore::table(17).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Category Breakdown")
            .header_row_count(1)
            .body_row_count(3)
            .body_row(0, |row| {
                // "Group A" with ^^ below -> rowspan=2
                row.cell_text(0, "Group A").cell_rowspan(0, 2);
            });
    });
}

#[test]
fn test_table_18_flat_combined_spans() {
    // table-18: Both colspan (>>) and rowspan (^^) in one table
    let doc = Lexplore::table(18).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Matrix")
            .header_row_count(2)
            .body_row_count(2)
            .header_row(0, |row| {
                // "Q1 | >> | >> | Q2 | >> | >>" -> two cells with colspan=3 each
                row.cell_count(2)
                    .cell_text(0, "Q1")
                    .cell_colspan(0, 3)
                    .cell_text(1, "Q2")
                    .cell_colspan(1, 3);
            });
    });
}

// ============================================================================
// Multi-line Cells
// ============================================================================

#[test]
fn test_table_05_flat_multiline() {
    // table-05: Blank lines between pipe groups trigger multi-line mode.
    // Consecutive pipe lines within a group form a single row with joined cell content.
    let doc = Lexplore::table(5).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Experiment Log")
            .header_row_count(1)
            .body_row_count(2)
            .column_count(3)
            .header_cells(0, &["Trial", "Conditions", "Result"])
            .body_row(0, |row| {
                row.cell_text(0, "Trial 1")
                    .cell_text(1, "20°C, pH 7.2")
                    .cell_text(2, "Successful growth\nobserved after 48hrs.");
            })
            .body_row(1, |row| {
                row.cell_text(0, "Trial 2")
                    .cell_text(1, "25°C, pH 6.8")
                    .cell_text(2, "No growth detected.");
            });
    });
}

#[test]
fn test_table_multiline_from_string() {
    use lex_core::lex::parsing::parse_document;

    let source = "Notes:\n    | Key | Value |\n\n    | A   | line1 |\n    |     | line2 |\n\n    | B   | solo  |\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item(0, |item| {
        item.assert_table()
            .header_row_count(1)
            .body_row_count(2)
            .header_cells(0, &["Key", "Value"])
            .body_row(0, |row| {
                row.cell_text(0, "A").cell_text(1, "line1\nline2");
            })
            .body_row(1, |row| {
                row.cell_text(0, "B").cell_text(1, "solo");
            });
    });
}

// ============================================================================
// Separator Lines
// ============================================================================

#[test]
fn test_table_07_flat_separator_lines() {
    // table-07: Separator lines (|---|---|) should be skipped
    let doc = Lexplore::table(7).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Migration Example")
            .row_count(3) // 1 header + 2 body, separator line is skipped
            .header_cells(0, &["Name", "Score"])
            .body_cells(0, &["Alice", "95"])
            .body_cells(1, &["Bob", "87"]);
    });
}

// ============================================================================
// Empty Cells and No-Header
// ============================================================================

#[test]
fn test_table_14_flat_empty_cells() {
    // table-14: Some cells are empty
    let doc = Lexplore::table(14).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Status Matrix")
            .header_row_count(1)
            .body_row_count(3)
            .column_count(4)
            .body_row(0, |row| {
                // Review | (empty) | Done | (empty)
                row.cell_text(0, "Review")
                    .cell_text(1, "")
                    .cell_text(2, "Done");
            });
    });
}

#[test]
fn test_table_16_flat_no_header() {
    // table-16: header=0, all rows are body
    let doc = Lexplore::table(16).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Quick Reference")
            .has_annotation_parameter_with_value("header", "0")
            .header_row_count(0)
            .body_row_count(2)
            .body_cells(0, &["Alice", "95"])
            .body_cells(1, &["Bob", "87"]);
    });
}

// ============================================================================
// Fullwidth Mode
// ============================================================================

#[test]
fn test_table_11_fullwidth() {
    // table-11: Content starting at column 1 triggers fullwidth mode
    let doc = Lexplore::table(11).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Fullwidth Comparison Table")
            .mode(VerbatimBlockMode::Fullwidth)
            .header_row_count(1)
            .body_row_count(4)
            .column_count(4);
    });
}

// ============================================================================
// Parsing from String
// ============================================================================

#[test]
fn test_table_from_string() {
    use lex_core::lex::parsing::parse_document;

    let source = "Simple Table:\n    | A | B |\n    | 1 | 2 |\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Simple Table")
            .header_row_count(1)
            .body_row_count(1)
            .header_cells(0, &["A", "B"])
            .body_cells(0, &["1", "2"]);
    });
}

#[test]
fn test_table_header_cells_marked_as_header() {
    use lex_core::lex::parsing::parse_document;

    let source = "T:\n    | H1 | H2 |\n    | B1 | B2 |\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item(0, |item| {
        item.assert_table()
            .header_row(0, |row| {
                row.cell_is_header(0, true).cell_is_header(1, true);
            })
            .body_row(0, |row| {
                row.cell_is_header(0, false).cell_is_header(1, false);
            });
    });
}

#[test]
fn test_table_separator_only_dashes() {
    use lex_core::lex::parsing::parse_document;

    let source = "T:\n    | A | B |\n    |---|---|\n    | 1 | 2 |\n";
    let doc = parse_document(source).unwrap();

    // Separator should be ignored
    assert_ast(&doc).item(0, |item| {
        item.assert_table().row_count(2);
    });
}

// ============================================================================
// Block Cell Content
// ============================================================================

#[test]
fn test_table_19_cell_with_list() {
    let doc = Lexplore::table(19).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Data")
            .header_row_count(1)
            .body_row_count(2)
            .body_row(0, |row| {
                row.cell_text(0, "Alice")
                    .cell_has_block_content(0, false)
                    .cell_has_block_content(1, true)
                    .cell_child_count(1, 1); // One list
            })
            .body_row(1, |row| {
                row.cell_text(0, "Bob").cell_has_block_content(1, false);
            });
    });
}

#[test]
fn test_table_20_cell_with_definition() {
    let doc = Lexplore::table(20).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Data")
            .header_row_count(1)
            .body_row_count(2)
            .body_row(0, |row| {
                row.cell_text(0, "Cache").cell_has_block_content(1, true);
            })
            .body_row(1, |row| {
                row.cell_has_block_content(1, false);
            });
    });
}

#[test]
fn test_table_21_cell_with_verbatim() {
    let doc = Lexplore::table(21).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Data")
            .header_row_count(1)
            .body_row_count(2)
            .body_row(0, |row| {
                row.cell_text(0, "Python").cell_has_block_content(1, true);
            })
            .body_row(1, |row| {
                row.cell_has_block_content(1, false);
            });
    });
}

#[test]
fn test_table_22_cell_with_mixed_content() {
    let doc = Lexplore::table(22).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Data")
            .header_row_count(1)
            .body_row_count(2)
            .body_row(0, |row| {
                row.cell_text(0, "Alice").cell_has_block_content(1, true);
            })
            .body_row(1, |row| {
                row.cell_text(0, "Bob").cell_has_block_content(1, false);
            });
    });
}

#[test]
fn test_table_23_cell_with_annotation() {
    let doc = Lexplore::table(23).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_table()
            .subject("Data")
            .header_row_count(1)
            .body_row_count(2)
            .body_row(0, |row| {
                row.cell_text(0, "Alpha").cell_has_block_content(1, true);
            })
            .body_row(1, |row| {
                row.cell_has_block_content(1, false);
            });
    });
}

#[test]
fn test_table_block_content_from_string() {
    use lex_core::lex::parsing::parse_document;

    let source = "T:\n    | Key | Value |\n\n    | A   | - item1 |\n    |     | - item2 |\n\n    | B   | plain   |\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item(0, |item| {
        item.assert_table()
            .header_row_count(1)
            .body_row_count(2)
            .body_row(0, |row| {
                row.cell_text(0, "A")
                    .cell_has_block_content(1, true)
                    .cell_child_count(1, 1); // One list
            })
            .body_row(1, |row| {
                row.cell_text(0, "B").cell_has_block_content(1, false);
            });
    });
}

#[test]
fn test_table_compact_mode_no_block_content() {
    use lex_core::lex::parsing::parse_document;

    // Compact mode (no blank lines) should never produce block content
    let source = "T:\n    | A | - item |\n    | B | plain  |\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item(0, |item| {
        item.assert_table().body_row(0, |row| {
            row.cell_has_block_content(0, false)
                .cell_has_block_content(1, false);
        });
    });
}

#[test]
fn test_table_inline_parsing_of_cells() {
    use lex_core::lex::parsing::parse_document;

    let source = "T:\n    | *bold* | normal |\n";
    let doc = parse_document(source).unwrap();

    // Cell content should be inline-parsed (TextContent with potential inlines)
    assert_ast(&doc).item(0, |item| {
        item.assert_table().header_cells(0, &["*bold*", "normal"]);
    });
}
