//! Treeviz formatter for AST nodes
//!
//! Treeviz is a visual representation of the AST, design specifically for document trees.
//! It features a one line per node format, which enables quick scanning of the tree, and is specially
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

use crate::lex::ast::{snapshot_from_document, AstSnapshot, Document};
use std::collections::HashMap;

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        let mut truncated = s.chars().take(max_chars).collect::<String>();
        truncated.push_str("...");
        truncated
    } else {
        s.to_string()
    }
}

/// Get the icon for a node type
fn get_icon(node_type: &str) -> &'static str {
    match node_type {
        "Document" => "⧉",
        "Session" => "§",
        "Paragraph" => "¶",
        "TextLine" => "↵",
        "List" => "☰",
        "ListItem" => "•",
        "Definition" => "≔",
        "VerbatimBlock" => "𝒱",
        "Annotation" => "\"",
        _ => "○",
    }
}

/// Build treeviz output from an AstSnapshot
fn format_snapshot(
    snapshot: &AstSnapshot,
    prefix: &str,
    child_index: usize,
    child_count: usize,
    show_linum: bool,
) -> String {
    let mut output = String::new();

    let is_last = child_index == child_count - 1;
    let connector = if is_last { "└─" } else { "├─" };
    let icon = get_icon(&snapshot.node_type);
    let truncated_label = truncate(&snapshot.label, 30);

    let linum_prefix = if show_linum {
        format!("{:02} ", snapshot.range.start.line + 1)
    } else {
        String::new()
    };

    output.push_str(&format!(
        "{linum_prefix}{prefix}{connector} {icon} {truncated_label}\n"
    ));

    // Process children if any
    if !snapshot.children.is_empty() {
        let child_prefix = format!("{}{}", prefix, if is_last { "  " } else { "│ " });
        let child_count = snapshot.children.len();

        for (i, child) in snapshot.children.iter().enumerate() {
            output.push_str(&format_snapshot(
                child,
                &child_prefix,
                i,
                child_count,
                show_linum,
            ));
        }
    }

    output
}

fn format_document_snapshot(snapshot: &AstSnapshot, show_linum: bool) -> String {
    let icon = get_icon(&snapshot.node_type);
    let truncated_label = truncate(&snapshot.label, 30);
    let mut output = format!("{icon} {truncated_label}\n");

    if !snapshot.children.is_empty() {
        let child_count = snapshot.children.len();
        for (i, child) in snapshot.children.iter().enumerate() {
            output.push_str(&format_snapshot(child, "", i, child_count, show_linum));
        }
    }

    output
}

pub fn to_treeviz_str(doc: &Document) -> String {
    to_treeviz_str_with_params(doc, &HashMap::new())
}

pub fn to_treeviz_str_with_params(doc: &Document, params: &HashMap<String, String>) -> String {
    let show_linum = params
        .get("show-linum")
        .map(|v| v != "false")
        .unwrap_or(false);

    let snapshot = snapshot_from_document(doc);
    format_document_snapshot(&snapshot, show_linum)
}

/// Formatter implementation for treeviz format
pub struct TreevizFormatter;

impl crate::lex::formats::registry::Formatter for TreevizFormatter {
    fn name(&self) -> &str {
        "treeviz"
    }

    fn serialize(
        &self,
        doc: &Document,
    ) -> Result<String, crate::lex::formats::registry::FormatError> {
        Ok(to_treeviz_str(doc))
    }

    fn description(&self) -> &str {
        "Visual tree representation with indentation and Unicode icons"
    }
}
