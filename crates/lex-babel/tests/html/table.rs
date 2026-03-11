use lex_babel::format::Format;
use lex_babel::formats::html::HtmlFormat;
use lex_babel::formats::markdown::MarkdownFormat;

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

    println!("HTML Output:\n{html}");

    assert!(html.contains("<table class=\"lex-table\">"));
    assert!(html.contains("<tr>"));
    assert!(html.contains("Header 1"));
    // Check alignment style
    // Note: Implementation details might vary (e.g. style attribute)
    assert!(html.contains("text-align: center"));
    assert!(html.contains("Cell 2"));
}
