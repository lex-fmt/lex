use lex_babel::format::Format;
use lex_babel::formats::html::HtmlFormat;
use lex_babel::formats::markdown::MarkdownFormat;
use lex_babel::ir::nodes::*;
use lex_core::lex::transforms::standard::STRING_TO_AST;

#[test]
fn test_table_html_export() {
    let md = r#"| Header 1 | Header 2 |
| :--- | :---: |
| Cell 1 | Cell 2 |
"#;

    // Markdown -> Lex
    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");

    // Lex -> HTML
    let html = HtmlFormat::default()
        .serialize(&doc)
        .expect("Failed to serialize html");

    assert!(html.contains("<table class=\"lex-table\">"));
    assert!(html.contains("<tr>"));
    assert!(html.contains("Header 1"));
    assert!(html.contains("text-align: center"));
    assert!(html.contains("Cell 2"));
}

#[test]
fn test_table_html_caption() {
    let lex_src = "Results:\n    | A | B |\n    | 1 | 2 |\n:: table ::\n";
    let doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    let html = HtmlFormat::default()
        .serialize(&doc)
        .expect("Failed to serialize html");

    assert!(html.contains("<caption>"), "Should have caption element");
    assert!(
        html.contains("Results"),
        "Caption should contain subject text"
    );
}

#[test]
fn test_table_html_colspan() {
    let lex_src =
        "Spans:\n    | Wide   | >>     | Normal |\n    | A      | B      | C      |\n:: table ::\n";
    let doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    let html = HtmlFormat::default()
        .serialize(&doc)
        .expect("Failed to serialize html");

    assert!(
        html.contains("colspan=\"2\""),
        "Should have colspan=2 for merged cell"
    );
}

#[test]
fn test_table_html_rowspan() {
    let lex_src =
        "Spans:\n    | Key | Value |\n    | A   | 1     |\n    | ^^  | 2     |\n:: table header=0 ::\n";
    let doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    let html = HtmlFormat::default()
        .serialize(&doc)
        .expect("Failed to serialize html");

    assert!(
        html.contains("rowspan=\"2\""),
        "Should have rowspan=2 for merged cell"
    );
}

#[test]
fn test_table_html_footnotes() {
    let lex_src =
        "Notes:\n    | Item  | Cost [1] |\n    | Alpha | 100      |\n\n    1. All prices in USD.\n:: table ::\n";
    let doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    let html = HtmlFormat::default()
        .serialize(&doc)
        .expect("Failed to serialize html");

    assert!(
        html.contains("lex-table-footnotes"),
        "Should have footnotes section"
    );
    assert!(html.contains("USD"), "Footnote content should be in output");
}

#[test]
fn test_table_ir_preserves_colspan_rowspan() {
    let lex_src = "Data:\n    | Wide   | >>     |\n    | A      | B      |\n:: table ::\n";
    let doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    let ir = lex_babel::to_ir(&doc);

    let table = ir.children.iter().find_map(|n| {
        if let DocNode::Table(t) = n {
            Some(t)
        } else {
            None
        }
    });

    let table = table.expect("Should have a table in IR");

    // Header row should have a cell with colspan=2
    assert!(!table.header.is_empty(), "Should have header rows");
    let header_row = &table.header[0];

    // After merge resolution, the merged cell should have colspan=2
    let has_colspan = header_row.cells.iter().any(|c| c.colspan == 2);
    assert!(has_colspan, "Should have cell with colspan=2");
}

#[test]
fn test_table_ir_preserves_caption() {
    let lex_src = "My Table:\n    | A | B |\n    | 1 | 2 |\n:: table ::\n";
    let doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    let ir = lex_babel::to_ir(&doc);

    let table = ir.children.iter().find_map(|n| {
        if let DocNode::Table(t) = n {
            Some(t)
        } else {
            None
        }
    });

    let table = table.expect("Should have a table in IR");
    assert!(table.caption.is_some(), "Should preserve caption");
    let caption = table.caption.as_ref().unwrap();
    let text: String = caption
        .iter()
        .filter_map(|i| {
            if let InlineContent::Text(t) = i {
                Some(t.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        text.contains("My Table"),
        "Caption should contain 'My Table'"
    );
}

#[test]
fn test_table_ir_preserves_footnotes() {
    let lex_src =
        "Notes:\n    | Item | Cost [1] |\n    | X    | 50       |\n\n    1. In EUR.\n:: table ::\n";
    let doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    let ir = lex_babel::to_ir(&doc);

    let table = ir.children.iter().find_map(|n| {
        if let DocNode::Table(t) = n {
            Some(t)
        } else {
            None
        }
    });

    let table = table.expect("Should have a table in IR");
    assert!(!table.footnotes.is_empty(), "Should preserve footnotes");
}
