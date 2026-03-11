use lex_babel::format::Format;
use lex_babel::formats::markdown::MarkdownFormat;

#[test]
fn test_table_round_trip() {
    let md = r#"| Header 1 | Header 2 |
| :--- | :---: |
| Cell 1 | Cell 2 |
| Cell 3 | Cell 4 |
"#;
    // Note: Comrak might normalize whitespace/alignment chars.

    // Markdown -> Lex
    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");

    // Lex -> Markdown
    let output = MarkdownFormat
        .serialize(&doc)
        .expect("Failed to serialize markdown");

    println!("Original:\n{md}");
    println!("Output:\n{output}");

    // Verify content presence
    assert!(output.contains("| Header 1 | Header 2 |"));
    assert!(output.contains("Cell 1"));
    assert!(output.contains("Cell 2"));

    // Verify alignment markers exist (Comrak output format)
    // Comrak usually outputs `| :--- | :---: |` or similar.
    assert!(output.contains(":--"));
    assert!(output.contains(":-:"));
}

#[test]
fn test_table_alignment_import() {
    use lex_babel::ir::nodes::{DocNode, TableCellAlignment};

    let md = r#"| Left | Center | Right |
| :--- | :----: | ----: |
| L    | C      | R     |
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");

    // Convert back to IR to verify structure (since Lex now uses doc.table verbatim)
    let ir_doc = lex_babel::to_ir(&doc);

    // Find the table node
    let table = ir_doc
        .children
        .iter()
        .find_map(|node| {
            if let DocNode::Table(t) = node {
                Some(t)
            } else {
                None
            }
        })
        .expect("Should have table");

    // Check first row of body
    let row = &table.rows[0];
    assert_eq!(row.cells.len(), 3);

    // Check alignments
    assert_eq!(row.cells[0].align, TableCellAlignment::Left);
    assert_eq!(row.cells[1].align, TableCellAlignment::Center);
    assert_eq!(row.cells[2].align, TableCellAlignment::Right);
}
