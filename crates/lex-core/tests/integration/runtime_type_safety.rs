// Runtime type safety tests for container validation
use lex_core::lex::ast::{Annotation, ContentItem, Definition, Label, Session};

#[test]
#[should_panic(expected = "GeneralPolicy does not allow Sessions")]
fn test_annotation_rejects_session_at_runtime() {
    let mut annotation = Annotation::marker(Label::new("note".to_string()));
    let session = Session::with_title("Invalid Nested Session".to_string());

    // This should panic due to runtime validation
    annotation.children.push(ContentItem::Session(session));
}

#[test]
#[should_panic(expected = "GeneralPolicy does not allow Sessions")]
fn test_definition_rejects_session_at_runtime() {
    let mut definition = Definition::with_subject("Term".to_string());
    let session = Session::with_title("Invalid Nested Session".to_string());

    // This should panic due to runtime validation
    definition.children.push(ContentItem::Session(session));
}

#[test]
fn test_session_allows_session() {
    let mut session = Session::with_title("Outer".to_string());
    let inner_session = Session::with_title("Inner".to_string());

    // This should work fine - SessionPolicy allows Sessions
    session.children.push(ContentItem::Session(inner_session));
    assert_eq!(session.children.len(), 1);
}

#[test]
fn test_push_typed_is_type_safe() {
    use lex_core::lex::ast::elements::typed_content::ContentElement;
    use lex_core::lex::ast::Paragraph;

    let mut annotation = Annotation::marker(Label::new("note".to_string()));
    let para = Paragraph::from_line("Valid content".to_string());

    // push_typed() should work with the correct type
    annotation
        .children
        .push_typed(ContentElement::Paragraph(para));
    assert_eq!(annotation.children.len(), 1);
}
