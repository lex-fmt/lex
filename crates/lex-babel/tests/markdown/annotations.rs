use lex_babel::format::Format;
use lex_babel::formats::lex::LexFormat;
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

/// Issue #593: when markdown imports an annotation with a blessed
/// shortcut spelling (`<!-- lex:title ...`), the `markdown → lex`
/// conversion must emit the same shortcut spelling (`:: title :: ...`)
/// rather than the verbose canonical (`:: lex.metadata.title ::`).
/// The IR-level `LabelForm` propagation is what makes this work.
#[test]
fn markdown_to_lex_preserves_blessed_shortcut_form() {
    let md = "<!-- lex:title -->\nMy Doc\n<!-- /lex:title -->\n";

    let doc = MarkdownFormat.parse(md).expect("parse markdown");
    let lex_output = LexFormat::default().serialize(&doc).expect("serialize lex");

    // The serializer may emit either inline (`:: title :: My Doc`) or
    // block (`:: title\n    My Doc`) shape — both are acceptable; the
    // critical check is that the label is the blessed shortcut spelling
    // rather than the verbose canonical.
    assert!(
        lex_output.contains(":: title"),
        "expected blessed shortcut spelling, got:\n{lex_output}"
    );
    assert!(
        lex_output.contains("My Doc"),
        "annotation body lost in output:\n{lex_output}"
    );
    assert!(
        !lex_output.contains("lex.metadata.title"),
        "canonical form leaked into output:\n{lex_output}"
    );
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
