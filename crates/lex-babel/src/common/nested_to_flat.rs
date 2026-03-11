//! Converts a nested IR tree structure into a flat event stream.
//!
//! # The High-Level Concept
//!
//! Traversing the nested document structure in pre-order lets us emit a
//! sequence of start/content/end events that can later be reassembled into
//! the original tree. Each container node produces its own start/end markers
//! and then recurses into children so the flat stream preserves the original
//! nesting.
//!
//! # The Algorithm
//!
//! 1. **Initialization:**
//!    - Create an empty event vector
//!    - Begin walking from the root `DocNode`
//!
//! 2. **Entering Containers:**
//!    - Emit the corresponding `Start*` event
//!    - Emit inline content, if any
//!    - Recurse into child nodes
//!
//! 3. **Handling Inline Nodes:**
//!    - Inline-only nodes become a single `Inline` event in place
//!
//! 4. **Exiting Containers:**
//!    - Emit the matching `End*` event once children are processed
//!
//! 5. **Completion:**
//!    - Return the accumulated event stream
//!
//! This mirrors the reverse process performed in `flat_to_nested`, ensuring
//! round-trippable conversions between the nested IR and flat event stream.

use crate::ir::events::Event;
use crate::ir::nodes::{
    Annotation, Definition, DocNode, Document, Heading, InlineContent, List, ListItem, Paragraph,
    Table, TableCell, TableRow, Verbatim,
};

/// Converts a `DocNode` tree to a flat vector of `Event`s.
pub fn tree_to_events(root_node: &DocNode) -> Vec<Event> {
    let mut events = Vec::new();
    walk_node(root_node, &mut events);
    events
}

fn walk_node(node: &DocNode, events: &mut Vec<Event>) {
    match node {
        DocNode::Document(Document { children }) => {
            events.push(Event::StartDocument);
            for child in children {
                walk_node(child, events);
            }
            events.push(Event::EndDocument);
        }
        DocNode::Heading(Heading {
            level,
            content,
            children,
        }) => {
            events.push(Event::StartHeading(*level));
            emit_inlines(content, events);
            if !children.is_empty() {
                events.push(Event::StartContent);
                for child in children {
                    walk_node(child, events);
                }
                events.push(Event::EndContent);
            }
            events.push(Event::EndHeading(*level));
        }
        DocNode::Paragraph(Paragraph { content }) => {
            events.push(Event::StartParagraph);
            emit_inlines(content, events);
            events.push(Event::EndParagraph);
        }
        DocNode::List(List {
            items,
            ordered,
            style,
            form,
        }) => {
            events.push(Event::StartList {
                ordered: *ordered,
                style: *style,
                form: *form,
            });
            for item in items {
                walk_list_item(item, events);
            }
            events.push(Event::EndList);
        }
        DocNode::ListItem(_) => {
            // List items are emitted by the surrounding list handler.
            if cfg!(debug_assertions) {
                unreachable!("ListItem should only be emitted by List");
            }
        }
        DocNode::Definition(Definition { term, description }) => {
            events.push(Event::StartDefinition);
            events.push(Event::StartDefinitionTerm);
            emit_inlines(term, events);
            events.push(Event::EndDefinitionTerm);
            events.push(Event::StartDefinitionDescription);
            if !description.is_empty() {
                events.push(Event::StartContent);
                for child in description {
                    walk_node(child, events);
                }
                events.push(Event::EndContent);
            }
            events.push(Event::EndDefinitionDescription);
            events.push(Event::EndDefinition);
        }
        DocNode::Verbatim(Verbatim {
            subject,
            language,
            content,
        }) => {
            events.push(Event::StartVerbatim {
                language: language.clone(),
                subject: subject.clone(),
            });
            events.push(Event::Inline(InlineContent::Text(content.clone())));
            events.push(Event::EndVerbatim);
        }
        DocNode::Annotation(Annotation {
            label,
            parameters,
            content,
        }) => {
            // Check if this is a metadata annotation that should be serialized as a single HTML block
            let metadata_labels = [
                "author", "note", "title", "date", "tags", "category", "template",
            ];
            if metadata_labels.contains(&label.as_str()) {
                // Serialize content to text
                // This is a simplification: we assume content is mostly text paragraphs
                let mut text_content = String::new();
                for child in content {
                    if let DocNode::Paragraph(p) = child {
                        for inline in &p.content {
                            if let InlineContent::Text(t) = inline {
                                text_content.push_str(t);
                            } else if let InlineContent::Reference(r) = inline {
                                text_content.push_str(r);
                            }
                            // Ignore other inline types for now or implement full serialization
                        }
                        text_content.push('\n');
                    }
                }

                // Let's construct the full comment string here.
                let mut comment_body = String::new();
                for (key, value) in parameters {
                    comment_body.push_str(&format!(" {key}={value}"));
                }
                if !text_content.is_empty() {
                    comment_body.push('\n');
                    comment_body.push_str(&text_content);
                }

                events.push(Event::StartVerbatim {
                    language: Some(format!("lex-metadata:{label}")),
                    subject: None,
                });
                events.push(Event::Inline(InlineContent::Text(comment_body)));
                events.push(Event::EndVerbatim);
                return;
            }

            events.push(Event::StartAnnotation {
                label: label.clone(),
                parameters: parameters.clone(),
            });
            if !content.is_empty() {
                events.push(Event::StartContent);
                for child in content {
                    walk_node(child, events);
                }
                events.push(Event::EndContent);
            }
            events.push(Event::EndAnnotation {
                label: label.clone(),
            });
        }
        DocNode::Table(Table {
            rows,
            header,
            caption: _,
        }) => {
            events.push(Event::StartTable);
            for row in header {
                walk_table_row(row, events, true);
            }
            for row in rows {
                walk_table_row(row, events, false);
            }
            events.push(Event::EndTable);
        }
        DocNode::Image(image) => events.push(Event::Image(image.clone())),
        DocNode::Video(video) => events.push(Event::Video(video.clone())),
        DocNode::Audio(audio) => events.push(Event::Audio(audio.clone())),
        DocNode::Inline(inline) => events.push(Event::Inline(inline.clone())),
    }
}

fn walk_table_row(row: &TableRow, events: &mut Vec<Event>, header: bool) {
    events.push(Event::StartTableRow { header });
    for cell in &row.cells {
        walk_table_cell(cell, events);
    }
    events.push(Event::EndTableRow);
}

fn walk_table_cell(cell: &TableCell, events: &mut Vec<Event>) {
    events.push(Event::StartTableCell {
        header: cell.header,
        align: cell.align,
    });
    if !cell.content.is_empty() {
        events.push(Event::StartContent);
        for child in &cell.content {
            walk_node(child, events);
        }
        events.push(Event::EndContent);
    }
    events.push(Event::EndTableCell);
}

fn walk_list_item(item: &ListItem, events: &mut Vec<Event>) {
    events.push(Event::StartListItem);
    emit_inlines(&item.content, events);
    if !item.children.is_empty() {
        events.push(Event::StartContent);
        for child in &item.children {
            walk_node(child, events);
        }
        events.push(Event::EndContent);
    }
    events.push(Event::EndListItem);
}

fn emit_inlines(inlines: &[InlineContent], events: &mut Vec<Event>) {
    for inline in inlines {
        events.push(Event::Inline(inline.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::flat_to_nested::events_to_tree;
    use crate::ir::nodes::{ListForm, ListStyle};

    fn sample_tree() -> DocNode {
        DocNode::Document(Document {
            children: vec![
                DocNode::Heading(Heading {
                    level: 2,
                    content: vec![InlineContent::Text("Intro".to_string())],
                    children: vec![DocNode::Paragraph(Paragraph {
                        content: vec![InlineContent::Text("Welcome".to_string())],
                    })],
                }),
                DocNode::List(List {
                    items: vec![ListItem {
                        content: vec![InlineContent::Text("Item".to_string())],
                        children: vec![DocNode::Verbatim(Verbatim {
                            subject: None,
                            language: Some("rust".to_string()),
                            content: "fn main() {}".to_string(),
                        })],
                    }],
                    ordered: false,
                    style: ListStyle::Bullet,
                    form: ListForm::Short,
                }),
                DocNode::Definition(Definition {
                    term: vec![InlineContent::Text("Term".to_string())],
                    description: vec![DocNode::Paragraph(Paragraph {
                        content: vec![InlineContent::Text("Definition".to_string())],
                    })],
                }),
                DocNode::Annotation(Annotation {
                    label: "note".to_string(),
                    parameters: vec![("key".to_string(), "value".to_string())],
                    content: vec![DocNode::Paragraph(Paragraph {
                        content: vec![InlineContent::Text("Body".to_string())],
                    })],
                }),
            ],
        })
    }

    #[test]
    fn flattens_nested_document() {
        let events = tree_to_events(&sample_tree());

        let expected = vec![
            Event::StartDocument,
            Event::StartHeading(2),
            Event::Inline(InlineContent::Text("Intro".to_string())),
            Event::StartContent,
            Event::StartParagraph,
            Event::Inline(InlineContent::Text("Welcome".to_string())),
            Event::EndParagraph,
            Event::EndContent,
            Event::EndHeading(2),
            Event::StartList {
                ordered: false,
                style: ListStyle::Bullet,
                form: ListForm::Short,
            },
            Event::StartListItem,
            Event::Inline(InlineContent::Text("Item".to_string())),
            Event::StartContent,
            Event::StartVerbatim {
                language: Some("rust".to_string()),
                subject: None,
            },
            Event::Inline(InlineContent::Text("fn main() {}".to_string())),
            Event::EndVerbatim,
            Event::EndContent,
            Event::EndListItem,
            Event::EndList,
            Event::StartDefinition,
            Event::StartDefinitionTerm,
            Event::Inline(InlineContent::Text("Term".to_string())),
            Event::EndDefinitionTerm,
            Event::StartDefinitionDescription,
            Event::StartContent,
            Event::StartParagraph,
            Event::Inline(InlineContent::Text("Definition".to_string())),
            Event::EndParagraph,
            Event::EndContent,
            Event::EndDefinitionDescription,
            Event::EndDefinition,
            Event::StartVerbatim {
                language: Some("lex-metadata:note".to_string()),
                subject: None,
            },
            Event::Inline(InlineContent::Text(" key=value\nBody\n".to_string())),
            Event::EndVerbatim,
            Event::EndDocument,
        ];

        assert_eq!(events, expected);
    }

    #[test]
    fn round_trips_with_flat_to_nested() {
        let original = sample_tree();
        let events = tree_to_events(&original);
        let rebuilt = events_to_tree(&events).expect("failed to rebuild");

        assert_eq!(DocNode::Document(rebuilt), original);
    }
}
