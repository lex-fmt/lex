//! Tests for the flat <-> nested IR conversion.

use lex_babel::common::flat_to_nested::events_to_tree;
use lex_babel::ir::events::Event;
use lex_babel::ir::nodes::*;
use lex_babel::ir::to_events::tree_to_events;

fn create_test_tree() -> DocNode {
    DocNode::Document(Document {
        title: None,
        subtitle: None,
        children: vec![
            DocNode::Heading(Heading {
                level: 1,
                content: vec![InlineContent::Text("Title".to_string())],
                children: vec![
                    DocNode::Paragraph(Paragraph {
                        content: vec![InlineContent::Text("Paragraph 1".to_string())],
                    }),
                    DocNode::List(List {
                        items: vec![
                            ListItem {
                                content: vec![InlineContent::Text("Item 1".to_string())],
                                children: vec![],
                            },
                            ListItem {
                                content: vec![InlineContent::Text("Item 2".to_string())],
                                children: vec![DocNode::Paragraph(Paragraph {
                                    content: vec![InlineContent::Text("Nested Para".to_string())],
                                })],
                            },
                        ],
                        ordered: false,
                        style: ListStyle::Bullet,
                        form: ListForm::Short,
                    }),
                ],
            }),
            DocNode::Paragraph(Paragraph {
                content: vec![
                    InlineContent::Text("Final ".to_string()),
                    InlineContent::Bold(vec![InlineContent::Text("paragraph".to_string())]),
                ],
            }),
        ],
    })
}

#[test]
fn test_round_trip_conversion() {
    let original_tree = create_test_tree();

    // 1. Convert tree to events (nested -> flat)
    let events = tree_to_events(&original_tree);

    // 2. Convert events back to tree (flat -> nested)
    let reconstructed_doc = events_to_tree(&events).expect("Failed to reconstruct tree");
    let reconstructed_tree = DocNode::Document(reconstructed_doc);

    // 3. Assert that the original and reconstructed trees are identical
    assert_eq!(original_tree, reconstructed_tree);
}

#[test]
fn test_event_stream_generation() {
    let tree = create_test_tree();
    let events = tree_to_events(&tree);

    let expected_events = vec![
        Event::StartDocument,
        Event::StartHeading(1),
        Event::Inline(InlineContent::Text("Title".to_string())),
        Event::StartContent,
        Event::StartParagraph,
        Event::Inline(InlineContent::Text("Paragraph 1".to_string())),
        Event::EndParagraph,
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
        Event::StartContent,
        Event::StartParagraph,
        Event::Inline(InlineContent::Text("Nested Para".to_string())),
        Event::EndParagraph,
        Event::EndContent,
        Event::EndListItem,
        Event::EndList,
        Event::EndContent,
        Event::EndHeading(1),
        Event::StartParagraph,
        Event::Inline(InlineContent::Text("Final ".to_string())),
        Event::Inline(InlineContent::Bold(vec![InlineContent::Text(
            "paragraph".to_string(),
        )])),
        Event::EndParagraph,
        Event::EndDocument,
    ];

    assert_eq!(events, expected_events);
}
