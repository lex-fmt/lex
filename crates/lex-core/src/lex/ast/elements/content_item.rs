//! Content item
//!
//! `ContentItem` is the common wrapper for all elements that can
//! appear in document content. It lets tooling operate uniformly on
//! mixed structures (paragraphs, sessions, lists, definitions, etc.).
//!
//! Examples:
//! - A session containing paragraphs and a list
//! - A paragraph followed by a definition and an annotation

use super::super::range::{Position, Range};
use super::super::traits::{AstNode, Container, Visitor, VisualStructure};
use super::annotation::Annotation;
use super::blank_line_group::BlankLineGroup;
use super::definition::Definition;
use super::list::{List, ListItem};
use super::paragraph::{Paragraph, TextLine};
use super::session::Session;
use super::verbatim::Verbatim;
use super::verbatim_line::VerbatimLine;
use std::fmt;

/// ContentItem represents any element that can appear in document content
#[derive(Debug, Clone, PartialEq)]
pub enum ContentItem {
    Paragraph(Paragraph),
    Session(Session),
    List(List),
    ListItem(ListItem),
    TextLine(TextLine),
    Definition(Definition),
    Annotation(Annotation),
    VerbatimBlock(Box<Verbatim>),
    VerbatimLine(VerbatimLine),
    BlankLineGroup(BlankLineGroup),
}

impl AstNode for ContentItem {
    fn node_type(&self) -> &'static str {
        match self {
            ContentItem::Paragraph(p) => p.node_type(),
            ContentItem::Session(s) => s.node_type(),
            ContentItem::List(l) => l.node_type(),
            ContentItem::ListItem(li) => li.node_type(),
            ContentItem::TextLine(tl) => tl.node_type(),
            ContentItem::Definition(d) => d.node_type(),
            ContentItem::Annotation(a) => a.node_type(),
            ContentItem::VerbatimBlock(fb) => fb.node_type(),
            ContentItem::VerbatimLine(fl) => fl.node_type(),
            ContentItem::BlankLineGroup(blg) => blg.node_type(),
        }
    }

    fn display_label(&self) -> String {
        match self {
            ContentItem::Paragraph(p) => p.display_label(),
            ContentItem::Session(s) => s.display_label(),
            ContentItem::List(l) => l.display_label(),
            ContentItem::ListItem(li) => li.display_label(),
            ContentItem::TextLine(tl) => tl.display_label(),
            ContentItem::Definition(d) => d.display_label(),
            ContentItem::Annotation(a) => a.display_label(),
            ContentItem::VerbatimBlock(fb) => fb.display_label(),
            ContentItem::VerbatimLine(fl) => fl.display_label(),
            ContentItem::BlankLineGroup(blg) => blg.display_label(),
        }
    }

    fn range(&self) -> &Range {
        match self {
            ContentItem::Paragraph(p) => p.range(),
            ContentItem::Session(s) => s.range(),
            ContentItem::List(l) => l.range(),
            ContentItem::ListItem(li) => li.range(),
            ContentItem::TextLine(tl) => tl.range(),
            ContentItem::Definition(d) => d.range(),
            ContentItem::Annotation(a) => a.range(),
            ContentItem::VerbatimBlock(fb) => fb.range(),
            ContentItem::VerbatimLine(fl) => fl.range(),
            ContentItem::BlankLineGroup(blg) => blg.range(),
        }
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        match self {
            ContentItem::Paragraph(p) => p.accept(visitor),
            ContentItem::Session(s) => s.accept(visitor),
            ContentItem::List(l) => l.accept(visitor),
            ContentItem::ListItem(li) => li.accept(visitor),
            ContentItem::TextLine(tl) => tl.accept(visitor),
            ContentItem::Definition(d) => d.accept(visitor),
            ContentItem::Annotation(a) => a.accept(visitor),
            ContentItem::VerbatimBlock(fb) => fb.accept(visitor),
            ContentItem::VerbatimLine(fl) => fl.accept(visitor),
            ContentItem::BlankLineGroup(blg) => blg.accept(visitor),
        }
    }
}

impl VisualStructure for ContentItem {
    fn is_source_line_node(&self) -> bool {
        match self {
            ContentItem::Paragraph(p) => p.is_source_line_node(),
            ContentItem::Session(s) => s.is_source_line_node(),
            ContentItem::List(l) => l.is_source_line_node(),
            ContentItem::ListItem(li) => li.is_source_line_node(),
            ContentItem::TextLine(tl) => tl.is_source_line_node(),
            ContentItem::Definition(d) => d.is_source_line_node(),
            ContentItem::Annotation(a) => a.is_source_line_node(),
            ContentItem::VerbatimBlock(fb) => fb.is_source_line_node(),
            ContentItem::VerbatimLine(fl) => fl.is_source_line_node(),
            ContentItem::BlankLineGroup(blg) => blg.is_source_line_node(),
        }
    }

    fn has_visual_header(&self) -> bool {
        match self {
            ContentItem::Paragraph(p) => p.has_visual_header(),
            ContentItem::Session(s) => s.has_visual_header(),
            ContentItem::List(l) => l.has_visual_header(),
            ContentItem::ListItem(li) => li.has_visual_header(),
            ContentItem::TextLine(tl) => tl.has_visual_header(),
            ContentItem::Definition(d) => d.has_visual_header(),
            ContentItem::Annotation(a) => a.has_visual_header(),
            ContentItem::VerbatimBlock(fb) => fb.has_visual_header(),
            ContentItem::VerbatimLine(fl) => fl.has_visual_header(),
            ContentItem::BlankLineGroup(blg) => blg.has_visual_header(),
        }
    }

    fn collapses_with_children(&self) -> bool {
        match self {
            ContentItem::Paragraph(p) => p.collapses_with_children(),
            ContentItem::Session(s) => s.collapses_with_children(),
            ContentItem::List(l) => l.collapses_with_children(),
            ContentItem::ListItem(li) => li.collapses_with_children(),
            ContentItem::TextLine(tl) => tl.collapses_with_children(),
            ContentItem::Definition(d) => d.collapses_with_children(),
            ContentItem::Annotation(a) => a.collapses_with_children(),
            ContentItem::VerbatimBlock(fb) => fb.collapses_with_children(),
            ContentItem::VerbatimLine(fl) => fl.collapses_with_children(),
            ContentItem::BlankLineGroup(blg) => blg.collapses_with_children(),
        }
    }
}

impl ContentItem {
    pub fn label(&self) -> Option<&str> {
        match self {
            ContentItem::Session(s) => Some(s.label()),
            ContentItem::Definition(d) => Some(d.label()),
            ContentItem::Annotation(a) => Some(a.label()),
            ContentItem::ListItem(li) => Some(li.label()),
            ContentItem::VerbatimBlock(fb) => Some(fb.subject.as_string()),
            _ => None,
        }
    }

    pub fn children(&self) -> Option<&[ContentItem]> {
        match self {
            ContentItem::Session(s) => Some(&s.children),
            ContentItem::Definition(d) => Some(&d.children),
            ContentItem::Annotation(a) => Some(&a.children),
            ContentItem::List(l) => Some(&l.items),
            ContentItem::ListItem(li) => Some(&li.children),
            ContentItem::Paragraph(p) => Some(&p.lines),
            ContentItem::VerbatimBlock(fb) => Some(&fb.children),
            ContentItem::TextLine(_) => None,
            ContentItem::VerbatimLine(_) => None,
            _ => None,
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<ContentItem>> {
        match self {
            ContentItem::Session(s) => Some(s.children.as_mut_vec()),
            ContentItem::Definition(d) => Some(d.children.as_mut_vec()),
            ContentItem::Annotation(a) => Some(a.children.as_mut_vec()),
            ContentItem::List(l) => Some(l.items.as_mut_vec()),
            ContentItem::ListItem(li) => Some(li.children.as_mut_vec()),
            ContentItem::Paragraph(p) => Some(&mut p.lines),
            ContentItem::VerbatimBlock(fb) => Some(fb.children.as_mut_vec()),
            ContentItem::TextLine(_) => None,
            ContentItem::VerbatimLine(_) => None,
            _ => None,
        }
    }

    pub fn text(&self) -> Option<String> {
        match self {
            ContentItem::Paragraph(p) => Some(p.text()),
            _ => None,
        }
    }

    pub fn is_paragraph(&self) -> bool {
        matches!(self, ContentItem::Paragraph(_))
    }
    pub fn is_session(&self) -> bool {
        matches!(self, ContentItem::Session(_))
    }
    pub fn is_list(&self) -> bool {
        matches!(self, ContentItem::List(_))
    }
    pub fn is_list_item(&self) -> bool {
        matches!(self, ContentItem::ListItem(_))
    }
    pub fn is_text_line(&self) -> bool {
        matches!(self, ContentItem::TextLine(_))
    }
    pub fn is_definition(&self) -> bool {
        matches!(self, ContentItem::Definition(_))
    }
    pub fn is_annotation(&self) -> bool {
        matches!(self, ContentItem::Annotation(_))
    }
    pub fn is_verbatim_block(&self) -> bool {
        matches!(self, ContentItem::VerbatimBlock(_))
    }

    pub fn is_verbatim_line(&self) -> bool {
        matches!(self, ContentItem::VerbatimLine(_))
    }

    pub fn is_blank_line_group(&self) -> bool {
        matches!(self, ContentItem::BlankLineGroup(_))
    }

    pub fn as_paragraph(&self) -> Option<&Paragraph> {
        if let ContentItem::Paragraph(p) = self {
            Some(p)
        } else {
            None
        }
    }
    pub fn as_session(&self) -> Option<&Session> {
        if let ContentItem::Session(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn as_list(&self) -> Option<&List> {
        if let ContentItem::List(l) = self {
            Some(l)
        } else {
            None
        }
    }
    pub fn as_list_item(&self) -> Option<&ListItem> {
        if let ContentItem::ListItem(li) = self {
            Some(li)
        } else {
            None
        }
    }
    pub fn as_definition(&self) -> Option<&Definition> {
        if let ContentItem::Definition(d) = self {
            Some(d)
        } else {
            None
        }
    }
    pub fn as_annotation(&self) -> Option<&Annotation> {
        if let ContentItem::Annotation(a) = self {
            Some(a)
        } else {
            None
        }
    }
    pub fn as_verbatim_block(&self) -> Option<&Verbatim> {
        if let ContentItem::VerbatimBlock(fb) = self {
            Some(fb)
        } else {
            None
        }
    }

    pub fn as_verbatim_line(&self) -> Option<&VerbatimLine> {
        if let ContentItem::VerbatimLine(fl) = self {
            Some(fl)
        } else {
            None
        }
    }

    pub fn as_blank_line_group(&self) -> Option<&BlankLineGroup> {
        if let ContentItem::BlankLineGroup(blg) = self {
            Some(blg)
        } else {
            None
        }
    }

    pub fn as_paragraph_mut(&mut self) -> Option<&mut Paragraph> {
        if let ContentItem::Paragraph(p) = self {
            Some(p)
        } else {
            None
        }
    }
    pub fn as_session_mut(&mut self) -> Option<&mut Session> {
        if let ContentItem::Session(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn as_list_mut(&mut self) -> Option<&mut List> {
        if let ContentItem::List(l) = self {
            Some(l)
        } else {
            None
        }
    }
    pub fn as_list_item_mut(&mut self) -> Option<&mut ListItem> {
        if let ContentItem::ListItem(li) = self {
            Some(li)
        } else {
            None
        }
    }
    pub fn as_definition_mut(&mut self) -> Option<&mut Definition> {
        if let ContentItem::Definition(d) = self {
            Some(d)
        } else {
            None
        }
    }
    pub fn as_annotation_mut(&mut self) -> Option<&mut Annotation> {
        if let ContentItem::Annotation(a) = self {
            Some(a)
        } else {
            None
        }
    }
    pub fn as_verbatim_block_mut(&mut self) -> Option<&mut Verbatim> {
        if let ContentItem::VerbatimBlock(fb) = self {
            Some(fb)
        } else {
            None
        }
    }

    pub fn as_verbatim_line_mut(&mut self) -> Option<&mut VerbatimLine> {
        if let ContentItem::VerbatimLine(fl) = self {
            Some(fl)
        } else {
            None
        }
    }

    pub fn as_blank_line_group_mut(&mut self) -> Option<&mut BlankLineGroup> {
        if let ContentItem::BlankLineGroup(blg) = self {
            Some(blg)
        } else {
            None
        }
    }

    /// Find the deepest element at the given position in this item and its children
    /// Returns the deepest (most nested) element that contains the position
    pub fn element_at(&self, pos: Position) -> Option<&ContentItem> {
        // Check nested items first - even if parent location doesn't contain position,
        // nested elements might. This is important because parent locations (like sessions)
        // may only cover their title, not their nested content.
        if let Some(children) = self.children() {
            for child in children {
                if let Some(result) = child.element_at(pos) {
                    return Some(result); // Return deepest element found
                }
            }
        }

        // Now, check the current item. An item is considered to be at the position if its
        // location contains the position.
        // If nested elements were found, they would have been returned above.
        // If no nested results were found, this item is the deepest element at the position.
        if self.range().contains(pos) {
            Some(self)
        } else {
            None
        }
    }

    /// Find the visual line element at the given position
    ///
    /// Returns the element representing a source line (TextLine, ListItem, VerbatimLine,
    /// BlankLineGroup). For container elements with headers (Session, Definition, Annotation,
    /// VerbatimBlock), it returns the deepest line element, not the container itself.
    pub fn visual_line_at(&self, pos: Position) -> Option<&ContentItem> {
        // First, check children for visual line nodes (depth-first search)
        if let Some(children) = self.children() {
            for child in children {
                if let Some(result) = child.visual_line_at(pos) {
                    return Some(result);
                }
            }
        }

        // If no children matched, check if this item is a true line-level visual node
        // (not a container with a header)
        let is_line_level = matches!(
            self,
            ContentItem::TextLine(_)
                | ContentItem::ListItem(_)
                | ContentItem::VerbatimLine(_)
                | ContentItem::BlankLineGroup(_)
        );

        if is_line_level && self.range().contains(pos) {
            Some(self)
        } else {
            None
        }
    }

    /// Find the block element at the given position
    ///
    /// Returns the shallowest block-level container element (Session, Definition, List,
    /// Paragraph, Annotation, VerbatimBlock) that contains the position. This skips
    /// line-level elements and returns the containing structural block.
    pub fn block_element_at(&self, pos: Position) -> Option<&ContentItem> {
        // Check if this is a block element that contains the position
        let is_block = matches!(
            self,
            ContentItem::Session(_)
                | ContentItem::Definition(_)
                | ContentItem::List(_)
                | ContentItem::Paragraph(_)
                | ContentItem::Annotation(_)
                | ContentItem::VerbatimBlock(_)
        );

        if is_block && self.range().contains(pos) {
            return Some(self);
        }

        // If not a block element, check children
        if let Some(children) = self.children() {
            for child in children {
                if let Some(result) = child.block_element_at(pos) {
                    return Some(result);
                }
            }
        }

        None
    }

    /// Find the path of nodes at the given position, starting from this item
    /// Returns a vector of nodes [self, child, grandchild, ...]
    pub fn node_path_at_position(&self, pos: Position) -> Vec<&ContentItem> {
        // Check nested items first
        if let Some(children) = self.children() {
            for child in children {
                let mut path = child.node_path_at_position(pos);
                if !path.is_empty() {
                    path.insert(0, self);
                    return path;
                }
            }
        }

        // If no children matched, check if this item contains the position
        if self.range().contains(pos) {
            vec![self]
        } else {
            Vec::new()
        }
    }

    /// Recursively iterate all descendants of this node (depth-first pre-order)
    /// Does not include the node itself, only its descendants
    pub fn descendants(&self) -> Box<dyn Iterator<Item = &ContentItem> + '_> {
        if let Some(children) = self.children() {
            Box::new(
                children
                    .iter()
                    .flat_map(|child| std::iter::once(child).chain(child.descendants())),
            )
        } else {
            Box::new(std::iter::empty())
        }
    }

    /// Recursively iterate all descendants with their relative depth
    /// Depth is relative to this node (direct children have depth 0, their children have depth 1, etc.)
    pub fn descendants_with_depth(
        &self,
        start_depth: usize,
    ) -> Box<dyn Iterator<Item = (&ContentItem, usize)> + '_> {
        if let Some(children) = self.children() {
            Box::new(children.iter().flat_map(move |child| {
                std::iter::once((child, start_depth))
                    .chain(child.descendants_with_depth(start_depth + 1))
            }))
        } else {
            Box::new(std::iter::empty())
        }
    }
}

impl fmt::Display for ContentItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContentItem::Paragraph(p) => write!(f, "Paragraph({} lines)", p.lines.len()),
            ContentItem::Session(s) => {
                write!(
                    f,
                    "Session('{}', {} items)",
                    s.title.as_string(),
                    s.children.len()
                )
            }
            ContentItem::List(l) => write!(f, "List({} items)", l.items.len()),
            ContentItem::ListItem(li) => {
                write!(f, "ListItem('{}', {} items)", li.text(), li.children.len())
            }
            ContentItem::TextLine(tl) => {
                write!(f, "TextLine('{}')", tl.text())
            }
            ContentItem::Definition(d) => {
                write!(
                    f,
                    "Definition('{}', {} items)",
                    d.subject.as_string(),
                    d.children.len()
                )
            }
            ContentItem::Annotation(a) => write!(
                f,
                "Annotation('{}', {} params, {} items)",
                a.data.label.value,
                a.data.parameters.len(),
                a.children.len()
            ),
            ContentItem::VerbatimBlock(fb) => {
                write!(f, "VerbatimBlock('{}')", fb.subject.as_string())
            }
            ContentItem::VerbatimLine(fl) => {
                write!(f, "VerbatimLine('{}')", fl.content.as_string())
            }
            ContentItem::BlankLineGroup(blg) => write!(f, "{blg}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::range::{Position, Range};
    use super::super::paragraph::Paragraph;
    use super::*;
    use crate::lex::ast::elements::typed_content;

    #[test]
    fn test_element_at_simple_paragraph() {
        let para = Paragraph::from_line("Test".to_string()).at(Range::new(
            0..0,
            Position::new(0, 0),
            Position::new(0, 4),
        ));
        let item = ContentItem::Paragraph(para);

        let pos = Position::new(0, 2);
        if let Some(result) = item.element_at(pos) {
            // Should return the deepest element, which is the TextLine
            assert!(result.is_text_line());
        } else {
            panic!("Expected to find element at position");
        }
    }

    #[test]
    fn test_element_at_position_outside_location() {
        let para = Paragraph::from_line("Test".to_string()).at(Range::new(
            0..0,
            Position::new(0, 0),
            Position::new(0, 4),
        ));
        let item = ContentItem::Paragraph(para);

        let pos = Position::new(0, 10);
        let result = item.element_at(pos);
        assert!(result.is_none());
    }

    #[test]
    fn test_element_at_no_location() {
        // Item with no location should not match any position
        let para = Paragraph::from_line("Test".to_string());
        let item = ContentItem::Paragraph(para);

        let pos = Position::new(5, 10);
        assert!(item.element_at(pos).is_none());
    }

    #[test]
    fn test_element_at_nested_session() {
        let para = Paragraph::from_line("Nested".to_string()).at(Range::new(
            0..0,
            Position::new(1, 0),
            Position::new(1, 6),
        ));
        let session = Session::new(
            super::super::super::text_content::TextContent::from_string(
                "Section".to_string(),
                None,
            ),
            typed_content::into_session_contents(vec![ContentItem::Paragraph(para)]),
        )
        .at(Range::new(0..0, Position::new(0, 0), Position::new(2, 0)));
        let item = ContentItem::Session(session);

        let pos = Position::new(1, 3);
        if let Some(result) = item.element_at(pos) {
            // Should return the deepest element, which is the TextLine
            assert!(result.is_text_line());
        } else {
            panic!("Expected to find deepest element");
        }
    }

    #[test]
    fn test_descendants_on_session_content_item() {
        let mut inner_session = Session::with_title("Inner".to_string());
        inner_session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Grandchild".to_string(),
            )));

        let mut session = Session::with_title("Outer".to_string());
        session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Child".to_string(),
            )));
        session.children.push(ContentItem::Session(inner_session));

        let item = ContentItem::Session(session);
        let descendants: Vec<_> = item.descendants().collect();
        assert_eq!(descendants.len(), 5);

        let paragraphs: Vec<_> = item.descendants().filter(|d| d.is_paragraph()).collect();
        assert_eq!(paragraphs.len(), 2);
    }

    #[test]
    fn element_at_prefers_child_even_if_parent_range_is_tight() {
        // Session range stops before the child paragraph range, but we should still find the child.
        let paragraph = Paragraph::from_line("Child".to_string()).at(Range::new(
            10..15,
            Position::new(1, 0),
            Position::new(1, 5),
        ));

        let mut session = Session::with_title("Header".to_string()).at(Range::new(
            0..6,
            Position::new(0, 0),
            Position::new(0, 6),
        ));
        session.children.push(ContentItem::Paragraph(paragraph));

        let pos = Position::new(1, 3);
        let item = ContentItem::Session(session);
        let result = item
            .element_at(pos)
            .expect("child paragraph should be discoverable");

        assert!(result.is_text_line());
    }

    #[test]
    fn descendants_with_depth_tracks_depths() {
        let paragraph =
            ContentItem::Paragraph(Paragraph::from_line("Para".to_string()).at(Range::new(
                0..4,
                Position::new(0, 0),
                Position::new(0, 4),
            )));

        let list_item = ListItem::with_content("-".to_string(), "Item".to_string(), vec![])
            .at(Range::new(5..9, Position::new(1, 0), Position::new(1, 4)));
        let list = ContentItem::List(List::new(vec![list_item]));

        let mut session = Session::with_title("Root".to_string());
        session.children.push(paragraph.clone());
        session.children.push(list.clone());

        let depths: Vec<(&str, usize)> = ContentItem::Session(session)
            .descendants_with_depth(0)
            .map(|(item, depth)| (item.node_type(), depth))
            .collect();

        assert_eq!(
            depths,
            vec![
                ("Paragraph", 0),
                ("TextLine", 1),
                ("List", 0),
                ("ListItem", 1),
            ]
        );
    }

    #[test]
    fn test_visual_line_at_finds_text_line() {
        // Create a paragraph with a text line
        let para = Paragraph::from_line("Test line".to_string()).at(Range::new(
            0..9,
            Position::new(0, 0),
            Position::new(0, 9),
        ));
        let item = ContentItem::Paragraph(para);

        let pos = Position::new(0, 5);
        let result = item.visual_line_at(pos);
        assert!(result.is_some());
        assert!(result.unwrap().is_text_line());
    }

    #[test]
    fn test_visual_line_at_finds_list_item() {
        let list_item = ListItem::with_content("-".to_string(), "Item text".to_string(), vec![])
            .at(Range::new(0..10, Position::new(0, 0), Position::new(0, 10)));
        let list = List::new(vec![list_item]).at(Range::new(
            0..10,
            Position::new(0, 0),
            Position::new(0, 10),
        ));
        let item = ContentItem::List(list);

        let pos = Position::new(0, 5);
        let result = item.visual_line_at(pos);
        assert!(result.is_some());
        assert!(result.unwrap().is_list_item());
    }

    #[test]
    fn test_visual_line_at_position_outside() {
        let para = Paragraph::from_line("Test".to_string()).at(Range::new(
            0..4,
            Position::new(0, 0),
            Position::new(0, 4),
        ));
        let item = ContentItem::Paragraph(para);

        let pos = Position::new(10, 10);
        let result = item.visual_line_at(pos);
        assert!(result.is_none());
    }

    #[test]
    fn test_block_element_at_finds_paragraph() {
        let para = Paragraph::from_line("Test".to_string()).at(Range::new(
            0..4,
            Position::new(0, 0),
            Position::new(0, 4),
        ));
        let item = ContentItem::Paragraph(para);

        let pos = Position::new(0, 2);
        let result = item.block_element_at(pos);
        assert!(result.is_some());
        assert!(result.unwrap().is_paragraph());
    }

    #[test]
    fn test_block_element_at_finds_session() {
        let mut session = Session::with_title("Section".to_string()).at(Range::new(
            0..10,
            Position::new(0, 0),
            Position::new(2, 0),
        ));
        session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Content".to_string(),
            )));
        let item = ContentItem::Session(session);

        let pos = Position::new(1, 0);
        let result = item.block_element_at(pos);
        assert!(result.is_some());
        // Should find the Session, not descend into nested elements
        assert!(result.unwrap().is_session());
    }

    #[test]
    fn test_block_element_at_skips_text_line() {
        // When called on a nested structure with TextLine, should return the Paragraph
        let para = Paragraph::from_line("Test".to_string()).at(Range::new(
            0..4,
            Position::new(0, 0),
            Position::new(0, 4),
        ));

        let mut session = Session::with_title("Section".to_string()).at(Range::new(
            0..10,
            Position::new(0, 0),
            Position::new(2, 0),
        ));
        session.children.push(ContentItem::Paragraph(para));
        let item = ContentItem::Session(session);

        let pos = Position::new(0, 2);
        let result = item.block_element_at(pos);
        assert!(result.is_some());
        // Should return Session, the first block element encountered
        assert!(result.unwrap().is_session());
    }

    #[test]
    fn test_block_element_at_position_outside() {
        let para = Paragraph::from_line("Test".to_string()).at(Range::new(
            0..4,
            Position::new(0, 0),
            Position::new(0, 4),
        ));
        let item = ContentItem::Paragraph(para);

        let pos = Position::new(10, 10);
        let result = item.block_element_at(pos);
        assert!(result.is_none());
    }

    #[test]
    fn test_comparison_element_at_vs_visual_line_at_vs_block_element_at() {
        // Build a structure: Session > Paragraph > TextLine
        let para = Paragraph::from_line("Test line".to_string()).at(Range::new(
            5..14,
            Position::new(1, 0),
            Position::new(1, 9),
        ));

        let mut session = Session::with_title("Title".to_string()).at(Range::new(
            0..14,
            Position::new(0, 0),
            Position::new(1, 9),
        ));
        session.children.push(ContentItem::Paragraph(para));
        let item = ContentItem::Session(session);

        let pos = Position::new(1, 5);

        // element_at should return the deepest element (TextLine)
        let deepest = item.element_at(pos);
        assert!(deepest.is_some());
        assert!(deepest.unwrap().is_text_line());

        // visual_line_at should also return TextLine (it's a visual line node)
        let visual = item.visual_line_at(pos);
        assert!(
            visual.is_some(),
            "visual_line_at should find a visual line element"
        );
        let visual_item = visual.unwrap();
        assert!(
            visual_item.is_text_line(),
            "Expected TextLine but got: {:?}",
            visual_item.node_type()
        );

        // block_element_at should return the Session (first block element)
        let block = item.block_element_at(pos);
        assert!(block.is_some());
        assert!(block.unwrap().is_session());
    }
}
