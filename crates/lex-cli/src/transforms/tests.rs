use super::*;

#[test]
fn token_line_transform_emits_line_tokens() {
    let source = "Session:\n    Content\n";
    let extra_params = HashMap::new();
    let output =
        execute_transform(source, "token-line-json", &extra_params).expect("transform to run");

    assert!(output.contains("\"line_type\""));
    assert!(output.contains("SubjectLine"));
    assert!(output.contains("ParagraphLine"));
}

#[test]
fn token_simple_outputs_names() {
    let source = "Session:\n    Content\n";
    let extra_params = HashMap::new();
    let output =
        execute_transform(source, "token-simple", &extra_params).expect("transform to run");

    assert!(output.contains("TEXT"));
    assert!(output.contains("BLANK_LINE"));
}

#[test]
fn token_line_simple_outputs_names() {
    let source = "Session:\n    Content\n";
    let extra_params = HashMap::new();
    let output =
        execute_transform(source, "token-line-simple", &extra_params).expect("transform to run");

    assert!(output.contains("SUBJECT_LINE"));
    assert!(output.contains("PARAGRAPH_LINE"));
}

#[test]
fn token_pprint_inserts_blank_line() {
    let source = "Hello\n\nWorld\n";
    let extra_params = HashMap::new();
    let output =
        execute_transform(source, "token-pprint", &extra_params).expect("transform to run");

    assert!(output.contains("BLANK_LINE\n\n"));
}

#[test]
fn token_line_pprint_indents_children() {
    let source = "Session:\n    Content\n";
    let extra_params = HashMap::new();
    let output =
        execute_transform(source, "token-line-pprint", &extra_params).expect("transform to run");

    assert!(output.contains("SUBJECT_LINE"));
    assert!(output.contains("  PARAGRAPH_LINE"));
}

#[test]
fn execute_transform_accepts_extra_params() {
    let source = "# Test\n";
    let mut extra_params = HashMap::new();
    extra_params.insert("all-nodes".to_string(), "true".to_string());
    extra_params.insert("max-depth".to_string(), "5".to_string());

    // Should not error with unknown params
    let result = execute_transform(source, "ast-treeviz", &extra_params);
    assert!(result.is_ok());
}

#[test]
fn ast_full_param_includes_annotations() {
    use lex_babel::formats::treeviz::to_treeviz_str_with_params;
    use lex_core::lex::ast::elements::annotation::Annotation;
    use lex_core::lex::ast::elements::label::Label;
    use lex_core::lex::ast::elements::paragraph::Paragraph;
    use lex_core::lex::ast::elements::typed_content::ContentElement;
    use lex_core::lex::ast::{ContentItem, Document};

    // Create a document with document-level annotation programmatically
    let annotation = Annotation::new(
        Label::new("test-annotation".to_string()),
        vec![],
        Vec::<ContentElement>::new(),
    );
    let doc = Document::with_annotations_and_content(
        vec![annotation],
        vec![ContentItem::Paragraph(Paragraph::from_line(
            "Regular content".to_string(),
        ))],
    );

    let mut extra_params = HashMap::new();

    // Without ast-full, annotations should be excluded from output
    let output_normal = to_treeviz_str_with_params(&doc, &extra_params);
    assert!(
        !output_normal.contains("test-annotation"),
        "Annotation label should not be visible without ast-full"
    );

    // With ast-full=true, annotations should be included
    extra_params.insert("ast-full".to_string(), "true".to_string());
    let output_full = to_treeviz_str_with_params(&doc, &extra_params);
    // The annotation icon is " (double quote character)
    assert!(
        output_full.contains("\" test-annotation"),
        "With ast-full=true, annotation with icon should appear in output. Output was:\n{output_full}"
    );
    assert!(
        output_full.contains("test-annotation"),
        "Annotation label should be visible with ast-full"
    );
}
