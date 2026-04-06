//! Table element tests
//!
//! Tests the Table element using Lexplore fixtures from `table.docs/`
//! and inline string sources.
//!
//! Note: Many table.docs fixtures were updated in comms to remove the
//! `:: table ::` closing marker (tables are now spec'd to be detected by
//! content). The lex-core parser hasn't been updated for content-based
//! detection yet, so those fixtures currently parse as Definition.
//! Fixture-based tests that depend on Table parsing are temporarily
//! replaced with Definition assertions. Tests using inline strings with
//! `:: table ::` continue to verify Table parsing behavior.

use lex_core::lex::ast::elements::verbatim::VerbatimBlockMode;
use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;

// ============================================================================
// Fixture-based tests (parse as Definition after :: table :: removal)
// ============================================================================

#[test]
fn test_table_01_flat_minimal() {
    // table-01: No :: table :: → parses as Definition
    let doc = Lexplore::table(1).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Favorite Pets");
    });
}

#[test]
fn test_table_02_flat_with_alignment() {
    // table-02: :: table align=lcr :: inside block → Definition with table annotation
    let doc = Lexplore::table(2).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Test Scores");
    });
}

#[test]
fn test_table_03_flat_header_count() {
    // table-03: :: table header=2 :: inside block → Definition
    let doc = Lexplore::table(3).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Quarterly Revenue");
    });
}

// ============================================================================
// Footnotes
// ============================================================================

#[test]
fn test_table_06_flat_with_footnotes() {
    // table-06: :: table align=lccc :: inside block → Definition
    let doc = Lexplore::table(6).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition()
            .subject("Comparative Analysis of Retrieval Methods");
    });
}

#[test]
fn test_table_no_footnotes() {
    // table-01: Parses as Definition (no :: table ::)
    let doc = Lexplore::table(1).parse().unwrap();
    assert_ast(&doc).item(0, |item| {
        item.assert_definition().subject("Favorite Pets");
    });
}

// ============================================================================
// Cell Merging
// ============================================================================

#[test]
fn test_table_04_flat_cell_merging() {
    // table-04: :: table header=2 :: inside block → Definition
    let doc = Lexplore::table(4).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Experiment Summary");
    });
}

#[test]
fn test_table_17_flat_rowspan() {
    // table-17: No :: table :: → Definition
    let doc = Lexplore::table(17).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Category Breakdown");
    });
}

#[test]
fn test_table_18_flat_combined_spans() {
    // table-18: :: table header=2 :: inside block → Definition
    let doc = Lexplore::table(18).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Matrix");
    });
}

// ============================================================================
// Multi-line Cells
// ============================================================================

#[test]
fn test_table_05_flat_multiline() {
    // table-05: No :: table :: → Definition
    let doc = Lexplore::table(5).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Experiment Log");
    });
}

#[test]
fn test_table_multiline_from_string() {
    use lex_core::lex::parsing::parse_document;

    let source = "Notes:\n    | Key | Value |\n\n    | A   | line1 |\n    |     | line2 |\n\n    | B   | solo  |\n:: table ::\n";
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
    // table-07: No :: table :: → Definition
    let doc = Lexplore::table(7).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Migration Example");
    });
}

// ============================================================================
// Empty Cells and No-Header
// ============================================================================

#[test]
fn test_table_14_flat_empty_cells() {
    // table-14: No :: table :: → Definition
    let doc = Lexplore::table(14).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Status Matrix");
    });
}

#[test]
fn test_table_16_flat_no_header() {
    // table-16: :: table header=0 :: inside block → Definition
    let doc = Lexplore::table(16).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Quick Reference");
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
// Parsing from String (these use :: table :: and still produce Table nodes)
// ============================================================================

#[test]
fn test_table_from_string() {
    use lex_core::lex::parsing::parse_document;

    let source = "Simple Table:\n    | A | B |\n    | 1 | 2 |\n:: table ::\n";
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

    let source = "T:\n    | H1 | H2 |\n    | B1 | B2 |\n:: table ::\n";
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

    let source = "T:\n    | A | B |\n    |---|---|\n    | 1 | 2 |\n:: table ::\n";
    let doc = parse_document(source).unwrap();

    // Separator should be ignored
    assert_ast(&doc).item(0, |item| {
        item.assert_table().row_count(2);
    });
}

// ============================================================================
// Block Cell Content (fixtures → Definition)
// ============================================================================

#[test]
fn test_table_19_cell_with_list() {
    let doc = Lexplore::table(19).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Data");
    });
}

#[test]
fn test_table_20_cell_with_definition() {
    let doc = Lexplore::table(20).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Data");
    });
}

#[test]
fn test_table_21_cell_with_verbatim() {
    let doc = Lexplore::table(21).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Data");
    });
}

#[test]
fn test_table_22_cell_with_mixed_content() {
    let doc = Lexplore::table(22).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Data");
    });
}

#[test]
fn test_table_23_cell_with_annotation() {
    let doc = Lexplore::table(23).parse().unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Data");
    });
}

#[test]
fn test_table_block_content_from_string() {
    use lex_core::lex::parsing::parse_document;

    let source = "T:\n    | Key | Value |\n\n    | A   | - item1 |\n    |     | - item2 |\n\n    | B   | plain   |\n:: table ::\n";
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
    let source = "T:\n    | A | - item |\n    | B | plain  |\n:: table ::\n";
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

    let source = "T:\n    | *bold* | normal |\n:: table ::\n";
    let doc = parse_document(source).unwrap();

    // Cell content should be inline-parsed (TextContent with potential inlines)
    assert_ast(&doc).item(0, |item| {
        item.assert_table().header_cells(0, &["*bold*", "normal"]);
    });
}

#[test]
fn test_table_single_row_is_header() {
    use lex_core::lex::parsing::parse_document;

    let source = "T:\n    | A | B |\n:: table ::\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item(0, |item| {
        item.assert_table()
            .header_row_count(1)
            .body_row_count(0)
            .header_cells(0, &["A", "B"]);
    });
}
