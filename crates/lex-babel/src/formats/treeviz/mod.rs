//! Treeviz formatter for AST nodes
//!
//! Treeviz is a visual representation of the AST, design specifically for document trees.
//! It features a visual tree and line based output. For a version that matches, each line to source line, see the ./linetreeviz module.
//! helpful for formats that are primarely line oriented (like text).
//!
//! It encodes the node structure as indentation, with 2 white spaces per level of nesting.
//!
//! So the format is :
//! <indentation>(per level) <icon><space><label> (truncated to 30 characters)
//!
//! Example: (truncation not withstanding)
//!
//!   ¶ This is a two-lined para…
// │    ↵ This is a two-lined pa…
// │    ↵ First, a simple defini…
// │  ≔ Root Definition
// │    ¶ This definition contai…
// │      ↵ This definition cont…
// │    ☰ 2 items
// │      • - Item 1 in definiti…
// │      • - Item 2 in definiti…
// │  ¶ This is a marker annotat…
// │    ↵ This is a marker annot…
// │  § 1. Primary Session {{ses…
// │    ¶ This session acts as t…
// │      ↵ This session acts as…

//! Icons
//!     Core elements:
//!         Document: ⧉
//!         Session: §
//!         SessionTitle: ⊤
//!         Annotation: '"'
//!         Paragraph: ¶
//!         List: ☰
//!         ListItem: •
//!         Verbatim: 𝒱
//!         ForeingLine: ℣
//!         Definition: ≔
//!     Container elements:
//!         SessionContainer: Ψ
//!         ContentContainer: ➔
//!         Content: ⊤
//!     Spans:
//!         Text: ◦
//!         TextLine: ↵
//!     Inlines (not yet implemented, leave here for now)
//!         Italic: 𝐼
//!         Bold: 𝐁
//!         Code: ƒ
//!         Math (not yet implemented, leave here for now)
//!         Math: √
//!     References (not yet implemented, leave here for now)
//!         Reference: ⊕
//!         ReferenceFile: /
//!         ReferenceCitation: †
//!         ReferenceCitationAuthor: "@"
//!         ReferenceCitationPage: ◫
//!         ReferenceToCome: ⋯
//!         ReferenceUnknown: ∅
//!         ReferenceFootnote: ³
//!         ReferenceSession: #

use super::icons::get_icon;
use crate::error::FormatError;
use crate::format::Format;
use lex_core::lex::ast::trait_helpers::try_as_container;
use lex_core::lex::ast::traits::{AstNode, Container, VisualStructure};
use lex_core::lex::ast::{ContentItem, Document};
use std::collections::HashMap;

/// Format a single ContentItem node
fn format_content_item(
    item: &ContentItem,
    prefix: &str,
    child_index: usize,
    child_count: usize,
    include_all: bool,
    show_linum: bool,
) -> String {
    let mut output = String::new();
    let is_last = child_index == child_count - 1;
    let connector = if is_last { "└─" } else { "├─" };
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

    let child_prefix = format!("{}{}", prefix, if is_last { "  " } else { "│ " });

    // Handle include_all: show visual headers using traits
    if include_all {
        if item.has_visual_header() {
            if let Some(container) = try_as_container(item) {
                let header = container.label();
                // Use the parent node's icon for the header (no synthetic type needed)
                let header_icon = get_icon(item.node_type());
                output.push_str(&format!("{child_prefix}├─ {header_icon} {header}\n"));
            }
        }

        // Handle special cases that need more than just the header
        match item {
            ContentItem::Session(s) => {
                // Show session annotations
                for (i, ann) in s.annotations.iter().enumerate() {
                    let ann_item = ContentItem::Annotation(ann.clone());
                    output.push_str(&format_content_item(
                        &ann_item,
                        &child_prefix,
                        i + 1,
                        s.annotations.len() + s.children().len(),
                        include_all,
                        show_linum,
                    ));
                }
            }
            ContentItem::ListItem(li) => {
                // Show marker as synthetic child
                let marker_icon = get_icon("Marker");
                output.push_str(&format!(
                    "{}├─ {} {}\n",
                    child_prefix,
                    marker_icon,
                    li.marker.as_string()
                ));

                // Show text content
                for (i, text_part) in li.text.iter().enumerate() {
                    let text_icon = get_icon("Text");
                    let connector = if i == li.text.len() - 1 && li.children().is_empty() {
                        "└─"
                    } else {
                        "├─"
                    };
                    output.push_str(&format!(
                        "{}{} {} {}\n",
                        child_prefix,
                        connector,
                        text_icon,
                        text_part.as_string()
                    ));
                }

                // Show list item annotations
                for ann in &li.annotations {
                    let ann_item = ContentItem::Annotation(ann.clone());
                    output.push_str(&format_content_item(
                        &ann_item,
                        &child_prefix,
                        0,
                        1,
                        include_all,
                        show_linum,
                    ));
                }
            }
            ContentItem::Definition(d) => {
                // Show definition annotations
                for ann in &d.annotations {
                    let ann_item = ContentItem::Annotation(ann.clone());
                    output.push_str(&format_content_item(
                        &ann_item,
                        &child_prefix,
                        0,
                        1,
                        include_all,
                        show_linum,
                    ));
                }
            }
            ContentItem::Annotation(a) => {
                // Show parameters (label already shown by get_visual_header)
                for param in &a.data.parameters {
                    let param_icon = get_icon("Parameter");
                    output.push_str(&format!(
                        "{}├─ {} {}={}\n",
                        child_prefix, param_icon, param.key, param.value
                    ));
                }
            }
            _ => {}
        }
    }

    // Process regular children using Container trait
    match item {
        ContentItem::VerbatimBlock(v) => {
            // Handle verbatim groups
            let mut group_output = String::new();
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
                let group_icon = get_icon("VerbatimGroup");
                let is_last_group = idx == v.group_len() - 1;
                let group_connector = if is_last_group { "└─" } else { "├─" };

                group_output.push_str(&format!(
                    "{child_prefix}{group_connector} {group_icon} {group_label}\n"
                ));

                let group_child_prefix = format!(
                    "{}{}",
                    child_prefix,
                    if is_last_group { "  " } else { "│ " }
                );

                for (i, child) in group.children.iter().enumerate() {
                    group_output.push_str(&format_content_item(
                        child,
                        &group_child_prefix,
                        i,
                        group.children.len(),
                        include_all,
                        show_linum,
                    ));
                }
            }
            output + &group_output
        }
        _ => {
            // Use Container trait to get children for all other types
            if let Some(container) = try_as_container(item) {
                output
                    + &format_children(container.children(), &child_prefix, include_all, show_linum)
            } else {
                // Leaf nodes have no children
                output
            }
        }
    }
}

fn format_children(
    children: &[ContentItem],
    prefix: &str,
    include_all: bool,
    show_linum: bool,
) -> String {
    let mut output = String::new();
    let child_count = children.len();
    for (i, child) in children.iter().enumerate() {
        output.push_str(&format_content_item(
            child,
            prefix,
            i,
            child_count,
            include_all,
            show_linum,
        ));
    }
    output
}

pub fn to_treeviz_str(doc: &Document) -> String {
    to_treeviz_str_with_params(doc, &HashMap::new())
}

/// Convert a document to treeviz string with optional parameters
///
/// # Parameters
///
/// - `"ast-full"`: When set to `"true"`, includes all AST node properties:
///   * Document-level annotations
///   * Session titles (as SessionTitle nodes)
///   * List item markers and text (as Marker and Text nodes)
///   * Definition subjects (as Subject nodes)
///   * Annotation labels and parameters (as Label and Parameter nodes)
///
/// # Examples
///
/// ```ignore
/// use std::collections::HashMap;
///
/// // Normal view (content only)
/// let output = to_treeviz_str_with_params(&doc, &HashMap::new());
///
/// // Full AST view (all properties)
/// let mut params = HashMap::new();
/// params.insert("ast-full".to_string(), "true".to_string());
/// let output = to_treeviz_str_with_params(&doc, &params);
/// ```
pub fn to_treeviz_str_with_params(doc: &Document, params: &HashMap<String, String>) -> String {
    // Check if ast-full parameter is set to true
    let include_all = params
        .get("ast-full")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

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

    // If include_all, show document-level annotations
    if include_all {
        for annotation in &doc.annotations {
            let ann_item = ContentItem::Annotation(annotation.clone());
            output.push_str(&format_content_item(
                &ann_item,
                "",
                0,
                1,
                include_all,
                show_linum,
            ));
        }
    }

    // Show document children (flattened from root session)
    let children = &doc.root.children;
    output + &format_children(children, "", include_all, show_linum)
}

/// Format implementation for treeviz format
pub struct TreevizFormat;

impl Format for TreevizFormat {
    fn name(&self) -> &str {
        "treeviz"
    }

    fn description(&self) -> &str {
        "Visual tree representation with indentation and Unicode icons"
    }

    fn file_extensions(&self) -> &[&str] {
        &["tree", "treeviz"]
    }

    fn supports_serialization(&self) -> bool {
        true
    }

    fn serialize(&self, doc: &Document) -> Result<String, FormatError> {
        Ok(to_treeviz_str(doc))
    }
}
