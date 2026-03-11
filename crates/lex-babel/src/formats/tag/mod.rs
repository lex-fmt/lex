//! XML-like AST tag serialization
//!
//! Serializes AST nodes directly to an XML-like format.
//!
//! ## Format
//!
//! - Node type → tag name (snake-case)
//! - Label → text content
//! - Children → nested tags (no wrapper)
//!
//! ## Example
//!
//! ```text
//! <document>
//!   <session>Introduction
//!     <paragraph>
//!       <text-line>Welcome to the guide</text-line>
//!     </paragraph>
//!   </session>
//! </document>
//! ```

use crate::error::FormatError;
use crate::format::Format;
use lex_core::lex::ast::trait_helpers::try_as_container;
use lex_core::lex::ast::traits::AstNode;
use lex_core::lex::ast::{ContentItem, Document};
use std::collections::HashMap;

/// Format a single ContentItem node with synthetic children support
fn format_content_item(
    item: &ContentItem,
    indent_level: usize,
    include_all: bool,
    show_linum: bool,
) -> String {
    let mut output = String::new();
    let indent = "  ".repeat(indent_level);
    let tag = to_tag_name(item.node_type());
    let range_attr = if show_linum {
        format!(
            " range=\"{}:{}\"",
            item.range().start.line + 1,
            item.range().start.column + 1
        )
    } else {
        String::new()
    };

    output.push_str(&format!("{indent}<{tag}{range_attr}>"));
    output.push_str(&escape_xml(&item.display_label()));

    // Collect special field children (not in Container trait)
    let mut special_children = Vec::new();

    // Handle include_all: only genuine special cases (fields not in children())
    if include_all {
        match item {
            ContentItem::Session(s) => {
                // Show session annotations (field, not in children())
                for ann in &s.annotations {
                    special_children.push(SpecialChild::Annotation(Box::new(ann.clone())));
                }
            }
            ContentItem::ListItem(li) => {
                // Show marker as special child (field, not in children())
                special_children.push(SpecialChild::Marker(li.marker.as_string().to_string()));

                // Show text content (field, not in children())
                for text_part in &li.text {
                    special_children.push(SpecialChild::Text(text_part.as_string().to_string()));
                }

                // Show list item annotations (field, not in children())
                for ann in &li.annotations {
                    special_children.push(SpecialChild::Annotation(Box::new(ann.clone())));
                }
            }
            ContentItem::Definition(d) => {
                // Show definition annotations (field, not in children())
                for ann in &d.annotations {
                    special_children.push(SpecialChild::Annotation(Box::new(ann.clone())));
                }
            }
            ContentItem::Annotation(a) => {
                // Show parameters (field, not in children())
                for param in &a.data.parameters {
                    special_children.push(SpecialChild::Parameter(
                        param.key.clone(),
                        param.value.clone(),
                    ));
                }
            }
            _ => {}
        }
    }

    // Handle VerbatimBlock specially (has groups, not simple children)
    if let ContentItem::VerbatimBlock(v) = item {
        if v.group_len() > 0 {
            output.push('\n');
            for (idx, group) in v.group().enumerate() {
                let group_label = if v.group_len() == 1 {
                    group.subject.as_string().to_string()
                } else {
                    format!(
                        "{} (group {} of {})",
                        group.subject.as_string(),
                        idx + 1,
                        v.group_len()
                    )
                };

                output.push_str(&format!(
                    "{}  <verbatim-group>{}\n",
                    indent,
                    escape_xml(&group_label)
                ));
                for child in group.children.iter() {
                    output.push_str(&format_content_item(
                        child,
                        indent_level + 2,
                        include_all,
                        show_linum,
                    ));
                }
                output.push_str(&format!("{indent}  </verbatim-group>\n"));
            }
            output.push_str(&format!("{indent}</{tag}>\n"));
            return output;
        }
    }

    // Get regular children - some types use Container trait, others have special fields
    let regular_children: &[ContentItem] = match item {
        ContentItem::Paragraph(p) => &p.lines,
        ContentItem::List(l) => &l.items,
        _ => {
            if let Some(container) = try_as_container(item) {
                container.children()
            } else {
                &[]
            }
        }
    };

    // Determine if we have any children to render
    let has_children = !special_children.is_empty() || !regular_children.is_empty();

    if !has_children {
        output.push_str(&format!("</{tag}>\n"));
    } else {
        output.push('\n');

        // Render special field children
        for special in special_children {
            match special {
                SpecialChild::Marker(marker) => {
                    output.push_str(&format!(
                        "{}  <marker>{}</marker>\n",
                        indent,
                        escape_xml(&marker)
                    ));
                }
                SpecialChild::Text(text) => {
                    output.push_str(&format!("{}  <text>{}</text>\n", indent, escape_xml(&text)));
                }
                SpecialChild::Parameter(key, value) => {
                    output.push_str(&format!(
                        "{}  <parameter>{}={}</parameter>\n",
                        indent,
                        escape_xml(&key),
                        escape_xml(&value)
                    ));
                }
                SpecialChild::Annotation(ann) => {
                    let ann_item = ContentItem::Annotation(*ann);
                    output.push_str(&format_content_item(
                        &ann_item,
                        indent_level + 1,
                        include_all,
                        show_linum,
                    ));
                }
            }
        }

        // Render regular children
        for child in regular_children {
            output.push_str(&format_content_item(
                child,
                indent_level + 1,
                include_all,
                show_linum,
            ));
        }

        output.push_str(&format!("{indent}</{tag}>\n"));
    }

    output
}

/// Helper enum for special field children (not in Container trait)
enum SpecialChild {
    Marker(String),
    Text(String),
    Parameter(String, String),
    Annotation(Box<lex_core::lex::ast::Annotation>),
}

/// Convert a node type name to a tag name (e.g., "TextLine" → "text-line")
fn to_tag_name(node_type: &str) -> String {
    let mut tag = String::new();
    for (i, c) in node_type.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            tag.push('-');
        }
        tag.push(c.to_lowercase().next().unwrap());
    }
    tag
}

/// Serialize a document to AST tag format
pub fn serialize_document(doc: &Document) -> String {
    serialize_document_with_params(doc, &HashMap::new())
}

/// Serialize a document to AST tag format with optional parameters
///
/// # Parameters
///
/// - `"ast-full"`: When set to `"true"`, includes all AST node properties:
///   * Document-level annotations (shown with `<annotation>` tags)
///   * Session titles (as `<session-title>` nodes)
///   * List item markers and text (as `<marker>` and `<text>` nodes)
///   * Definition subjects (as `<subject>` nodes)
///   * Annotation labels and parameters (as `<label>` and `<parameter>` nodes)
///
/// # Examples
///
/// ```ignore
/// use std::collections::HashMap;
///
/// // Normal view (content only)
/// let output = serialize_document_with_params(&doc, &HashMap::new());
///
/// // Full AST view (all properties)
/// let mut params = HashMap::new();
/// params.insert("ast-full".to_string(), "true".to_string());
/// let output = serialize_document_with_params(&doc, &params);
/// ```
pub fn serialize_document_with_params(doc: &Document, params: &HashMap<String, String>) -> String {
    // Check if ast-full parameter is set to true
    let include_all = params
        .get("ast-full")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    let show_linum = params
        .get("show-linum")
        .map(|v| v != "false")
        .unwrap_or(false);

    let mut result = String::new();
    result.push_str("<document>\n");

    // If include_all, show document-level annotations
    if include_all {
        for annotation in &doc.annotations {
            let ann_item = ContentItem::Annotation(annotation.clone());
            result.push_str(&format_content_item(&ann_item, 1, include_all, show_linum));
        }
    }

    // Show document children (flattened from root session)
    for child in &doc.root.children {
        result.push_str(&format_content_item(child, 1, include_all, show_linum));
    }

    result.push_str("</document>");
    result
}

/// Escape XML special characters
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&apos;")
}

/// Format implementation for XML-like tag format
pub struct TagFormat;

impl Format for TagFormat {
    fn name(&self) -> &str {
        "tag"
    }

    fn description(&self) -> &str {
        "XML-like tag format with hierarchical structure"
    }

    fn file_extensions(&self) -> &[&str] {
        &["tag", "xml"]
    }

    fn supports_serialization(&self) -> bool {
        true
    }

    fn serialize(&self, doc: &Document) -> Result<String, FormatError> {
        Ok(serialize_document(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::ast::elements::typed_content;
    use lex_core::lex::ast::{ContentItem, Paragraph, Session, TextContent};

    #[test]
    fn test_serialize_simple_paragraph() {
        let doc = Document::with_content(vec![ContentItem::Paragraph(Paragraph::from_line(
            "Hello world".to_string(),
        ))]);

        let result = serialize_document(&doc);
        assert!(result.contains("<document>"));
        assert!(result.contains("<paragraph>"));
        assert!(result.contains("Hello world"));
        assert!(result.contains("</paragraph>"));
        assert!(result.contains("</document>"));
    }

    #[test]
    fn test_serialize_session_with_paragraph() {
        let doc = Document::with_content(vec![ContentItem::Session(Session::new(
            TextContent::from_string("Introduction".to_string(), None),
            typed_content::into_session_contents(vec![ContentItem::Paragraph(
                Paragraph::from_line("Welcome".to_string()),
            )]),
        ))]);

        let result = serialize_document(&doc);
        assert!(result.contains("<session>Introduction"));
        assert!(result.contains("<paragraph>"));
        assert!(result.contains("Welcome"));
        assert!(result.contains("</paragraph>"));
        assert!(result.contains("</session>"));
    }

    #[test]
    fn test_serialize_nested_sessions() {
        let doc = Document::with_content(vec![ContentItem::Session(Session::new(
            TextContent::from_string("Root".to_string(), None),
            typed_content::into_session_contents(vec![
                ContentItem::Paragraph(Paragraph::from_line("Para 1".to_string())),
                ContentItem::Session(Session::new(
                    TextContent::from_string("Nested".to_string(), None),
                    typed_content::into_session_contents(vec![ContentItem::Paragraph(
                        Paragraph::from_line("Nested para".to_string()),
                    )]),
                )),
            ]),
        ))]);

        let result = serialize_document(&doc);
        assert!(result.contains("<session>Root"));
        assert!(result.contains("<paragraph>"));
        assert!(result.contains("Para 1"));
        assert!(result.contains("<session>Nested"));
        assert!(result.contains("Nested para"));
    }

    #[test]
    fn test_xml_escaping() {
        let doc = Document::with_content(vec![ContentItem::Paragraph(Paragraph::from_line(
            "Text with <special> & \"chars\"".to_string(),
        ))]);

        let result = serialize_document(&doc);
        assert!(result.contains("&lt;special&gt;"));
        assert!(result.contains("&amp;"));
        assert!(result.contains("&quot;"));
    }

    #[test]
    fn test_empty_session() {
        let doc = Document::with_content(vec![ContentItem::Session(Session::with_title(
            "Empty".to_string(),
        ))]);

        let result = serialize_document(&doc);
        assert!(result.contains("<session>Empty</session>"));
    }

    #[test]
    fn test_serialize_simple_list() {
        use lex_core::lex::ast::{List, ListItem};

        let doc = Document::with_content(vec![ContentItem::List(List::new(vec![
            ListItem::new("-".to_string(), "First item".to_string()),
            ListItem::new("-".to_string(), "Second item".to_string()),
        ]))]);

        let result = serialize_document(&doc);
        assert!(result.contains("<list>"));
        assert!(result.contains("<list-item>First item</list-item>"));
        assert!(result.contains("<list-item>Second item</list-item>"));
        assert!(result.contains("</list>"));
    }

    #[test]
    fn test_format_trait() {
        let format = TagFormat;
        assert_eq!(format.name(), "tag");
        assert!(format.supports_serialization());
        assert!(!format.supports_parsing());

        let doc = Document::with_content(vec![ContentItem::Paragraph(Paragraph::from_line(
            "Test".to_string(),
        ))]);

        let result = format.serialize(&doc);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Test"));
    }
}
