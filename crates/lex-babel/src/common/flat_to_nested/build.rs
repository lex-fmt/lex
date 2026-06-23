//! The flat-to-nested build algorithm.
//!
//! [`events_to_tree`] is the heart of the converter: it walks the linear event
//! stream, maintaining a stack of open [`StackNode`](super::stack_node::StackNode)
//! containers, and reconstructs the nested [`Document`] tree. The auto-close
//! helpers ([`auto_close_headings_at_or_deeper`], [`auto_close_all_headings`])
//! implement the heading-hierarchy logic that lets flat formats (Markdown, HTML,
//! LaTeX) omit explicit heading-close markers; [`finalize_container`] is the
//! shared pop-validate-attach step every `End` event funnels through.

use super::stack_node::StackNode;
use super::ConversionError;
use crate::ir::events::Event;
use crate::ir::nodes::*;

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
        return Ok(Document {
            title: None,
            subtitle: None,
            children: vec![],
            document_annotations: vec![],
        });
    }

    let mut stack: Vec<StackNode> = Vec::new();
    let mut event_iter = events.iter().peekable();

    // Expect StartDocument as first event
    match event_iter.next() {
        Some(Event::StartDocument) => {
            stack.push(StackNode::Document(Document {
                title: None,
                subtitle: None,
                children: vec![],
                document_annotations: vec![],
            }));
        }
        Some(other) => {
            return Err(ConversionError::MismatchedEvents {
                expected: "StartDocument".to_string(),
                found: format!("{other:?}"),
            });
        }
        None => {
            return Ok(Document {
                title: None,
                subtitle: None,
                children: vec![],
                document_annotations: vec![],
            })
        }
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
                    // Phase 3a of #570: `Document::document_annotations`
                    // stays empty on this path. The event stream
                    // doesn't yet distinguish document-scope
                    // annotations from inline ones — every
                    // StartAnnotation under StartDocument lands in
                    // `children` as today. Phase 3b adds the
                    // scope marker (or formalises a position
                    // contract) atomically with the legacy-path
                    // retirement.
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

            Event::StartVerbatim {
                language,
                subject,
                subject_href,
                parameters,
            } => {
                stack.push(StackNode::Verbatim {
                    subject: subject.clone(),
                    subject_href: subject_href.clone(),
                    language: language.clone(),
                    content: String::new(),
                    parameters: parameters.clone(),
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

            Event::StartAnnotation {
                label,
                parameters,
                form,
            } => {
                stack.push(StackNode::Annotation {
                    label: label.clone(),
                    parameters: parameters.clone(),
                    content: vec![],
                    form: *form,
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

            Event::StartTable { caption, fullwidth } => {
                stack.push(StackNode::Table {
                    rows: vec![],
                    header: vec![],
                    caption: caption.clone(),
                    footnotes: vec![],
                    fullwidth: *fullwidth,
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

            Event::StartTableCell {
                header,
                align,
                colspan,
                rowspan,
            } => {
                stack.push(StackNode::TableCell {
                    content: vec![],
                    header: *header,
                    align: *align,
                    colspan: *colspan,
                    rowspan: *rowspan,
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
                        colspan,
                        rowspan,
                    } => {
                        let cell = TableCell {
                            content,
                            header,
                            align,
                            colspan,
                            rowspan,
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

            Event::StartTableFootnotes => {
                stack.push(StackNode::TableFootnotes { content: vec![] });
            }

            Event::EndTableFootnotes => {
                let node = stack.pop().ok_or_else(|| {
                    ConversionError::UnexpectedEnd("EndTableFootnotes with empty stack".to_string())
                })?;
                match node {
                    StackNode::TableFootnotes { content } => {
                        let parent = stack.last_mut().ok_or_else(|| {
                            ConversionError::UnexpectedEnd(
                                "No parent for table footnotes".to_string(),
                            )
                        })?;
                        match parent {
                            StackNode::Table { footnotes, .. } => {
                                *footnotes = content;
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
                            expected: "TableFootnotes".to_string(),
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
