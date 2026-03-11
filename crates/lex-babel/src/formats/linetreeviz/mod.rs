//! Line Tree Visualization - Collapsed tree representation
//!
//! This format provides the same visual tree structure as treeviz but collapses
//! homogeneous container nodes (Paragraph, List) with their children by showing
//! combined parent+child icons (e.g., ¶ ↵ for Paragraph/TextLine, ☰ • for List/ListItem).
//!
//! ## Example
//!
//! ```text
//! ⧉ Document (0 annotations, 2 items)
//! ├─ § Session Title
//! │ ├─ ¶ ↵ First line of paragraph
//! │ └─ ¶ ↵ Second line of paragraph
//! └─ ☰ • List item 1
//!   └─ ☰ • List item 2
//! ```
//!
//! ## Key Differences from treeviz
//!
//! - Collapses Paragraph containers with TextLine children (shows `¶ ↵` not separate nodes)
//! - Collapses List containers with ListItem children (shows `☰ •` not separate nodes)
//! - Uses VisualStructure trait to identify collapsible containers
//! - Shares icon mapping with treeviz

use super::icons::get_icon;
use crate::error::FormatError;
use crate::format::Format;
use lex_core::lex::ast::traits::{AstNode, Container, VisualStructure};
use lex_core::lex::ast::{ContentItem, Document};
use std::collections::HashMap;

/// Format a single ContentItem node with collapsing logic
fn format_content_item(
    item: &ContentItem,
    prefix: &str,
    child_index: usize,
    child_count: usize,
    show_linum: bool,
) -> String {
    let mut output = String::new();
    let is_last = child_index == child_count - 1;
    let connector = if is_last { "└─" } else { "├─" };

    // Check if this node collapses with its children using the VisualStructure trait
    let collapses = match item {
        ContentItem::Paragraph(p) => p.collapses_with_children(),
        ContentItem::List(l) => l.collapses_with_children(),
        ContentItem::Session(s) => s.collapses_with_children(),
        ContentItem::Definition(d) => d.collapses_with_children(),
        ContentItem::Annotation(a) => a.collapses_with_children(),
        ContentItem::VerbatimBlock(v) => v.collapses_with_children(),
        _ => false,
    };

    if collapses {
        // Get parent info
        let parent_icon = get_icon(item.node_type());
        let children: Vec<&dyn AstNode> = match item {
            ContentItem::Paragraph(p) => p.lines.iter().map(|l| l as &dyn AstNode).collect(),
            ContentItem::List(l) => l.items.iter().map(|i| i as &dyn AstNode).collect(),
            _ => Vec::new(),
        };

        // Show children with combined parent+child icons, using the parent's connector
        for (i, child) in children.iter().enumerate() {
            let child_is_last = i == children.len() - 1;
            let child_icon = get_icon(child.node_type());

            // For the first child, use the parent's connector; for subsequent children get indented
            if i == 0 {
                let linum_prefix = if show_linum {
                    format!("{:02} ", child.range().start.line + 1)
                } else {
                    String::new()
                };

                output.push_str(&format!(
                    "{}{}{} {} {} {}\n",
                    linum_prefix,
                    prefix,
                    connector,
                    parent_icon,
                    child_icon,
                    child.display_label()
                ));
            } else {
                // Subsequent children get indented with the parent's continuation
                let child_prefix = format!("{}{}", prefix, if is_last { "  " } else { "│ " });
                let child_connector = if child_is_last { "└─" } else { "├─" };
                let linum_prefix = if show_linum {
                    format!("{:02} ", child.range().start.line + 1)
                } else {
                    String::new()
                };

                output.push_str(&format!(
                    "{}{}{} {} {} {}\n",
                    linum_prefix,
                    child_prefix,
                    child_connector,
                    parent_icon,
                    child_icon,
                    child.display_label()
                ));
            }

            // Process grandchildren if any (for nested structures within collapsed items)
            // For now, we don't handle this case as TextLine and basic ListItem don't have children
        }
    } else {
        // Normal node - show as usual
        let icon = get_icon(item.node_type());
        let linum_prefix = if show_linum {
            format!("{:02} ", item.range().start.line + 1)
        } else {
            String::new()
        };

        output.push_str(&format!(
            "{}{}{} {} {}\n",
            linum_prefix,
            prefix,
            connector,
            icon,
            item.display_label()
        ));

        // Process children
        let children = match item {
            ContentItem::Session(s) => s.children(),
            ContentItem::Definition(d) => d.children(),
            ContentItem::ListItem(li) => li.children(),
            ContentItem::Annotation(a) => a.children(),
            _ => &[],
        };

        if !children.is_empty() {
            let child_prefix = format!("{}{}", prefix, if is_last { "  " } else { "│ " });
            for (i, child) in children.iter().enumerate() {
                output.push_str(&format_content_item(
                    child,
                    &child_prefix,
                    i,
                    children.len(),
                    show_linum,
                ));
            }
        }
    }

    output
}

/// Convert a document to linetreeviz string
pub fn to_linetreeviz_str(doc: &Document) -> String {
    to_linetreeviz_str_with_params(doc, &HashMap::new())
}

/// Convert a document to linetreeviz string with optional parameters
///
/// # Parameters
///
/// - `"ast-full"`: When set to `"true"`, includes all AST node properties
///   Note: Currently this parameter is not fully implemented for linetreeviz
pub fn to_linetreeviz_str_with_params(doc: &Document, params: &HashMap<String, String>) -> String {
    let show_linum = params
        .get("show-linum")
        .map(|v| v != "false")
        .unwrap_or(false);

    let icon = get_icon("Document");
    let mut output = format!(
        "{} Document ({} annotations, {} items)\n",
        icon,
        doc.annotations.len(),
        doc.root.children.len()
    );

    let children = &doc.root.children;
    for (i, child) in children.iter().enumerate() {
        output.push_str(&format_content_item(
            child,
            "",
            i,
            children.len(),
            show_linum,
        ));
    }

    output
}

/// Format implementation for line tree visualization
pub struct LinetreevizFormat;

impl Format for LinetreevizFormat {
    fn name(&self) -> &str {
        "linetreeviz"
    }

    fn description(&self) -> &str {
        "Tree visualization with collapsed containers (Paragraph/List)"
    }

    fn file_extensions(&self) -> &[&str] {
        &["linetree"]
    }

    fn supports_serialization(&self) -> bool {
        true
    }

    fn supports_parsing(&self) -> bool {
        false
    }

    fn serialize(&self, doc: &Document) -> Result<String, FormatError> {
        Ok(to_linetreeviz_str(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icon_mapping() {
        assert_eq!(get_icon("Session"), "§");
        assert_eq!(get_icon("TextLine"), "↵");
        assert_eq!(get_icon("ListItem"), "•");
        assert_eq!(get_icon("Definition"), "≔");
        assert_eq!(get_icon("Paragraph"), "¶");
        assert_eq!(get_icon("List"), "☰");
    }
}
