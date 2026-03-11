//! Typed content variants for type-safe container construction
//!
//! This module provides specialized content types that enforce nesting rules at the type level.
//! Instead of using the universal ContentItem enum everywhere, containers can use these
//! restricted types to prevent invalid nesting at compile time.
//!
//! # Hierarchy
//!
//! ```text
//! ContentItem (universal)
//!   ├─ SessionContent (allows Sessions)
//!   │   ├─ Session
//!   │   └─ ContentElement
//!   └─ ContentElement (no Sessions/Annotations)
//!       ├─ Paragraph
//!       ├─ List
//!       ├─ Definition
//!       ├─ VerbatimBlock
//!       └─ ...
//!
//! ListContent (only ListItems)
//!   └─ ListItem
//!
//! VerbatimContent (only VerbatimLines)
//!   └─ VerbatimLine
//! ```
//!
//! # Usage
//!
//! ```ignore
//! // Convert ContentItem to typed variant
//! let content_item: ContentItem = /* ... */;
//! let element: ContentElement = content_item.try_into()?; // Rejects Sessions
//!
//! // Convert typed variant back to ContentItem
//! let item: ContentItem = element.into();
//! ```

use super::annotation::Annotation;
use super::blank_line_group::BlankLineGroup;
use super::content_item::ContentItem;
use super::definition::Definition;
use super::list::{List, ListItem};
use super::paragraph::{Paragraph, TextLine};
use super::session::Session;
use super::verbatim::Verbatim;
use super::verbatim_line::VerbatimLine;

// ============================================================================
// CONTENT ELEMENT (No Sessions or Annotations)
// ============================================================================

/// ContentElement represents all elements EXCEPT Sessions
///
/// Used by GeneralContainer (Definition.children, Annotation.children, ListItem.children)
/// to enforce that Sessions cannot be nested in these contexts.
/// Annotations ARE allowed in ContentElement.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentElement {
    Annotation(Annotation),
    Paragraph(Paragraph),
    List(List),
    Definition(Definition),
    VerbatimBlock(Box<Verbatim>),
    TextLine(TextLine),
    VerbatimLine(VerbatimLine),
    BlankLineGroup(BlankLineGroup),
}

impl TryFrom<ContentItem> for ContentElement {
    type Error = &'static str;

    fn try_from(item: ContentItem) -> Result<Self, Self::Error> {
        match item {
            ContentItem::Session(_) => Err("Sessions are not allowed in ContentElement"),
            ContentItem::Annotation(a) => Ok(ContentElement::Annotation(a)),
            ContentItem::Paragraph(p) => Ok(ContentElement::Paragraph(p)),
            ContentItem::List(l) => Ok(ContentElement::List(l)),
            ContentItem::Definition(d) => Ok(ContentElement::Definition(d)),
            ContentItem::VerbatimBlock(vb) => Ok(ContentElement::VerbatimBlock(vb)),
            ContentItem::TextLine(tl) => Ok(ContentElement::TextLine(tl)),
            ContentItem::VerbatimLine(vl) => Ok(ContentElement::VerbatimLine(vl)),
            ContentItem::BlankLineGroup(blg) => Ok(ContentElement::BlankLineGroup(blg)),
            ContentItem::ListItem(_) => Err("ListItem should not be used as ContentElement"),
        }
    }
}

impl From<ContentElement> for ContentItem {
    fn from(element: ContentElement) -> Self {
        match element {
            ContentElement::Annotation(a) => ContentItem::Annotation(a),
            ContentElement::Paragraph(p) => ContentItem::Paragraph(p),
            ContentElement::List(l) => ContentItem::List(l),
            ContentElement::Definition(d) => ContentItem::Definition(d),
            ContentElement::VerbatimBlock(vb) => ContentItem::VerbatimBlock(vb),
            ContentElement::TextLine(tl) => ContentItem::TextLine(tl),
            ContentElement::VerbatimLine(vl) => ContentItem::VerbatimLine(vl),
            ContentElement::BlankLineGroup(blg) => ContentItem::BlankLineGroup(blg),
        }
    }
}

// ============================================================================
// SESSION CONTENT (Includes Sessions)
// ============================================================================

/// SessionContent represents all elements including Sessions
///
/// Used by SessionContainer (Document.root, Session.children) where unlimited
/// Session nesting is allowed. Since Annotations are now part of ContentElement,
/// SessionContent only needs to distinguish Sessions from everything else.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionContent {
    Session(Session),
    Element(ContentElement),
}

impl From<ContentItem> for SessionContent {
    fn from(item: ContentItem) -> Self {
        match item {
            ContentItem::Session(s) => SessionContent::Session(s),
            // Annotations and everything else go through ContentElement
            other => match ContentElement::try_from(other) {
                Ok(element) => SessionContent::Element(element),
                Err(_) => unreachable!("All non-Session items should convert to ContentElement"),
            },
        }
    }
}

impl From<SessionContent> for ContentItem {
    fn from(content: SessionContent) -> Self {
        match content {
            SessionContent::Session(s) => ContentItem::Session(s),
            SessionContent::Element(e) => e.into(),
        }
    }
}

impl From<ContentElement> for SessionContent {
    fn from(element: ContentElement) -> Self {
        SessionContent::Element(element)
    }
}

// ============================================================================
// LIST CONTENT (Only ListItems)
// ============================================================================

/// ListContent represents only ListItem elements
///
/// Used by ListContainer (List.items) to enforce homogeneous list structure.
#[derive(Debug, Clone, PartialEq)]
pub enum ListContent {
    ListItem(ListItem),
}

impl TryFrom<ContentItem> for ListContent {
    type Error = &'static str;

    fn try_from(item: ContentItem) -> Result<Self, Self::Error> {
        match item {
            ContentItem::ListItem(li) => Ok(ListContent::ListItem(li)),
            _ => Err("Only ListItems are allowed in ListContent"),
        }
    }
}

impl From<ListContent> for ContentItem {
    fn from(content: ListContent) -> Self {
        match content {
            ListContent::ListItem(li) => ContentItem::ListItem(li),
        }
    }
}

// ============================================================================
// VERBATIM CONTENT (Only VerbatimLines)
// ============================================================================

/// VerbatimContent represents only VerbatimLine elements
///
/// Used by VerbatimContainer (VerbatimBlock.children) to enforce that verbatim
/// blocks only contain verbatim lines.
#[derive(Debug, Clone, PartialEq)]
pub enum VerbatimContent {
    VerbatimLine(VerbatimLine),
}

impl TryFrom<ContentItem> for VerbatimContent {
    type Error = &'static str;

    fn try_from(item: ContentItem) -> Result<Self, Self::Error> {
        match item {
            ContentItem::VerbatimLine(vl) => Ok(VerbatimContent::VerbatimLine(vl)),
            _ => Err("Only VerbatimLines are allowed in VerbatimContent"),
        }
    }
}

impl From<VerbatimContent> for ContentItem {
    fn from(content: VerbatimContent) -> Self {
        match content {
            VerbatimContent::VerbatimLine(vl) => ContentItem::VerbatimLine(vl),
        }
    }
}

// ============================================================================
// BATCH CONVERSION HELPERS
// ============================================================================

/// Convert a Vec<ContentItem> to Vec<ContentElement>, failing if any item is a Session or Annotation
pub fn try_into_content_elements(
    items: Vec<ContentItem>,
) -> Result<Vec<ContentElement>, &'static str> {
    items.into_iter().map(ContentElement::try_from).collect()
}

/// Convert a Vec<ContentItem> to Vec<SessionContent> (always succeeds)
pub fn into_session_contents(items: Vec<ContentItem>) -> Vec<SessionContent> {
    items.into_iter().map(SessionContent::from).collect()
}

/// Convert a Vec<ContentItem> to Vec<ListContent>, failing if any item is not a ListItem
pub fn try_into_list_contents(items: Vec<ContentItem>) -> Result<Vec<ListContent>, &'static str> {
    items.into_iter().map(ListContent::try_from).collect()
}

/// Convert a Vec<ContentItem> to Vec<VerbatimContent>, failing if any item is not a VerbatimLine
pub fn try_into_verbatim_contents(
    items: Vec<ContentItem>,
) -> Result<Vec<VerbatimContent>, &'static str> {
    items.into_iter().map(VerbatimContent::try_from).collect()
}

#[cfg(test)]
mod tests {
    use super::super::paragraph::Paragraph;
    use super::super::session::Session;
    use super::*;

    #[test]
    fn test_content_element_rejects_session() {
        let session = Session::with_title("Test".to_string());
        let item = ContentItem::Session(session);

        let result = ContentElement::try_from(item);
        assert!(result.is_err());
    }

    #[test]
    fn test_content_element_accepts_paragraph() {
        let para = Paragraph::from_line("Test".to_string());
        let item = ContentItem::Paragraph(para.clone());

        let result = ContentElement::try_from(item);
        assert!(result.is_ok());

        match result.unwrap() {
            ContentElement::Paragraph(p) => assert_eq!(p.text(), para.text()),
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_session_content_accepts_session() {
        let session = Session::with_title("Test".to_string());
        let item = ContentItem::Session(session.clone());

        let content = SessionContent::from(item);

        match content {
            SessionContent::Session(s) => assert_eq!(s.title.as_string(), "Test"),
            _ => panic!("Expected Session"),
        }
    }

    #[test]
    fn test_list_content_only_accepts_list_items() {
        let para = Paragraph::from_line("Test".to_string());
        let item = ContentItem::Paragraph(para);

        let result = ListContent::try_from(item);
        assert!(result.is_err());
    }

    #[test]
    fn test_batch_conversion_content_elements() {
        let para1 = Paragraph::from_line("Test 1".to_string());
        let para2 = Paragraph::from_line("Test 2".to_string());
        let items = vec![ContentItem::Paragraph(para1), ContentItem::Paragraph(para2)];

        let result = try_into_content_elements(items);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn test_batch_conversion_rejects_session() {
        let para = Paragraph::from_line("Test".to_string());
        let session = Session::with_title("Test".to_string());
        let items = vec![ContentItem::Paragraph(para), ContentItem::Session(session)];

        let result = try_into_content_elements(items);
        assert!(result.is_err());
    }
}
