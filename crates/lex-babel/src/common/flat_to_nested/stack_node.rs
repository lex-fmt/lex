//! The node-stack machinery for [`events_to_tree`](super::events_to_tree).
//!
//! [`StackNode`] is the in-progress, mutable counterpart to the immutable
//! [`DocNode`] tree. While the converter walks the event stream it keeps a
//! stack of `StackNode`s — each one an "open" container accumulating inline
//! content and child nodes. On the matching `End` event the node is finalized
//! into a [`DocNode`] via [`StackNode::into_doc_node`] and handed to its parent.

use super::ConversionError;
use crate::ir::nodes::*;

/// Represents a node being built on the stack
#[derive(Debug)]
pub(super) enum StackNode {
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
        subject_href: Option<String>,
        language: Option<String>,
        content: String,
        parameters: Vec<(String, String)>,
    },
    Annotation {
        label: String,
        parameters: Vec<(String, String)>,
        content: Vec<DocNode>,
        form: LabelForm,
    },
    Table {
        rows: Vec<TableRow>,
        header: Vec<TableRow>,
        caption: Option<Vec<InlineContent>>,
        footnotes: Vec<DocNode>,
        fullwidth: bool,
    },
    TableRow {
        cells: Vec<TableCell>,
        header: bool,
    },
    TableCell {
        content: Vec<DocNode>,
        header: bool,
        align: TableCellAlignment,
        colspan: usize,
        rowspan: usize,
    },
    TableFootnotes {
        content: Vec<DocNode>,
    },
}

impl StackNode {
    /// Convert to a DocNode (used when popping from stack)
    pub(super) fn into_doc_node(self) -> DocNode {
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
                subject_href,
                language,
                content,
                parameters,
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
                            form: LabelForm::Canonical,
                        });
                    }
                }
                DocNode::Verbatim(Verbatim {
                    subject,
                    subject_href,
                    language,
                    content,
                    parameters,
                })
            }
            StackNode::Annotation {
                label,
                parameters,
                content,
                form,
            } => DocNode::Annotation(Annotation {
                label,
                parameters,
                content,
                form,
            }),
            StackNode::Table {
                rows,
                header,
                caption,
                footnotes,
                fullwidth,
            } => DocNode::Table(Table {
                rows,
                header,
                caption,
                footnotes,
                fullwidth,
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
            StackNode::TableFootnotes { .. } => {
                panic!("TableFootnotes cannot be converted directly to DocNode")
            }
        }
    }

    /// Get the node type name for error messages
    pub(super) fn type_name(&self) -> &str {
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
            StackNode::TableFootnotes { .. } => "TableFootnotes",
        }
    }

    /// Add a child DocNode to this container
    pub(super) fn add_child(&mut self, child: DocNode) -> Result<(), ConversionError> {
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
            StackNode::TableFootnotes { content, .. } => {
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
    pub(super) fn add_inline(&mut self, inline: InlineContent) -> Result<(), ConversionError> {
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
