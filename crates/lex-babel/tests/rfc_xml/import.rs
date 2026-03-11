use lex_babel::FormatRegistry;
use lex_core::lex::ast::ContentItem;

#[test]
fn test_rfc_xml_import_basic() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rfc version="3">
  <front>
    <title>Test RFC</title>
  </front>
  <middle>
    <section title="Introduction">
      <t>Hello World</t>
    </section>
  </middle>
</rfc>"#;

    let registry = FormatRegistry::with_defaults();
    let doc = registry.parse(xml, "rfc_xml").expect("Failed to parse");

    // Root -> Session (Test RFC) -> Session (Introduction) -> Paragraph (Hello World)
    let root_children = &doc.root.children;
    assert_eq!(root_children.len(), 1);

    if let ContentItem::Session(session) = &root_children[0] {
        assert!(session.title.as_string().contains("Test RFC"));
        let inner = &session.children;
        assert_eq!(inner.len(), 1);

        if let ContentItem::Session(subsession) = &inner[0] {
            assert!(subsession.title.as_string().contains("Introduction"));
            let content = &subsession.children;
            assert_eq!(content.len(), 1);

            if let ContentItem::Paragraph(para) = &content[0] {
                assert_eq!(para.text(), "Hello World");
            } else {
                panic!("Expected Paragraph, found {:?}", content[0]);
            }
        } else {
            panic!("Expected Sub-Session");
        }
    } else {
        panic!("Expected Session");
    }
}
