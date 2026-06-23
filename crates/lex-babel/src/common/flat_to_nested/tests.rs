use super::*;

#[test]
fn test_empty_document() {
    let events = vec![Event::StartDocument, Event::EndDocument];

    let doc = events_to_tree(&events).unwrap();
    assert_eq!(doc.children.len(), 0);
}

#[test]
fn test_simple_paragraph() {
    let events = vec![
        Event::StartDocument,
        Event::StartParagraph,
        Event::Inline(InlineContent::Text("Hello world".to_string())),
        Event::EndParagraph,
        Event::EndDocument,
    ];

    let doc = events_to_tree(&events).unwrap();
    assert_eq!(doc.children.len(), 1);

    match &doc.children[0] {
        DocNode::Paragraph(para) => {
            assert_eq!(para.content.len(), 1);
            assert!(matches!(&para.content[0], InlineContent::Text(t) if t == "Hello world"));
        }
        _ => panic!("Expected Paragraph"),
    }
}

#[test]
fn test_heading_with_content() {
    let events = vec![
        Event::StartDocument,
        Event::StartHeading(1),
        Event::Inline(InlineContent::Text("Title".to_string())),
        Event::EndHeading(1),
        Event::EndDocument,
    ];

    let doc = events_to_tree(&events).unwrap();
    assert_eq!(doc.children.len(), 1);

    match &doc.children[0] {
        DocNode::Heading(heading) => {
            assert_eq!(heading.level, 1);
            assert_eq!(heading.content.len(), 1);
            assert!(heading.children.is_empty());
        }
        _ => panic!("Expected Heading"),
    }
}

#[test]
fn test_nested_heading_with_paragraph() {
    let events = vec![
        Event::StartDocument,
        Event::StartHeading(1),
        Event::Inline(InlineContent::Text("Title".to_string())),
        Event::StartParagraph,
        Event::Inline(InlineContent::Text("Content".to_string())),
        Event::EndParagraph,
        Event::EndHeading(1),
        Event::EndDocument,
    ];

    let doc = events_to_tree(&events).unwrap();
    assert_eq!(doc.children.len(), 1);

    match &doc.children[0] {
        DocNode::Heading(heading) => {
            assert_eq!(heading.level, 1);
            assert_eq!(heading.children.len(), 1);
            assert!(matches!(&heading.children[0], DocNode::Paragraph(_)));
        }
        _ => panic!("Expected Heading"),
    }
}

#[test]
fn test_list_with_items() {
    let events = vec![
        Event::StartDocument,
        Event::StartList {
            ordered: false,
            style: ListStyle::Bullet,
            form: ListForm::Short,
        },
        Event::StartListItem,
        Event::Inline(InlineContent::Text("Item 1".to_string())),
        Event::EndListItem,
        Event::StartListItem,
        Event::Inline(InlineContent::Text("Item 2".to_string())),
        Event::EndListItem,
        Event::EndList,
        Event::EndDocument,
    ];

    let doc = events_to_tree(&events).unwrap();
    assert_eq!(doc.children.len(), 1);

    match &doc.children[0] {
        DocNode::List(list) => {
            assert_eq!(list.items.len(), 2);
        }
        _ => panic!("Expected List"),
    }
}

#[test]
fn test_definition() {
    let events = vec![
        Event::StartDocument,
        Event::StartDefinition,
        Event::StartDefinitionTerm,
        Event::Inline(InlineContent::Text("Term".to_string())),
        Event::EndDefinitionTerm,
        Event::StartDefinitionDescription,
        Event::StartParagraph,
        Event::Inline(InlineContent::Text("Description".to_string())),
        Event::EndParagraph,
        Event::EndDefinitionDescription,
        Event::EndDefinition,
        Event::EndDocument,
    ];

    let doc = events_to_tree(&events).unwrap();
    assert_eq!(doc.children.len(), 1);

    match &doc.children[0] {
        DocNode::Definition(def) => {
            assert_eq!(def.term.len(), 1);
            assert_eq!(def.description.len(), 1);
        }
        _ => panic!("Expected Definition"),
    }
}

#[test]
fn test_verbatim() {
    let events = vec![
        Event::StartDocument,
        Event::StartVerbatim {
            language: Some("rust".to_string()),
            subject: None,
            subject_href: None,
            parameters: Vec::new(),
        },
        Event::Inline(InlineContent::Text("fn main() {}".to_string())),
        Event::EndVerbatim,
        Event::EndDocument,
    ];

    let doc = events_to_tree(&events).unwrap();
    assert_eq!(doc.children.len(), 1);

    match &doc.children[0] {
        DocNode::Verbatim(verb) => {
            assert_eq!(verb.language, Some("rust".to_string()));
            assert_eq!(verb.content, "fn main() {}");
        }
        _ => panic!("Expected Verbatim"),
    }
}

#[test]
fn test_annotation() {
    let events = vec![
        Event::StartDocument,
        Event::StartAnnotation {
            label: "note".to_string(),
            parameters: vec![("type".to_string(), "warning".to_string())],
            form: LabelForm::Canonical,
        },
        Event::StartParagraph,
        Event::Inline(InlineContent::Text("Warning text".to_string())),
        Event::EndParagraph,
        Event::EndAnnotation {
            label: "note".to_string(),
        },
        Event::EndDocument,
    ];

    let doc = events_to_tree(&events).unwrap();
    assert_eq!(doc.children.len(), 1);

    match &doc.children[0] {
        DocNode::Annotation(anno) => {
            assert_eq!(anno.label, "note");
            assert_eq!(anno.parameters.len(), 1);
            assert_eq!(anno.content.len(), 1);
        }
        _ => panic!("Expected Annotation"),
    }
}

#[test]
fn test_complex_nested_document() {
    let events = vec![
        Event::StartDocument,
        Event::StartHeading(1),
        Event::Inline(InlineContent::Text("Chapter 1".to_string())),
        Event::StartHeading(2),
        Event::Inline(InlineContent::Text("Section 1.1".to_string())),
        Event::StartParagraph,
        Event::Inline(InlineContent::Text("Some text".to_string())),
        Event::EndParagraph,
        Event::StartList {
            ordered: false,
            style: ListStyle::Bullet,
            form: ListForm::Short,
        },
        Event::StartListItem,
        Event::Inline(InlineContent::Text("Item".to_string())),
        Event::EndListItem,
        Event::EndList,
        Event::EndHeading(2),
        Event::EndHeading(1),
        Event::EndDocument,
    ];

    let doc = events_to_tree(&events).unwrap();
    assert_eq!(doc.children.len(), 1);

    match &doc.children[0] {
        DocNode::Heading(h1) => {
            assert_eq!(h1.level, 1);
            assert_eq!(h1.children.len(), 1);

            match &h1.children[0] {
                DocNode::Heading(h2) => {
                    assert_eq!(h2.level, 2);
                    assert_eq!(h2.children.len(), 2); // paragraph and list
                }
                _ => panic!("Expected nested Heading"),
            }
        }
        _ => panic!("Expected top Heading"),
    }
}

#[test]
fn test_error_mismatched_end() {
    let events = vec![
        Event::StartDocument,
        Event::StartParagraph,
        Event::EndHeading(1), // Wrong end!
    ];

    let result = events_to_tree(&events);
    assert!(matches!(
        result,
        Err(ConversionError::MismatchedEvents { .. })
    ));
}

#[test]
fn test_error_unclosed_container() {
    let events = vec![
        Event::StartDocument,
        Event::StartParagraph,
        Event::EndDocument, // Missing EndParagraph
    ];

    let result = events_to_tree(&events);
    assert!(matches!(
        result,
        Err(ConversionError::UnclosedContainers(_))
    ));
}

#[test]
fn test_error_extra_events() {
    let events = vec![
        Event::StartDocument,
        Event::EndDocument,
        Event::StartParagraph, // Extra after end!
    ];

    let result = events_to_tree(&events);
    assert!(matches!(result, Err(ConversionError::ExtraEvents)));
}

#[test]
fn test_error_mismatched_heading_level() {
    let events = vec![
        Event::StartDocument,
        Event::StartHeading(1),
        Event::EndHeading(2), // Wrong level!
        Event::EndDocument,
    ];

    let result = events_to_tree(&events);
    assert!(matches!(
        result,
        Err(ConversionError::MismatchedEvents { .. })
    ));
}

#[test]
fn document_annotations_survive_tree_events_round_trip() {
    // Phase 3b (#614): `document_annotations` is carried through
    // the IR without being flattened into the event stream as a
    // synthetic `frontmatter` annotation. `tree_to_events` →
    // `events_to_tree` is a structural round-trip on `children`,
    // but `document_annotations` is intentionally invisible to
    // the event layer (format-specific serializers read it from
    // the IR directly). The round-trip rebuilds an empty slot;
    // we lock that contract here so a regression to the old
    // synthesis is caught.
    use crate::ir::nodes::Annotation;
    use crate::ir::to_events::tree_to_events;

    let original = Document {
        title: None,
        subtitle: None,
        children: vec![DocNode::Paragraph(Paragraph {
            content: vec![InlineContent::Text("Body.".to_string())],
        })],
        document_annotations: vec![Annotation {
            label: "lex.metadata.author".to_string(),
            parameters: vec![],
            content: vec![DocNode::Paragraph(Paragraph {
                content: vec![InlineContent::Text("Alice".to_string())],
            })],
            form: LabelForm::Canonical,
        }],
    };

    let events = tree_to_events(&DocNode::Document(original.clone()));

    // No synthetic `frontmatter` annotation event leaks into the
    // stream — the Phase 3b flip retired that synthesis.
    let has_frontmatter = events.iter().any(|e| {
        matches!(
            e,
            Event::StartAnnotation { label, .. } if label == "frontmatter"
        )
    });
    assert!(
        !has_frontmatter,
        "Phase 3b: tree_to_events must not synthesize a frontmatter event"
    );

    // Body events still round-trip through events_to_tree.
    let rebuilt = events_to_tree(&events).expect("events_to_tree");
    assert_eq!(rebuilt.children, original.children);
}

#[test]
fn test_round_trip() {
    use crate::ir::to_events::tree_to_events;

    let original_doc = Document {
        title: None,
        subtitle: None,
        children: vec![DocNode::Heading(Heading {
            level: 1,
            content: vec![InlineContent::Text("Title".to_string())],
            children: vec![DocNode::Paragraph(Paragraph {
                content: vec![InlineContent::Text("Content".to_string())],
            })],
        })],
        document_annotations: vec![],
    };

    // Convert to events
    let events = tree_to_events(&DocNode::Document(original_doc.clone()));

    // Convert back to tree
    let reconstructed = events_to_tree(&events).unwrap();

    // Should match
    assert_eq!(original_doc, reconstructed);
}

#[test]
fn test_round_trip_complex() {
    use crate::ir::to_events::tree_to_events;

    let original_doc = Document {
        title: None,
        subtitle: None,
        children: vec![DocNode::Heading(Heading {
            level: 1,
            content: vec![
                InlineContent::Text("Title ".to_string()),
                InlineContent::Bold(vec![InlineContent::Text("bold".to_string())]),
            ],
            children: vec![
                DocNode::List(List {
                    items: vec![
                        ListItem {
                            content: vec![InlineContent::Text("Item 1".to_string())],
                            children: vec![],
                        },
                        ListItem {
                            content: vec![InlineContent::Text("Item 2".to_string())],
                            children: vec![DocNode::Paragraph(Paragraph {
                                content: vec![InlineContent::Text("Nested".to_string())],
                            })],
                        },
                    ],
                    ordered: false,
                    style: ListStyle::Bullet,
                    form: ListForm::Short,
                }),
                DocNode::Definition(Definition {
                    term: vec![InlineContent::Text("Term".to_string())],
                    description: vec![DocNode::Paragraph(Paragraph {
                        content: vec![InlineContent::Text("Desc".to_string())],
                    })],
                }),
            ],
        })],
        document_annotations: vec![],
    };

    let events = tree_to_events(&DocNode::Document(original_doc.clone()));
    let reconstructed = events_to_tree(&events).unwrap();

    assert_eq!(original_doc, reconstructed);
}
