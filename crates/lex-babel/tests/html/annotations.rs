use lex_babel::format::Format;
use lex_babel::formats::html::HtmlFormat;
use lex_babel::formats::markdown::MarkdownFormat;

#[test]
fn test_annotation_html_export() {
    let md = r#"
<!-- lex:note type=warning -->
This is a warning.
<!-- /lex:note -->
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");
    let html = HtmlFormat::default()
        .serialize(&doc)
        .expect("Failed to serialize html");

    println!("HTML Output:\n{html}");

    // HTML export should preserve annotations as HTML comments
    assert!(html.contains("<!-- lex:note type=warning"));
    assert!(html.contains("This is a warning."));
    assert!(html.contains("-->"));
}
