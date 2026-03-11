//! Shared icon mapping for tree visualization formats
//!
//! This module provides a centralized icon mapping for all tree-based visualization
//! formats (treeviz, linetreeviz, etc.) to ensure consistency.

/// Get the Unicode icon for a given AST node type
///
/// Returns a single Unicode character that visually represents the node type.
/// These icons are used in tree visualization formats to provide quick visual
/// identification of node types.
pub fn get_icon(node_type: &str) -> &'static str {
    match node_type {
        "Document" => "⧉",
        "Session" => "§",
        "Paragraph" => "¶",
        "TextLine" => "↵",
        "List" => "☰",
        "ListItem" => "•",
        "Definition" => "≔",
        "VerbatimBlock" => "𝒱",
        "VerbatimLine" => "↵",
        "Annotation" => "\"",
        "BlankLineGroup" => "⎯",
        _ => "○",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icon_mappings() {
        assert_eq!(get_icon("Document"), "⧉");
        assert_eq!(get_icon("Session"), "§");
        assert_eq!(get_icon("Paragraph"), "¶");
        assert_eq!(get_icon("TextLine"), "↵");
        assert_eq!(get_icon("List"), "☰");
        assert_eq!(get_icon("ListItem"), "•");
        assert_eq!(get_icon("Definition"), "≔");
        assert_eq!(get_icon("VerbatimBlock"), "𝒱");
        assert_eq!(get_icon("VerbatimLine"), "↵");
        assert_eq!(get_icon("Annotation"), "\"");
        assert_eq!(get_icon("BlankLineGroup"), "⎯");
    }

    #[test]
    fn test_unknown_node_type() {
        assert_eq!(get_icon("UnknownType"), "○");
    }
}
