//! End-to-end integration tests: tables with escaped pipes and backtick-protected
//! pipes parse into the expected cells through the full parsing pipeline.

use lex_core::lex::parsing::parse_document;
use lex_core::lex::testing::assert_ast;

#[test]
fn escaped_pipe_in_cell_preserves_content_as_literal() {
    let src = "Table test:\n    | feature  | example |\n    | pipe alt | a\\|b    |\n:: table ::\n";
    let doc = parse_document(src).expect("parse should succeed");
    assert_ast(&doc).item(0, |item| {
        item.assert_table()
            .header_row_count(1)
            .body_row_count(1)
            .column_count(2)
            .header_cells(0, &["feature", "example"])
            .body_row(0, |row| {
                row.cell_text(0, "pipe alt").cell_text(1, "a|b");
            });
    });
}

#[test]
fn backtick_literal_region_protects_pipes_in_cell() {
    let src = "Table test:\n    | code    | meaning          |\n    | `a|b`   | pipe inside code |\n    | `x|y|z` | three pipes      |\n:: table ::\n";
    let doc = parse_document(src).expect("parse should succeed");
    assert_ast(&doc).item(0, |item| {
        item.assert_table()
            .header_row_count(1)
            .body_row_count(2)
            .column_count(2)
            .header_cells(0, &["code", "meaning"])
            .body_row(0, |row| {
                row.cell_text(0, "`a|b`").cell_text(1, "pipe inside code");
            })
            .body_row(1, |row| {
                row.cell_text(0, "`x|y|z`").cell_text(1, "three pipes");
            });
    });
}

#[test]
fn escaped_pipe_mixed_with_backtick_region() {
    let src = "Mixed:\n    | plain | code  |\n    | a\\|b  | `c|d` |\n:: table ::\n";
    let doc = parse_document(src).expect("parse should succeed");
    assert_ast(&doc).item(0, |item| {
        item.assert_table()
            .header_row_count(1)
            .body_row_count(1)
            .column_count(2)
            .body_row(0, |row| {
                row.cell_text(0, "a|b").cell_text(1, "`c|d`");
            });
    });
}

#[test]
fn double_backslash_before_pipe_still_splits() {
    // `\\|` = literal backslash + structural pipe.
    let src = "Double bs:\n    | a\\\\ | b |\n:: table ::\n";
    let doc = parse_document(src).expect("parse should succeed");
    assert_ast(&doc).item(0, |item| {
        item.assert_table()
            .header_row_count(1)
            .column_count(2)
            .header_row(0, |row| {
                row.cell_text(0, "a\\\\").cell_text(1, "b");
            });
    });
}

#[test]
fn multiple_escaped_pipes_in_one_cell() {
    let src = "Many pipes:\n    | a\\|b\\|c\\|d | other |\n:: table ::\n";
    let doc = parse_document(src).expect("parse should succeed");
    assert_ast(&doc).item(0, |item| {
        item.assert_table()
            .header_row_count(1)
            .column_count(2)
            .header_row(0, |row| {
                row.cell_text(0, "a|b|c|d").cell_text(1, "other");
            });
    });
}

#[test]
fn backtick_in_cell_without_inner_pipe_unchanged() {
    // Sanity check: backtick-delimited content without internal pipes still works.
    let src = "Sanity:\n    | `code` | plain |\n:: table ::\n";
    let doc = parse_document(src).expect("parse should succeed");
    assert_ast(&doc).item(0, |item| {
        item.assert_table()
            .header_row_count(1)
            .column_count(2)
            .header_row(0, |row| {
                row.cell_text(0, "`code`").cell_text(1, "plain");
            });
    });
}
