use lex_babel::format::Format;
use lex_babel::formats::markdown::MarkdownFormat;

#[test]
fn test_annotation_round_trip() {
    let md = r#"
<!-- lex:note type=warning -->
This is a warning.
<!-- /lex:note -->
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");
    let output = MarkdownFormat
        .serialize(&doc)
        .expect("Failed to serialize markdown");

    println!("Output:\n{output}");

    assert!(output.contains("<!-- lex:note type=warning"));
    assert!(output.contains("This is a warning."));
    assert!(output.contains("-->"));
}

#[test]
fn test_nested_annotations() {
    let md = r#"
<!-- lex:outer -->
  <!-- lex:inner -->
  Nested content
  <!-- /lex:inner -->
<!-- /lex:outer -->
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");
    let output = MarkdownFormat
        .serialize(&doc)
        .expect("Failed to serialize markdown");

    assert!(output.contains("<!-- lex:outer -->"));
    assert!(output.contains("<!-- lex:inner -->"));
    assert!(output.contains("Nested content"));
    assert!(output.contains("<!-- /lex:inner -->"));
    assert!(output.contains("<!-- /lex:outer -->"));
}
