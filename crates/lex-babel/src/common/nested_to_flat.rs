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
        DocNode::Document(Document {
            children,
            document_annotations,
            ..
        }) => {
            events.push(Event::StartDocument);
            // Post-refac/label cleanup: synthesize the `frontmatter`
            // annotation event from `document_annotations` here, at
            // the events-emission layer. The IR Document no longer
            // carries a synthetic `frontmatter` annotation in
            // `children` (that was the legacy `from_lex_document`
            // promotion). Downstream HTML/Markdown serializers still
            // match on `Event::StartAnnotation { label: "frontmatter", .. }`
            // unchanged — the event shape is preserved.
            emit_frontmatter_event(document_annotations, events);
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
            // Post-refac/label cleanup: the bare-label metadata
            // whitelist (`author`, `title`, …) that used to live here
            // and synthesize `lex-metadata:<label>` verbatim events is
            // gone. After #570 Phase 3b activated `NormalizeLabels`,
            // no production path produces IR annotations with those
            // bare labels — they're either rewritten to canonical
            // `lex.metadata.*` and promoted into the `frontmatter`
            // annotation by `from_lex_document`, or they come from
            // markdown imports as a single packed `frontmatter`
            // annotation. The HTML/Markdown serializers handle the
            // `frontmatter` label directly in their `StartAnnotation`
            // arms.
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
            caption,
            footnotes,
            fullwidth,
        }) => {
            events.push(Event::StartTable {
                caption: caption.clone(),
                fullwidth: *fullwidth,
            });
            for row in header {
                walk_table_row(row, events, true);
            }
            for row in rows {
                walk_table_row(row, events, false);
            }
            if !footnotes.is_empty() {
                events.push(Event::StartTableFootnotes);
                for node in footnotes {
                    walk_node(node, events);
                }
                events.push(Event::EndTableFootnotes);
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
        colspan: cell.colspan,
        rowspan: cell.rowspan,
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

/// Synthesize the `frontmatter` annotation event from a document's
/// `document_annotations` slot. Each annotation's `lex.metadata.<key>`
/// label is stripped to the short form (`key`); annotations with text
/// body become `(key, body_text)` params; annotations with structured
/// parameters become `(key.subkey, value)` params. The legacy
/// `from_lex_document` promotion produced the same flat-params shape
/// the downstream serializers expect.
fn emit_frontmatter_event(document_annotations: &[Annotation], events: &mut Vec<Event>) {
    if document_annotations.is_empty() {
        return;
    }
    let mut parameters: Vec<(String, String)> = Vec::new();
    for ann in document_annotations {
        let key = ann
            .label
            .strip_prefix("lex.metadata.")
            .unwrap_or(ann.label.as_str())
            .to_string();
        let body_text = flatten_paragraph_text(&ann.content);
        if !body_text.is_empty() {
            parameters.push((key, body_text));
        } else if !ann.parameters.is_empty() {
            for (k, v) in &ann.parameters {
                parameters.push((format!("{key}.{k}"), v.clone()));
            }
        } else {
            // Marker-form annotation with neither body nor params:
            // emit the key with an empty value so its presence is
            // surfaced (matches the legacy promotion behaviour for
            // such cases).
            parameters.push((key, String::new()));
        }
    }
    events.push(Event::StartAnnotation {
        label: "frontmatter".to_string(),
        parameters,
    });
    events.push(Event::EndAnnotation {
        label: "frontmatter".to_string(),
    });
}

/// Flatten the text content of an annotation's paragraph children into
/// a single string. Used by `emit_frontmatter_event` to derive the
/// metadata value when the annotation has a body but no structured
/// parameters. Mirrors what `from_lex_document` used to do in its
/// children-scan promotion logic.
fn flatten_paragraph_text(content: &[DocNode]) -> String {
    let mut text = String::new();
    for child in content {
        if let DocNode::Paragraph(p) = child {
            for inline in &p.content {
                if let InlineContent::Text(t) = inline {
                    text.push_str(t);
                }
            }
        }
    }
    text
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
            title: None,
            subtitle: None,
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
            document_annotations: vec![],
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
            // Post-refac/label cleanup: the bare-label metadata
            // path that used to emit a `lex-metadata:note` verbatim
            // is gone. The `:: note ::` annotation now flows through
            // the normal annotation events.
            Event::StartAnnotation {
                label: "note".to_string(),
                parameters: vec![("key".to_string(), "value".to_string())],
            },
            Event::StartContent,
            Event::StartParagraph,
            Event::Inline(InlineContent::Text("Body".to_string())),
            Event::EndParagraph,
            Event::EndContent,
            Event::EndAnnotation {
                label: "note".to_string(),
            },
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
