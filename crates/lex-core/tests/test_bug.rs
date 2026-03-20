use lex_core::lex::parsing::parse_document;

#[test]
fn test_external_table_annotation_configures_header_count() {
    // External annotation before the table should configure it
    let source = ":: table header=2 ::\nSubject:\n    | 1 |\n    | 2 |\n    | 3 |\n";
    let doc = parse_document(source).unwrap();

    let table = doc
        .root
        .children
        .iter()
        .find_map(|i| {
            if let lex_core::lex::ast::ContentItem::Table(t) = i {
                Some(t)
            } else {
                None
            }
        })
        .expect("Table should be parsed");

    // Annotation should be attached
    assert_eq!(
        table.annotations.len(),
        1,
        "Annotation was not attached to the table"
    );

    // The table should have split the rows based on the annotation's 'header=2'
    assert_eq!(
        table.header_rows.len(),
        2,
        "Header rows were not split according to annotation"
    );
    assert_eq!(
        table.body_rows.len(),
        1,
        "Body rows were not split according to annotation"
    );
}
