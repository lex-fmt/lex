use lex_core::lex::testing::lexplore::specfile_finder::{
    get_doc_root, list_files_by_number, DocumentType, ElementType,
};

#[test]
fn validate_unique_spec_numbers() {
    // Validate elements
    let element_types = [
        ElementType::Paragraph,
        ElementType::List,
        ElementType::Session,
        ElementType::Definition,
        ElementType::Annotation,
        ElementType::Verbatim,
    ];

    for elem_type in element_types {
        let dir = get_doc_root("elements", Some(elem_type.dir_name()));
        // This will panic if duplicates are found, which is what we want for a test
        let _ = list_files_by_number(&dir).unwrap();
    }

    // Validate document types
    let doc_types = [DocumentType::Benchmark, DocumentType::Trifecta];

    for doc_type in doc_types {
        let dir = get_doc_root(doc_type.dir_name(), None);
        // This will panic if duplicates are found
        let _ = list_files_by_number(&dir).unwrap();
    }
}
