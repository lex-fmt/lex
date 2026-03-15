use lex_babel::format::Format;
use lex_babel::formats::markdown::MarkdownFormat;
use lex_babel::ir::nodes::{DocNode, TableCellAlignment};
use lex_core::lex::transforms::standard::STRING_TO_AST;

#[test]
fn test_table_round_trip() {
    let md = r#"| Header 1 | Header 2 |
| :--- | :---: |
| Cell 1 | Cell 2 |
| Cell 3 | Cell 4 |
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");
    let output = MarkdownFormat
        .serialize(&doc)
        .expect("Failed to serialize markdown");

    assert!(output.contains("| Header 1 | Header 2 |"));
    assert!(output.contains("Cell 1"));
    assert!(output.contains("Cell 2"));
    assert!(output.contains(":--"));
    assert!(output.contains(":-:"));
}

#[test]
fn test_table_alignment_import() {
    let md = r#"| Left | Center | Right |
| :--- | :----: | ----: |
| L    | C      | R     |
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");
    let ir_doc = lex_babel::to_ir(&doc);

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

    let row = &table.rows[0];
    assert_eq!(row.cells.len(), 3);
    assert_eq!(row.cells[0].align, TableCellAlignment::Left);
    assert_eq!(row.cells[1].align, TableCellAlignment::Center);
    assert_eq!(row.cells[2].align, TableCellAlignment::Right);
}

#[test]
fn test_table_markdown_caption() {
    let lex_src = "Results:\n    | A | B |\n    | 1 | 2 |\n:: table ::\n";
    let doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    let md = MarkdownFormat
        .serialize(&doc)
        .expect("Failed to serialize markdown");

    // Caption should appear as bold text before the table
    assert!(
        md.contains("**Results**"),
        "Caption should render as bold text. Got:\n{md}"
    );
    assert!(md.contains("| A | B |"), "Table content should be present");
}

#[test]
fn test_table_markdown_colspan_defaults_to_1() {
    // Markdown doesn't support colspan, so all cells have colspan=1 on import
    let md = "| A | B |\n| --- | --- |\n| 1 | 2 |\n";

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");
    let ir_doc = lex_babel::to_ir(&doc);

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

    for row in table.header.iter().chain(table.rows.iter()) {
        for cell in &row.cells {
            assert_eq!(cell.colspan, 1);
            assert_eq!(cell.rowspan, 1);
        }
    }
}
