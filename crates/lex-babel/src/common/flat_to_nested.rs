//! Converts a flat event stream back to a nested IR tree structure.
//!
//! # The High-Level Concept
//!
//! The core challenge is to reconstruct a tree structure from a linear sequence of events.
//! The algorithm uses a stack to keep track of the current nesting level. The stack acts as
//! a memory of "open" containers. When we encounter a `Start` event for a container (like a
//! heading or list), we push it onto the stack, making it the new "current" container. When
//! we see its corresponding `End` event, we pop it off, returning to the parent container.
//!
//! # Auto-Closing Headings (For Flat Formats)
//!
//! This converter includes special logic for headings to support flat document formats
//! (Markdown, HTML, LaTeX) where headings don't have explicit close markers. When a new
//! `StartHeading(level)` event is encountered, the converter automatically closes any
//! currently open headings at the same or deeper level before opening the new heading.
//!
//! This means format parsers can simply emit `StartHeading` events without worrying about
//! emitting matching `EndHeading` events - the generic converter handles the hierarchy.
//!
//! Example event stream from Markdown parser:
//! ```text
//! StartDocument
//! StartHeading(1)         <- Opens h1
//! StartHeading(2)         <- Auto-closes nothing, opens h2 nested in h1
//! StartHeading(1)         <- Auto-closes h2 and previous h1, opens new h1
//! EndDocument             <- Auto-closes remaining h1
//! ```
//!
//! # The Algorithm
//!
//! 1. **Initialization:**
//!    - Create the root `Document` node
//!    - Create an empty stack
//!    - Push the root onto the stack as the current container
//!
//! 2. **Processing `Start` Events:**
//!    - Create a new empty `DocNode` for that element
//!    - Add it as a child to the current parent (top of stack)
//!    - Push it onto the stack as the new current container
//!
//! 3. **Processing Content Events (Inline):**
//!    - Add the content to the current parent (top of stack)
//!    - Do NOT modify the stack (content is a leaf)
//!
//! 4. **Processing `End` Events:**
//!    - Pop the node off the stack
//!    - Validate that the popped node matches the End event
//!
//! 5. **Completion:**
//!    - The stack should contain only the root Document node
//!    - This root contains the complete reconstructed AST

use crate::ir::events::Event;
use crate::ir::nodes::*;

/// Error type for flat-to-nested conversion
#[derive(Debug, Clone, PartialEq)]
pub enum ConversionError {
    /// Stack was empty when trying to pop
    UnexpectedEnd(String),
    /// Mismatched start/end events
    MismatchedEvents { expected: String, found: String },
    /// Unexpected inline content in wrong context
    UnexpectedInline(String),
    /// Events remaining after document end
    ExtraEvents,
    /// Stack not empty at end (unclosed containers)
    UnclosedContainers(usize),
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversionError::UnexpectedEnd(msg) => write!(f, "Unexpected end event: {msg}"),
            ConversionError::MismatchedEvents { expected, found } => {
                write!(f, "Mismatched events: expected {expected}, found {found}")
            }
            ConversionError::UnexpectedInline(msg) => {
                write!(f, "Unexpected inline content: {msg}")
            }
            ConversionError::ExtraEvents => write!(f, "Extra events after document end"),
            ConversionError::UnclosedContainers(count) => {
                write!(f, "Unclosed containers: {count} nodes remain on stack")
            }
        }
    }
}

impl std::error::Error for ConversionError {}

/// Represents a node being built on the stack
#[derive(Debug)]
enum StackNode {
    Document(Document),
    Heading {
        level: usize,
        content: Vec<InlineContent>,
        children: Vec<DocNode>,
    },
    Paragraph {
        content: Vec<InlineContent>,
    },
    List {
        items: Vec<ListItem>,
        ordered: bool,
        style: ListStyle,
        form: ListForm,
    },
    ListItem {
        content: Vec<InlineContent>,
        children: Vec<DocNode>,
    },
    Definition {
        term: Vec<InlineContent>,
        description: Vec<DocNode>,
        in_term: bool,
    },
    Verbatim {
        subject: Option<String>,
        language: Option<String>,
        content: String,
    },
    Annotation {
        label: String,
        parameters: Vec<(String, String)>,
        content: Vec<DocNode>,
    },
    Table {
        rows: Vec<TableRow>,
        header: Vec<TableRow>,
        caption: Option<Vec<InlineContent>>,
    },
    TableRow {
        cells: Vec<TableCell>,
        header: bool,
    },
    TableCell {
        content: Vec<DocNode>,
        header: bool,
        align: TableCellAlignment,
    },
}

impl StackNode {
    /// Convert to a DocNode (used when popping from stack)
    fn into_doc_node(self) -> DocNode {
        match self {
            StackNode::Document(doc) => DocNode::Document(doc),
            StackNode::Heading {
                level,
                content,
                children,
            } => DocNode::Heading(Heading {
                level,
                content,
                children,
            }),
            StackNode::Paragraph { content } => DocNode::Paragraph(Paragraph { content }),
            StackNode::List {
                items,
                ordered,
                style,
                form,
            } => DocNode::List(List {
                items,
                ordered,
                style,
                form,
            }),
            StackNode::ListItem { content, children } => {
                DocNode::ListItem(ListItem { content, children })
            }
            StackNode::Definition {
                term, description, ..
            } => DocNode::Definition(Definition { term, description }),
            StackNode::Verbatim {
                subject,
                language,
                content,
            } => {
                if let Some(lang) = &language {
                    if let Some(label) = lang.strip_prefix("lex-metadata:") {
                        // Convert back to Annotation
                        // Format: " key=val key2=val2\nBody"

                        let (header, body) = if let Some((h, b)) = content.split_once('\n') {
                            (h, Some(b.to_string()))
                        } else {
                            (content.as_str(), None)
                        };

                        let mut parameters = vec![];
                        for part in header.split_whitespace() {
                            if let Some((key, value)) = part.split_once('=') {
                                parameters.push((key.to_string(), value.to_string()));
                            }
                        }

                        let mut content_nodes = vec![];
                        if let Some(text) = body {
                            let text = text.strip_suffix('\n').unwrap_or(&text);

                            if !text.is_empty() {
                                content_nodes.push(DocNode::Paragraph(Paragraph {
                                    content: vec![InlineContent::Text(text.to_string())],
                                }));
                            }
                        }

                        return DocNode::Annotation(Annotation {
                            label: label.to_string(),
                            parameters,
                            content: content_nodes,
                        });
                    }
                }
                DocNode::Verbatim(Verbatim {
                    subject,
                    language,
                    content,
                })
            }
            StackNode::Annotation {
                label,
                parameters,
                content,
            } => DocNode::Annotation(Annotation {
                label,
                parameters,
                content,
            }),
            StackNode::Table {
                rows,
                header,
                caption,
            } => DocNode::Table(Table {
                rows,
                header,
                caption,
            }),
            StackNode::TableRow { cells: _, .. } => {
                // TableRow is not a DocNode, it's part of Table
                // This should not happen if logic is correct (TableRow is consumed by Table)
                panic!("TableRow cannot be converted directly to DocNode")
            }
            StackNode::TableCell { .. } => {
                // TableCell is not a DocNode
                panic!("TableCell cannot be converted directly to DocNode")
            }
        }
    }

    /// Get the node type name for error messages
    fn type_name(&self) -> &str {
        match self {
            StackNode::Document(_) => "Document",
            StackNode::Heading { .. } => "Heading",
            StackNode::Paragraph { .. } => "Paragraph",
            StackNode::List { .. } => "List",
            StackNode::ListItem { .. } => "ListItem",
            StackNode::Definition { .. } => "Definition",
            StackNode::Verbatim { .. } => "Verbatim",
            StackNode::Annotation { .. } => "Annotation",
            StackNode::Table { .. } => "Table",
            StackNode::TableRow { .. } => "TableRow",
            StackNode::TableCell { .. } => "TableCell",
        }
    }

    /// Add a child DocNode to this container
    fn add_child(&mut self, child: DocNode) -> Result<(), ConversionError> {
        match self {
            StackNode::Document(doc) => {
                doc.children.push(child);
                Ok(())
            }
            StackNode::Heading { children, .. } => {
                children.push(child);
                Ok(())
            }
            StackNode::ListItem { children, .. } => {
                children.push(child);
                Ok(())
            }
            StackNode::List { items, .. } => {
                if let DocNode::ListItem(item) = child {
                    items.push(item);
                    Ok(())
                } else {
                    Err(ConversionError::MismatchedEvents {
                        expected: "ListItem".to_string(),
                        found: format!("{child:?}"),
                    })
                }
            }
            StackNode::Definition {
                description,
                in_term,
                ..
            } => {
                if *in_term {
                    Err(ConversionError::UnexpectedInline(
                        "Cannot add child to definition term".to_string(),
                    ))
                } else {
                    description.push(child);
                    Ok(())
                }
            }
            StackNode::Annotation { content, .. } => {
                content.push(child);
                Ok(())
            }
            StackNode::TableCell { content, .. } => {
                content.push(child);
                Ok(())
            }
            _ => Err(ConversionError::UnexpectedInline(format!(
                "Node {} cannot have children",
                self.type_name()
            ))),
        }
    }

    /// Add inline content to this node
    fn add_inline(&mut self, inline: InlineContent) -> Result<(), ConversionError> {
        match self {
            StackNode::Heading { content, .. } => {
                content.push(inline);
                Ok(())
            }
            StackNode::Paragraph { content } => {
                content.push(inline);
                Ok(())
            }
            StackNode::ListItem { content, .. } => {
                content.push(inline);
                Ok(())
            }
            StackNode::Definition { term, in_term, .. } => {
                if *in_term {
                    term.push(inline);
                    Ok(())
                } else {
                    Err(ConversionError::UnexpectedInline(
                        "Inline content in definition description".to_string(),
                    ))
                }
            }
            StackNode::Verbatim { content, .. } => {
                if let InlineContent::Text(text) = inline {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(&text);
                    Ok(())
                } else {
                    Err(ConversionError::UnexpectedInline(
                        "Verbatim can only contain plain text".to_string(),
                    ))
                }
            }
            _ => Err(ConversionError::UnexpectedInline(format!(
                "Cannot add inline content to {}",
                self.type_name()
            ))),
        }
    }
}

fn finalize_container<F>(
    stack: &mut Vec<StackNode>,
    event_name: &str,
    parent_label: &str,
    validate: F,
) -> Result<(), ConversionError>
where
    F: FnOnce(StackNode) -> Result<StackNode, ConversionError>,
{
    let node = stack
        .pop()
        .ok_or_else(|| ConversionError::UnexpectedEnd(format!("{event_name} with empty stack")))?;

    let node = validate(node)?;

    let doc_node = node.into_doc_node();
    let parent = stack
        .last_mut()
        .ok_or_else(|| ConversionError::UnexpectedEnd(format!("No parent for {parent_label}")))?;
    parent.add_child(doc_node)?;

    Ok(())
}

/// Auto-close any open headings at the same or deeper level
///
/// This implements the common pattern for flat document formats (Markdown, HTML, LaTeX)
/// where headings don't have explicit close markers. When we encounter a new heading,
/// we need to close any currently open headings at the same or deeper level.
///
/// Example:
/// ```text
/// # Chapter 1        <- Opens h1
/// ## Section 1.1     <- Opens h2 (nested in h1)
/// # Chapter 2        <- Closes h2, closes h1, opens new h1
/// ```
fn auto_close_headings_at_or_deeper(
    stack: &mut Vec<StackNode>,
    new_level: usize,
) -> Result<(), ConversionError> {
    // Find all headings to close (from top of stack backwards)
    let mut headings_to_close = Vec::new();

    for (i, node) in stack.iter().enumerate().rev() {
        if let StackNode::Heading { level, .. } = node {
            if *level >= new_level {
                headings_to_close.push(i);
            } else {
                // Found a parent heading at lower level, stop
                break;
            }
        } else {
            // Hit a non-heading container, stop looking
            break;
        }
    }

    // Close headings in reverse order (deepest first)
    for _ in 0..headings_to_close.len() {
        finalize_container(stack, "auto-close heading", "heading", |node| match node {
            StackNode::Heading { .. } => Ok(node),
            other => Err(ConversionError::MismatchedEvents {
                expected: "Heading".to_string(),
                found: other.type_name().to_string(),
            }),
        })?;
    }

    Ok(())
}

/// Auto-close all open headings at document end
///
/// This ensures all headings are properly closed when we reach EndDocument,
/// which is necessary for flat formats that don't have explicit heading close markers.
fn auto_close_all_headings(stack: &mut Vec<StackNode>) -> Result<(), ConversionError> {
    // Count how many headings are open
    let mut heading_count = 0;
    for node in stack.iter().rev() {
        if matches!(node, StackNode::Heading { .. }) {
            heading_count += 1;
        } else {
            // Stop at first non-heading
            break;
        }
    }

    // Close all headings
    for _ in 0..heading_count {
        finalize_container(
            stack,
            "auto-close heading at end",
            "heading",
            |node| match node {
                StackNode::Heading { .. } => Ok(node),
                other => Err(ConversionError::MismatchedEvents {
                    expected: "Heading".to_string(),
                    found: other.type_name().to_string(),
                }),
            },
        )?;
    }

    Ok(())
}

/// Converts a flat event stream back to a nested IR tree.
///
/// # Arguments
///
/// * `events` - The flat sequence of events to process
///
/// # Returns
///
/// * `Ok(Document)` - The reconstructed document tree
/// * `Err(ConversionError)` - If the event stream is malformed
///
/// # Example
///
/// ```ignore
/// use lex_babel::ir::events::Event;
/// use lex_babel::common::flat_to_nested::events_to_tree;
///
/// let events = vec![
///     Event::StartDocument,
///     Event::StartParagraph,
///     Event::Inline(InlineContent::Text("Hello".to_string())),
///     Event::EndParagraph,
///     Event::EndDocument,
/// ];
///
/// let doc = events_to_tree(&events)?;
/// assert_eq!(doc.children.len(), 1);
/// ```
pub fn events_to_tree(events: &[Event]) -> Result<Document, ConversionError> {
    if events.is_empty() {
        return Ok(Document { children: vec![] });
    }

    let mut stack: Vec<StackNode> = Vec::new();
    let mut event_iter = events.iter().peekable();

    // Expect StartDocument as first event
    match event_iter.next() {
        Some(Event::StartDocument) => {
            stack.push(StackNode::Document(Document { children: vec![] }));
        }
        Some(other) => {
            return Err(ConversionError::MismatchedEvents {
                expected: "StartDocument".to_string(),
                found: format!("{other:?}"),
            });
        }
        None => return Ok(Document { children: vec![] }),
    }

    // Process events
    while let Some(event) = event_iter.next() {
        match event {
            Event::StartDocument => {
                return Err(ConversionError::MismatchedEvents {
                    expected: "content or EndDocument".to_string(),
                    found: "StartDocument".to_string(),
                });
            }

            Event::EndDocument => {
                // Auto-close any remaining open headings before closing document
                // This handles flat formats where headings may not have explicit EndHeading events
                auto_close_all_headings(&mut stack)?;

                // Pop the document from stack
                if stack.len() != 1 {
                    return Err(ConversionError::UnclosedContainers(stack.len() - 1));
                }
                let doc_node = stack.pop().unwrap();
                if let StackNode::Document(doc) = doc_node {
                    // Check for extra events
                    if event_iter.peek().is_some() {
                        return Err(ConversionError::ExtraEvents);
                    }
                    return Ok(doc);
                } else {
                    return Err(ConversionError::MismatchedEvents {
                        expected: "Document".to_string(),
                        found: doc_node.type_name().to_string(),
                    });
                }
            }

            Event::StartHeading(level) => {
                // Auto-close any open headings at same or deeper level
                // This handles flat formats (Markdown, HTML) where headings don't have explicit close markers
                auto_close_headings_at_or_deeper(&mut stack, *level)?;

                // Push new heading
                let node = StackNode::Heading {
                    level: *level,
                    content: vec![],
                    children: vec![],
                };
                stack.push(node);
            }

            Event::EndHeading(level) => {
                // Explicit EndHeading is optional - used by nested_to_flat for export
                // Validate that the top of stack is a heading at this level
                finalize_container(&mut stack, "EndHeading", "heading", |node| match node {
                    StackNode::Heading {
                        level: node_level, ..
                    } if node_level == *level => Ok(node),
                    StackNode::Heading {
                        level: node_level, ..
                    } => Err(ConversionError::MismatchedEvents {
                        expected: format!("EndHeading({node_level})"),
                        found: format!("EndHeading({level})"),
                    }),
                    other => Err(ConversionError::MismatchedEvents {
                        expected: "Heading".to_string(),
                        found: other.type_name().to_string(),
                    }),
                })?;
            }

            Event::StartContent => {
                // Content markers don't affect tree structure - they're used by serializers
                // to create visual wrappers for indented content
            }

            Event::EndContent => {
                // Content markers don't affect tree structure
            }

            Event::StartParagraph => {
                stack.push(StackNode::Paragraph { content: vec![] });
            }

            Event::EndParagraph => {
                finalize_container(&mut stack, "EndParagraph", "paragraph", |node| match node {
                    StackNode::Paragraph { .. } => Ok(node),
                    other => Err(ConversionError::MismatchedEvents {
                        expected: "Paragraph".to_string(),
                        found: other.type_name().to_string(),
                    }),
                })?;
            }

            Event::StartList {
                ordered,
                style,
                form,
            } => {
                stack.push(StackNode::List {
                    items: vec![],
                    ordered: *ordered,
                    style: *style,
                    form: *form,
                });
            }

            Event::EndList => {
                finalize_container(&mut stack, "EndList", "list", |node| match node {
                    StackNode::List { .. } => Ok(node),
                    other => Err(ConversionError::MismatchedEvents {
                        expected: "List".to_string(),
                        found: other.type_name().to_string(),
                    }),
                })?;
            }

            Event::StartListItem => {
                stack.push(StackNode::ListItem {
                    content: vec![],
                    children: vec![],
                });
            }

            Event::EndListItem => {
                finalize_container(&mut stack, "EndListItem", "list item", |node| match node {
                    StackNode::ListItem { .. } => Ok(node),
                    other => Err(ConversionError::MismatchedEvents {
                        expected: "ListItem".to_string(),
                        found: other.type_name().to_string(),
                    }),
                })?;
            }

            Event::StartDefinition => {
                stack.push(StackNode::Definition {
                    term: vec![],
                    description: vec![],
                    in_term: false,
                });
            }

            Event::EndDefinition => {
                finalize_container(
                    &mut stack,
                    "EndDefinition",
                    "definition",
                    |node| match node {
                        StackNode::Definition { .. } => Ok(node),
                        other => Err(ConversionError::MismatchedEvents {
                            expected: "Definition".to_string(),
                            found: other.type_name().to_string(),
                        }),
                    },
                )?;
            }

            Event::StartDefinitionTerm => {
                if let Some(StackNode::Definition { in_term, .. }) = stack.last_mut() {
                    *in_term = true;
                } else {
                    return Err(ConversionError::MismatchedEvents {
                        expected: "Definition on stack".to_string(),
                        found: "StartDefinitionTerm".to_string(),
                    });
                }
            }

            Event::EndDefinitionTerm => {
                if let Some(StackNode::Definition { in_term, .. }) = stack.last_mut() {
                    *in_term = false;
                } else {
                    return Err(ConversionError::MismatchedEvents {
                        expected: "Definition on stack".to_string(),
                        found: "EndDefinitionTerm".to_string(),
                    });
                }
            }

            Event::StartDefinitionDescription => {
                // Just a marker, definition is already in description mode after EndDefinitionTerm
            }

            Event::EndDefinitionDescription => {
                // Just a marker, no action needed
            }

            Event::StartVerbatim { language, subject } => {
                stack.push(StackNode::Verbatim {
                    subject: subject.clone(),
                    language: language.clone(),
                    content: String::new(),
                });
            }

            Event::EndVerbatim => {
                finalize_container(&mut stack, "EndVerbatim", "verbatim", |node| match node {
                    StackNode::Verbatim { .. } => Ok(node),
                    other => Err(ConversionError::MismatchedEvents {
                        expected: "Verbatim".to_string(),
                        found: other.type_name().to_string(),
                    }),
                })?;
            }

            Event::StartAnnotation { label, parameters } => {
                stack.push(StackNode::Annotation {
                    label: label.clone(),
                    parameters: parameters.clone(),
                    content: vec![],
                });
            }

            Event::EndAnnotation { label } => {
                finalize_container(
                    &mut stack,
                    "EndAnnotation",
                    "annotation",
                    |node| match node {
                        StackNode::Annotation {
                            label: ref node_label,
                            ..
                        } if node_label == label || label.is_empty() => Ok(node),
                        StackNode::Annotation {
                            label: ref node_label,
                            ..
                        } => Err(ConversionError::MismatchedEvents {
                            expected: format!("EndAnnotation({node_label})"),
                            found: format!("EndAnnotation({label})"),
                        }),
                        other => Err(ConversionError::MismatchedEvents {
                            expected: "Annotation".to_string(),
                            found: other.type_name().to_string(),
                        }),
                    },
                )?;
            }

            Event::StartTable => {
                stack.push(StackNode::Table {
                    rows: vec![],
                    header: vec![],
                    caption: None,
                });
            }

            Event::EndTable => {
                finalize_container(&mut stack, "EndTable", "table", |node| match node {
                    StackNode::Table { .. } => Ok(node),
                    other => Err(ConversionError::MismatchedEvents {
                        expected: "Table".to_string(),
                        found: other.type_name().to_string(),
                    }),
                })?;
            }

            Event::StartTableRow { header } => {
                stack.push(StackNode::TableRow {
                    cells: vec![],
                    header: *header,
                });
            }

            Event::EndTableRow => {
                // TableRow is special: it's not a DocNode, so finalize_container won't work directly
                // We need to pop it and add it to the Table parent manually
                let node = stack.pop().ok_or_else(|| {
                    ConversionError::UnexpectedEnd("EndTableRow with empty stack".to_string())
                })?;

                match node {
                    StackNode::TableRow { cells, header } => {
                        let row = TableRow { cells };
                        let parent = stack.last_mut().ok_or_else(|| {
                            ConversionError::UnexpectedEnd("No parent for table row".to_string())
                        })?;

                        match parent {
                            StackNode::Table {
                                rows,
                                header: table_header,
                                ..
                            } => {
                                if header {
                                    table_header.push(row);
                                } else {
                                    rows.push(row);
                                }
                                Ok(())
                            }
                            _ => Err(ConversionError::MismatchedEvents {
                                expected: "Table".to_string(),
                                found: parent.type_name().to_string(),
                            }),
                        }?;
                    }
                    other => {
                        return Err(ConversionError::MismatchedEvents {
                            expected: "TableRow".to_string(),
                            found: other.type_name().to_string(),
                        })
                    }
                }
            }

            Event::StartTableCell { header, align } => {
                stack.push(StackNode::TableCell {
                    content: vec![],
                    header: *header,
                    align: *align,
                });
            }

            Event::EndTableCell => {
                // TableCell is special:            Event::EndTableCell => {
                let node = stack.pop().ok_or_else(|| {
                    ConversionError::UnexpectedEnd("EndTableCell with empty stack".to_string())
                })?;

                match node {
                    StackNode::TableCell {
                        content,
                        header,
                        align,
                    } => {
                        let cell = TableCell {
                            content,
                            header,
                            align,
                        };
                        let parent = stack.last_mut().ok_or_else(|| {
                            ConversionError::UnexpectedEnd("No parent for table cell".to_string())
                        })?;

                        match parent {
                            StackNode::TableRow { cells, .. } => {
                                cells.push(cell);
                                Ok(())
                            }
                            _ => Err(ConversionError::MismatchedEvents {
                                expected: "TableRow".to_string(),
                                found: parent.type_name().to_string(),
                            }),
                        }?;
                    }
                    other => {
                        return Err(ConversionError::MismatchedEvents {
                            expected: "TableCell".to_string(),
                            found: other.type_name().to_string(),
                        })
                    }
                }
            }

            Event::Image(image) => {
                let parent = stack.last_mut().ok_or_else(|| {
                    ConversionError::UnexpectedEnd("Image event with empty stack".to_string())
                })?;
                parent.add_child(DocNode::Image(image.clone()))?;
            }

            Event::Video(video) => {
                let parent = stack.last_mut().ok_or_else(|| {
                    ConversionError::UnexpectedEnd("Video event with empty stack".to_string())
                })?;
                parent.add_child(DocNode::Video(video.clone()))?;
            }

            Event::Audio(audio) => {
                let parent = stack.last_mut().ok_or_else(|| {
                    ConversionError::UnexpectedEnd("Audio event with empty stack".to_string())
                })?;
                parent.add_child(DocNode::Audio(audio.clone()))?;
            }

            Event::Inline(inline) => {
                let parent = stack.last_mut().ok_or_else(|| {
                    ConversionError::UnexpectedInline("Inline content with no parent".to_string())
                })?;
                parent.add_inline(inline.clone())?;
            }
        }
    }

    // If we reach here, document wasn't properly closed
    Err(ConversionError::UnclosedContainers(stack.len()))
}

#[cfg(test)]
mod tests {
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
    fn test_round_trip() {
        use crate::ir::to_events::tree_to_events;

        let original_doc = Document {
            children: vec![DocNode::Heading(Heading {
                level: 1,
                content: vec![InlineContent::Text("Title".to_string())],
                children: vec![DocNode::Paragraph(Paragraph {
                    content: vec![InlineContent::Text("Content".to_string())],
                })],
            })],
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
        };

        let events = tree_to_events(&DocNode::Document(original_doc.clone()));
        let reconstructed = events_to_tree(&events).unwrap();

        assert_eq!(original_doc, reconstructed);
    }
}
