//! Trait-based helpers for AST node analysis
//!
//! This module provides utilities to leverage AST traits (Container, VisualStructure)
//! for extracting node information uniformly across formats.

use super::traits::{Container, VisualStructure};
use super::ContentItem;

/// Try to cast a ContentItem to a Container trait object
///
/// Returns Some(&dyn Container) if the node implements Container, None otherwise.
/// Useful for uniformly accessing label() and children() across different node types.
pub fn try_as_container(item: &ContentItem) -> Option<&dyn Container> {
    match item {
        ContentItem::Session(s) => Some(s as &dyn Container),
        ContentItem::Definition(d) => Some(d as &dyn Container),
        ContentItem::Annotation(a) => Some(a as &dyn Container),
        ContentItem::ListItem(li) => Some(li as &dyn Container),
        ContentItem::VerbatimBlock(v) => Some(v.as_ref() as &dyn Container),
        _ => None,
    }
}

/// Get the visual header label for a node using the Container trait
///
/// For nodes that have_visual_header() and implement Container, returns their label:
/// - Session → title
/// - Definition → subject
/// - Annotation → label value
/// - VerbatimBlock → subject
///
/// Returns None if the node doesn't have a visual header or doesn't implement Container.
pub fn get_visual_header(item: &ContentItem) -> Option<String> {
    if !item.has_visual_header() {
        return None;
    }

    try_as_container(item).map(|c| c.label().to_string())
}

/// Get regular children for any ContentItem
///
/// Returns the children slice for container nodes, handling special cases:
/// - Paragraph → lines (which are ContentItems wrapping TextLines)
/// - List → items (which are ContentItems wrapping ListItems)
/// - Other Container nodes → children()
/// - Leaf nodes → empty slice
pub fn get_children(item: &ContentItem) -> &[ContentItem] {
    match item {
        ContentItem::Session(s) => s.children(),
        ContentItem::Paragraph(p) => &p.lines,
        ContentItem::List(l) => &l.items,
        ContentItem::Definition(d) => d.children(),
        ContentItem::ListItem(li) => li.children(),
        ContentItem::Annotation(a) => a.children(),
        // VerbatimBlock needs special handling for groups, so return empty here
        ContentItem::VerbatimBlock(_) => &[],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::elements::{Paragraph, Session};

    #[test]
    fn test_try_as_container_session() {
        let session = Session::with_title("Test".to_string());
        let item = ContentItem::Session(session);

        assert!(try_as_container(&item).is_some());
        assert_eq!(try_as_container(&item).unwrap().label(), "Test");
    }

    #[test]
    fn test_try_as_container_paragraph() {
        let para = Paragraph::from_line("Test".to_string());
        let item = ContentItem::Paragraph(para);

        // Paragraph doesn't implement Container
        assert!(try_as_container(&item).is_none());
    }

    #[test]
    fn test_get_visual_header_session() {
        let session = Session::with_title("My Title".to_string());
        let item = ContentItem::Session(session);

        assert_eq!(get_visual_header(&item), Some("My Title".to_string()));
    }

    #[test]
    fn test_get_visual_header_paragraph() {
        let para = Paragraph::from_line("Text".to_string());
        let item = ContentItem::Paragraph(para);

        // Paragraph doesn't have visual header
        assert_eq!(get_visual_header(&item), None);
    }

    #[test]
    fn test_get_children_session() {
        let session = Session::with_title("Test".to_string());
        let item = ContentItem::Session(session);

        assert_eq!(get_children(&item).len(), 0);
    }

    #[test]
    fn test_get_children_paragraph() {
        let para = Paragraph::from_line("Test".to_string());
        let item = ContentItem::Paragraph(para);

        // Paragraph has one TextLine child
        assert_eq!(get_children(&item).len(), 1);
    }
}
