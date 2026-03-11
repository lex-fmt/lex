//! List element
//!
//! A list is an ordered collection of items, each with its own text
//! and optional nested content. Lists can be used to structure
//! outlines, steps, or bullet points.
//!
//! Lists must have a minimum of 2 items.  And it's not ilegal to have mixed decorations in a list, as the parser will consider the first item's decoration to set the list type. The ordering doesn't have to be correct, as lists itself are ordered, they are just a marker, but tooling will order them under demand.
//!
//! Parsing Structure:
//!
//! Nested List (with content):
//! | Element | Prec. Blank | Head     | Blank | Content | Tail   |
//! |---------|-------------|----------|-------|---------|--------|
//! | List    | Optional    | ListLine | No    | Yes     | dedent |
//!
//! Flat List (no nested content):
//! | Element | Prec. Blank | Head     | Tail                |
//! |---------|-------------|----------|---------------------|
//! | List    | Yes         | ListLine | BlankLine or Dedent |
//!
//! Special Cases:
//! - Two Item Minimum: A list must have 2+ items, otherwise it's a paragraph
//! - Dialog Rule: Lines starting with "-" can be marked as dialog (paragraphs) rather than list items
//!
//! Examples:
//!    A flat list with the plain decoration:
//!         - Bread
//!         - Milk
//! They can be nested, and have other styles:
//!    1. Groceries
//!        - Bread
//!        - Milk
//!
//!Formalize Sequence Decoration Styles
//!        List items have two parts, their markers , that determines they are a list item, and their
//!  text content. These appear together in the source text (possible even without space to separate
//! them, perfectly legal), but they are different things, and the actual text for list items does
//!not include the marker.
//!         1. Presentation
//!           
//!          List items can be presented in various dimensions.
//!           - Decoration Styles:
//!             - Plain , a dash -
//!             - Numerical 3
//!             - Roman Numerals IV
//!             - Alphabetical b
//!           - Separators:
//!             - Periods , as in 3.
//!             - Parenthesis as in b)
//!             - Double parenthesis  as in (c)
//!           - Forms:
//!             - Forms only matter for deeper than 1 level items.
//!             - Short: only that level , as in 3.
//!             - Extender: full nested index , as in 1.3.5
//!
// !          Note that all of these can be mixed, for example: 1.b.iii is a valid sequence item as
//! in 1.b)v)
//!
//!
//!        2. Parsing Rules
//!
//!          The list decoration  style, separator and form are determined by the first list item in
//! that list's (same level). Nested lists can have multiple characteristics, and at each level the
//! first of it's items will determine the lists.
//!          Note that these are taken as the authors choice for presentation, they are in no way
//! used to validate or invalidate lists. A list with items in a nonsensical order and mixed
//! presentation is a perfectly valid lists. Tooling such as formatters or publishing tools will often
//! order items correctly on exports. This is a feature actually, as it's easier to same number all
//! items while editing, else on insertions and swaps you must reorder all subsequent items.
//!          The presentation characteristics are a list property, as they are set for the entire list,
//!  even if the source text did not do it consistently. And they are to be used on any ast -> string
//! representation to form the sequence marker (the full combination of all presentation traits).
//!         In list items we do keep the source marker version for recreation capacity, and
//! formatters and other tools are not to be expected to use them.
//!
//!
//! Learn More:
//! - Lists spec: specs/v1/elements/list.lex
//! - Labels (used by annotations in lists): specs/v1/elements/label.lex
//! - Parameters (used by annotations in lists): specs/v1/elements/parameter.lex

use super::super::range::{Position, Range};
use super::super::text_content::TextContent;
use super::super::traits::AstNode;
use super::super::traits::Container;
use super::super::traits::Visitor;
use super::super::traits::VisualStructure;
use super::annotation::Annotation;
use super::container::{GeneralContainer, ListContainer};
use super::content_item::ContentItem;
use super::typed_content::{ContentElement, ListContent};
use std::fmt;

/// A list contains multiple list items
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub items: ListContainer,
    pub marker: Option<super::sequence_marker::SequenceMarker>,
    pub annotations: Vec<Annotation>,
    pub location: Range,
}

/// A list item has a marker, body text, and optional nested content
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    pub marker: TextContent,
    pub text: Vec<TextContent>,
    pub children: GeneralContainer,
    pub annotations: Vec<Annotation>,
    pub location: Range,
}

impl List {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }
    pub fn new(items: Vec<ListItem>) -> Self {
        let typed_items = items
            .into_iter()
            .map(ListContent::ListItem)
            .collect::<Vec<_>>();
        Self {
            items: ListContainer::from_typed(typed_items),
            marker: None,
            annotations: Vec::new(),
            location: Self::default_location(),
        }
    }

    /// Preferred builder
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// Annotations attached to this list as a whole.
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Mutable access to list annotations.
    pub fn annotations_mut(&mut self) -> &mut Vec<Annotation> {
        &mut self.annotations
    }

    /// Iterate over annotation blocks on the list element.
    pub fn iter_annotations(&self) -> std::slice::Iter<'_, Annotation> {
        self.annotations.iter()
    }

    /// Iterate over all content items nested inside list-level annotations.
    pub fn iter_annotation_contents(&self) -> impl Iterator<Item = &ContentItem> {
        self.annotations
            .iter()
            .flat_map(|annotation| annotation.children())
    }

    /// Lists currently have no standalone header; this always returns `None`.
    pub fn header_location(&self) -> Option<&Range> {
        None
    }

    /// Bounding range covering only the list items.
    pub fn body_location(&self) -> Option<Range> {
        Range::bounding_box(self.items.iter().map(|item| item.range()))
    }
}

impl AstNode for List {
    fn node_type(&self) -> &'static str {
        "List"
    }
    fn display_label(&self) -> String {
        format!("{} items", self.items.len())
    }
    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        visitor.visit_list(self);
        super::super::traits::visit_children(visitor, &self.items);
        visitor.leave_list(self);
    }
}

impl VisualStructure for List {
    fn collapses_with_children(&self) -> bool {
        true
    }
}

impl fmt::Display for List {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "List({} items)", self.items.len())
    }
}

impl ListItem {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }
    pub fn new(marker: String, text: String) -> Self {
        Self::with_content(marker, text, Vec::new())
    }
    pub fn with_content(marker: String, text: String, children: Vec<ContentElement>) -> Self {
        Self::with_text_content(
            TextContent::from_string(marker, None),
            TextContent::from_string(text, None),
            children,
        )
    }
    /// Create a ListItem with TextContent that may have location information
    pub fn with_text_content(
        marker: TextContent,
        text_content: TextContent,
        children: Vec<ContentElement>,
    ) -> Self {
        Self {
            marker,
            text: vec![text_content],
            children: GeneralContainer::from_typed(children),
            annotations: Vec::new(),
            location: Self::default_location(),
        }
    }

    /// Preferred builder
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }
    pub fn text(&self) -> &str {
        self.text[0].as_string()
    }

    pub fn marker(&self) -> &str {
        self.marker.as_string()
    }

    /// Annotations attached to this list item.
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Mutable access to list-item annotations.
    pub fn annotations_mut(&mut self) -> &mut Vec<Annotation> {
        &mut self.annotations
    }

    /// Iterate annotation blocks associated with this list item.
    pub fn iter_annotations(&self) -> std::slice::Iter<'_, Annotation> {
        self.annotations.iter()
    }

    /// Iterate all content items nested inside the list item's annotations.
    pub fn iter_annotation_contents(&self) -> impl Iterator<Item = &ContentItem> {
        self.annotations
            .iter()
            .flat_map(|annotation| annotation.children())
    }
}

impl AstNode for ListItem {
    fn node_type(&self) -> &'static str {
        "ListItem"
    }
    fn display_label(&self) -> String {
        let text = self.text().trim();
        if text.chars().count() > 50 {
            format!("{}…", text.chars().take(50).collect::<String>())
        } else {
            text.to_string()
        }
    }
    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        visitor.visit_list_item(self);
        super::super::traits::visit_children(visitor, &self.children);
        visitor.leave_list_item(self);
    }
}

impl VisualStructure for ListItem {
    fn is_source_line_node(&self) -> bool {
        true
    }
}

impl Container for ListItem {
    fn label(&self) -> &str {
        self.text[0].as_string()
    }
    fn children(&self) -> &[ContentItem] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<ContentItem> {
        self.children.as_mut_vec()
    }
}

impl fmt::Display for ListItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ListItem('{}')", self.text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::elements::label::Label;
    use crate::lex::ast::elements::paragraph::Paragraph;
    use crate::lex::ast::elements::typed_content::ContentElement;
    use crate::lex::ast::Data;

    #[test]
    fn test_list() {
        let location = super::super::super::range::Range::new(
            0..0,
            super::super::super::range::Position::new(1, 0),
            super::super::super::range::Position::new(1, 10),
        );
        let list = List::new(vec![]).at(location.clone());
        assert_eq!(list.location, location);
    }

    #[test]
    fn test_list_body_location() {
        let item_range = Range::new(5..10, Position::new(1, 0), Position::new(1, 5));
        let item = ListItem::with_text_content(
            TextContent::from_string("-".to_string(), Some(item_range.clone())),
            TextContent::from_string("Item".to_string(), Some(item_range.clone())),
            Vec::new(),
        )
        .at(item_range.clone());

        let list =
            List::new(vec![item]).at(Range::new(0..15, Position::new(0, 0), Position::new(2, 0)));

        assert!(list.header_location().is_none());
        assert_eq!(list.body_location().unwrap().span, item_range.span);
    }

    #[test]
    fn list_body_location_spans_multiple_items_and_empty_list() {
        let item1_range = Range::new(0..5, Position::new(0, 0), Position::new(0, 5));
        let item2_range = Range::new(10..14, Position::new(1, 0), Position::new(1, 4));

        let item1 = ListItem::with_text_content(
            TextContent::from_string("-".to_string(), Some(item1_range.clone())),
            TextContent::from_string("One".to_string(), Some(item1_range.clone())),
            Vec::new(),
        )
        .at(item1_range.clone());
        let item2 = ListItem::with_text_content(
            TextContent::from_string("-".to_string(), Some(item2_range.clone())),
            TextContent::from_string("Two".to_string(), Some(item2_range.clone())),
            Vec::new(),
        )
        .at(item2_range.clone());

        let list = List::new(vec![item1.clone(), item2.clone()]);
        let body = list
            .body_location()
            .expect("expected bounding box for items");

        assert_eq!(body.span.start, item1_range.span.start);
        assert_eq!(body.span.end, item2_range.span.end);

        let empty_list = List::new(vec![]);
        assert!(empty_list.body_location().is_none());
    }

    #[test]
    fn list_annotation_iteration_exposes_children() {
        let child = ContentItem::Paragraph(Paragraph::from_line("note".to_string()));
        let annotation = Annotation::from_data(
            Data::new(Label::new("meta".into()), Vec::new()),
            vec![ContentElement::try_from(child).unwrap()],
        );

        let mut list = List::new(vec![ListItem::new("-".into(), "Item".into())]);
        list.annotations.push(annotation.clone());

        let contents: Vec<&ContentItem> = list.iter_annotation_contents().collect();
        assert_eq!(contents.len(), 1);

        let mut item = ListItem::new("-".into(), "Item".into());
        item.annotations.push(annotation);
        let item_contents: Vec<&ContentItem> = item.iter_annotation_contents().collect();
        assert_eq!(item_contents.len(), 1);
    }

    #[test]
    fn display_label_truncates_long_text() {
        let long_text = "x".repeat(60);
        let item = ListItem::new("-".into(), long_text.clone());

        let label = item.display_label();
        assert!(label.ends_with("…"));
        assert!(label.chars().count() < long_text.chars().count());

        let short = ListItem::new("-".into(), "short".into());
        assert_eq!(short.display_label(), "short");
    }

    mod sequence_marker_integration {
        use super::*;
        use crate::lex::ast::elements::{DecorationStyle, Form, Separator};
        use crate::lex::ast::ContentItem;
        use crate::lex::loader::DocumentLoader;

        #[test]
        fn parse_extracts_plain_marker() {
            let source = "- Item one\n- Item two";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let list = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::List(list) = item {
                        Some(list)
                    } else {
                        None
                    }
                })
                .expect("expected list");

            assert!(list.marker.is_some());
            let marker = list.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Plain);
            assert_eq!(marker.separator, Separator::Period);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "-");
        }

        #[test]
        fn parse_extracts_numerical_period_marker() {
            let source = "1. First item\n2. Second item";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let list = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::List(list) = item {
                        Some(list)
                    } else {
                        None
                    }
                })
                .expect("expected list");

            assert!(list.marker.is_some());
            let marker = list.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Numerical);
            assert_eq!(marker.separator, Separator::Period);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "1.");
        }

        #[test]
        fn parse_extracts_numerical_paren_marker() {
            let source = "1) First item\n2) Second item";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let list = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::List(list) = item {
                        Some(list)
                    } else {
                        None
                    }
                })
                .expect("expected list");

            assert!(list.marker.is_some());
            let marker = list.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Numerical);
            assert_eq!(marker.separator, Separator::Parenthesis);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "1)");
        }

        #[test]
        fn parse_extracts_alphabetical_marker() {
            let source = "a. Alpha\nb. Beta";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let list = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::List(list) = item {
                        Some(list)
                    } else {
                        None
                    }
                })
                .expect("expected list");

            assert!(list.marker.is_some());
            let marker = list.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Alphabetical);
            assert_eq!(marker.separator, Separator::Period);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "a.");
        }

        #[test]
        fn parse_extracts_roman_marker() {
            let source = "I. First\nII. Second";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let list = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::List(list) = item {
                        Some(list)
                    } else {
                        None
                    }
                })
                .expect("expected list");

            assert!(list.marker.is_some());
            let marker = list.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Roman);
            assert_eq!(marker.separator, Separator::Period);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "I.");
        }

        #[test]
        fn parse_extracts_extended_numerical_marker() {
            let source = "1.2.3 Item\n1.2.4 Item";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let list = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::List(list) = item {
                        Some(list)
                    } else {
                        None
                    }
                })
                .expect("expected list");

            assert!(list.marker.is_some());
            let marker = list.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Numerical);
            assert_eq!(marker.separator, Separator::Period);
            assert_eq!(marker.form, Form::Extended);
            assert_eq!(marker.raw_text.as_string(), "1.2.3");
        }

        #[test]
        fn parse_extracts_double_paren_marker() {
            let source = "(1) Item one\n(2) Item two";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let list = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::List(list) = item {
                        Some(list)
                    } else {
                        None
                    }
                })
                .expect("expected list");

            assert!(list.marker.is_some());
            let marker = list.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Numerical);
            assert_eq!(marker.separator, Separator::DoubleParens);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "(1)");
        }

        #[test]
        fn empty_list_has_no_marker() {
            let list = List::new(vec![]);
            assert!(list.marker.is_none());
        }
    }
}
