//! Session element
//!
//!     A session is the main structural element of lex documents. Sessions can be arbitrarily nested
//!     and contain required titles and content.
//!
//!     Sessions establish hierarchy within a document via their title and nested content, like all
//!     major elements in lex. The structure of the document is a tree of sessions, which can be
//!     nested arbitrarily. This creates powerful addressing capabilities as one can target any
//!     sub-session from an index.
//!
//! Structure:
//!
//!         - Title: short text identifying the session
//!         - Content: any elements allowed in the body (including other sessions for unlimited nesting)
//!
//!     The title can be any text content, and is often decorated with an ordering indicator, just
//!     like lists, and in lex all the numerical, alphabetical, and roman numeral indicators are
//!     supported.
//!
//! Parsing Rules
//!
//! Sessions follow this parsing pattern:
//!
//! | Element | Prec. Blank | Head          | Blank | Content | Tail   |
//! |---------|-------------|---------------|-------|---------|--------|
//! | Session | Yes         | ParagraphLine | Yes   | Yes     | dedent |
//!
//!     Sessions are unique in that the head must be enclosed by blank lines (both preceding and
//!     following). The reason this is significant is that it makes for a lot of complication in
//!     specific scenarios.
//!
//!     Consider the parsing of a session that is the very first element of its parent session. As
//!     it's the very first element, the preceding blank line is part of its parent session. It can
//!     see the following blank line before the paragraph just fine, as it belongs to it. But the
//!     first blank line is out of its reach.
//!
//!     The obvious solution would be to imperatively walk the tree up and check if the parent
//!     session has a preceding blank line. This works but this makes the grammar context sensitive,
//!     and now things are way more complicated, goodbye simple regular language parser.
//!
//!     The way this is handled is that we inject a synthetic token that represents the preceding
//!     blank line. This token is not produced by the logos lexer, but is created by the lexing
//!     pipeline to capture context information from parent to children elements so that parsing can
//!     be done in a regular single pass. As expected, this token is not consumed nor becomes a
//!     blank line node, but it's only used to decide on the parsing of the child elements.
//!
//!     For more details on how sessions fit into the AST structure and indentation model, see
//!     the [elements](crate::lex::ast::elements) module.
//!
//! Examples:
//!
//! Welcome to The Lex format
//!
//!     Lex is a plain text document format. ...
//!
//! 1.4 The Finale
//!
//!     Here is where we stop.
//!
use super::super::range::{Position, Range};
use super::super::text_content::TextContent;
use super::super::traits::{AstNode, Container, Visitor, VisualStructure};
use super::annotation::Annotation;
use super::container::SessionContainer;
use super::content_item::ContentItem;
use super::definition::Definition;
use super::list::{List, ListItem};
use super::paragraph::Paragraph;
use super::typed_content::SessionContent;
use super::verbatim::Verbatim;
use std::fmt;

/// A session represents a hierarchical container with a title
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub title: TextContent,
    pub marker: Option<super::sequence_marker::SequenceMarker>,
    pub children: SessionContainer,
    pub annotations: Vec<Annotation>,
    pub location: Range,
}

impl Session {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }
    pub fn new(title: TextContent, children: Vec<SessionContent>) -> Self {
        Self {
            title,
            marker: None,
            children: SessionContainer::from_typed(children),
            annotations: Vec::new(),
            location: Self::default_location(),
        }
    }
    pub fn with_title(title: String) -> Self {
        Self {
            title: TextContent::from_string(title, None),
            marker: None,
            children: SessionContainer::empty(),
            annotations: Vec::new(),
            location: Self::default_location(),
        }
    }

    /// Preferred builder
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// Annotations attached to this session header/content block.
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Range covering only the session title line, if available.
    pub fn header_location(&self) -> Option<&Range> {
        self.title.location.as_ref()
    }

    /// Bounding range covering only the session's children.
    pub fn body_location(&self) -> Option<Range> {
        Range::bounding_box(self.children.iter().map(|item| item.range()))
    }

    /// Get the title text without the sequence marker
    ///
    /// Returns the title with any leading sequence marker removed.
    /// For example, "1. Introduction" becomes "Introduction".
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let session = parse_session("1. Introduction:\n\n    Content");
    /// assert_eq!(session.title_text(), "Introduction");
    /// ```
    pub fn title_text(&self) -> &str {
        if let Some(marker) = &self.marker {
            let full_title = self.title.as_string();
            let marker_text = marker.as_str();

            // Find where the marker ends in the title
            if let Some(pos) = full_title.find(marker_text) {
                // Skip the marker and any whitespace after it
                let after_marker = &full_title[pos + marker_text.len()..];
                return after_marker.trim_start();
            }
        }

        // No marker, return the full title
        self.title.as_string()
    }

    /// Get the full title including any sequence marker
    ///
    /// Returns the complete title as it appears in the source, including
    /// any sequence marker prefix.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let session = parse_session("1. Introduction:\n\n    Content");
    /// assert_eq!(session.full_title(), "1. Introduction");
    /// ```
    pub fn full_title(&self) -> &str {
        self.title.as_string()
    }

    /// Mutable access to session annotations.
    pub fn annotations_mut(&mut self) -> &mut Vec<Annotation> {
        &mut self.annotations
    }

    /// Iterate over annotation blocks in source order.
    pub fn iter_annotations(&self) -> std::slice::Iter<'_, Annotation> {
        self.annotations.iter()
    }

    /// Iterate over all content items nested inside attached annotations.
    pub fn iter_annotation_contents(&self) -> impl Iterator<Item = &ContentItem> {
        self.annotations
            .iter()
            .flat_map(|annotation| annotation.children())
    }

    // ========================================================================
    // DELEGATION TO CONTAINER
    // All query/traversal methods delegate to the rich container implementation
    // ========================================================================

    /// Iterate over immediate content items
    pub fn iter_items(&self) -> impl Iterator<Item = &ContentItem> {
        self.children.iter()
    }

    /// Iterate over immediate paragraph children
    pub fn iter_paragraphs(&self) -> impl Iterator<Item = &Paragraph> {
        self.children.iter_paragraphs()
    }

    /// Iterate over immediate session children
    pub fn iter_sessions(&self) -> impl Iterator<Item = &Session> {
        self.children.iter_sessions()
    }

    /// Iterate over immediate list children
    pub fn iter_lists(&self) -> impl Iterator<Item = &List> {
        self.children.iter_lists()
    }

    /// Iterate over immediate verbatim block children
    pub fn iter_verbatim_blocks(&self) -> impl Iterator<Item = &Verbatim> {
        self.children.iter_verbatim_blocks()
    }

    /// Iterate all nodes in the session tree (depth-first pre-order traversal)
    pub fn iter_all_nodes(&self) -> Box<dyn Iterator<Item = &ContentItem> + '_> {
        self.children.iter_all_nodes()
    }

    /// Iterate all nodes with their depth (0 = immediate children)
    pub fn iter_all_nodes_with_depth(
        &self,
    ) -> Box<dyn Iterator<Item = (&ContentItem, usize)> + '_> {
        self.children.iter_all_nodes_with_depth()
    }

    /// Recursively iterate all paragraphs at any depth
    pub fn iter_paragraphs_recursive(&self) -> Box<dyn Iterator<Item = &Paragraph> + '_> {
        self.children.iter_paragraphs_recursive()
    }

    /// Recursively iterate all sessions at any depth
    pub fn iter_sessions_recursive(&self) -> Box<dyn Iterator<Item = &Session> + '_> {
        self.children.iter_sessions_recursive()
    }

    /// Recursively iterate all lists at any depth
    pub fn iter_lists_recursive(&self) -> Box<dyn Iterator<Item = &List> + '_> {
        self.children.iter_lists_recursive()
    }

    /// Recursively iterate all verbatim blocks at any depth
    pub fn iter_verbatim_blocks_recursive(&self) -> Box<dyn Iterator<Item = &Verbatim> + '_> {
        self.children.iter_verbatim_blocks_recursive()
    }

    /// Recursively iterate all list items at any depth
    pub fn iter_list_items_recursive(&self) -> Box<dyn Iterator<Item = &ListItem> + '_> {
        self.children.iter_list_items_recursive()
    }

    /// Recursively iterate all definitions at any depth
    pub fn iter_definitions_recursive(&self) -> Box<dyn Iterator<Item = &Definition> + '_> {
        self.children.iter_definitions_recursive()
    }

    /// Recursively iterate all annotations at any depth
    pub fn iter_annotations_recursive(&self) -> Box<dyn Iterator<Item = &Annotation> + '_> {
        self.children.iter_annotations_recursive()
    }

    /// Get the first paragraph (returns None if not found)
    pub fn first_paragraph(&self) -> Option<&Paragraph> {
        self.children.first_paragraph()
    }

    /// Get the first session (returns None if not found)
    pub fn first_session(&self) -> Option<&Session> {
        self.children.first_session()
    }

    /// Get the first list (returns None if not found)
    pub fn first_list(&self) -> Option<&List> {
        self.children.first_list()
    }

    /// Get the first definition (returns None if not found)
    pub fn first_definition(&self) -> Option<&Definition> {
        self.children.first_definition()
    }

    /// Get the first annotation (returns None if not found)
    pub fn first_annotation(&self) -> Option<&Annotation> {
        self.children.first_annotation()
    }

    /// Get the first verbatim block (returns None if not found)
    pub fn first_verbatim(&self) -> Option<&Verbatim> {
        self.children.first_verbatim()
    }

    /// Get the first paragraph, panicking if none found
    pub fn expect_paragraph(&self) -> &Paragraph {
        self.children.expect_paragraph()
    }

    /// Get the first session, panicking if none found
    pub fn expect_session(&self) -> &Session {
        self.children.expect_session()
    }

    /// Get the first list, panicking if none found
    pub fn expect_list(&self) -> &List {
        self.children.expect_list()
    }

    /// Get the first definition, panicking if none found
    pub fn expect_definition(&self) -> &Definition {
        self.children.expect_definition()
    }

    /// Get the first annotation, panicking if none found
    pub fn expect_annotation(&self) -> &Annotation {
        self.children.expect_annotation()
    }

    /// Get the first verbatim block, panicking if none found
    pub fn expect_verbatim(&self) -> &Verbatim {
        self.children.expect_verbatim()
    }

    /// Find all paragraphs matching a predicate
    pub fn find_paragraphs<F>(&self, predicate: F) -> Vec<&Paragraph>
    where
        F: Fn(&Paragraph) -> bool,
    {
        self.children.find_paragraphs(predicate)
    }

    /// Find all sessions matching a predicate
    pub fn find_sessions<F>(&self, predicate: F) -> Vec<&Session>
    where
        F: Fn(&Session) -> bool,
    {
        self.children.find_sessions(predicate)
    }

    /// Find all lists matching a predicate
    pub fn find_lists<F>(&self, predicate: F) -> Vec<&List>
    where
        F: Fn(&List) -> bool,
    {
        self.children.find_lists(predicate)
    }

    /// Find all definitions matching a predicate
    pub fn find_definitions<F>(&self, predicate: F) -> Vec<&Definition>
    where
        F: Fn(&Definition) -> bool,
    {
        self.children.find_definitions(predicate)
    }

    /// Find all annotations matching a predicate
    pub fn find_annotations<F>(&self, predicate: F) -> Vec<&Annotation>
    where
        F: Fn(&Annotation) -> bool,
    {
        self.children.find_annotations(predicate)
    }

    /// Find all nodes matching a generic predicate
    pub fn find_nodes<F>(&self, predicate: F) -> Vec<&ContentItem>
    where
        F: Fn(&ContentItem) -> bool,
    {
        self.children.find_nodes(predicate)
    }

    /// Find all nodes at a specific depth
    pub fn find_nodes_at_depth(&self, target_depth: usize) -> Vec<&ContentItem> {
        self.children.find_nodes_at_depth(target_depth)
    }

    /// Find all nodes within a depth range
    pub fn find_nodes_in_depth_range(
        &self,
        min_depth: usize,
        max_depth: usize,
    ) -> Vec<&ContentItem> {
        self.children
            .find_nodes_in_depth_range(min_depth, max_depth)
    }

    /// Find nodes at a specific depth matching a predicate
    pub fn find_nodes_with_depth<F>(&self, target_depth: usize, predicate: F) -> Vec<&ContentItem>
    where
        F: Fn(&ContentItem) -> bool,
    {
        self.children.find_nodes_with_depth(target_depth, predicate)
    }

    /// Count immediate children by type
    pub fn count_by_type(&self) -> (usize, usize, usize, usize) {
        self.children.count_by_type()
    }

    /// Returns the deepest (most nested) element that contains the position
    pub fn element_at(&self, pos: Position) -> Option<&ContentItem> {
        self.children.element_at(pos)
    }

    /// Returns the visual line element at the given position
    pub fn visual_line_at(&self, pos: Position) -> Option<&ContentItem> {
        self.children.visual_line_at(pos)
    }

    /// Returns the block element at the given position
    pub fn block_element_at(&self, pos: Position) -> Option<&ContentItem> {
        self.children.block_element_at(pos)
    }

    /// Returns the deepest AST node at the given position, if any
    pub fn find_nodes_at_position(&self, position: Position) -> Vec<&dyn AstNode> {
        self.children.find_nodes_at_position(position)
    }

    /// Returns the path of nodes at the given position, starting from this session
    pub fn node_path_at_position(&self, pos: Position) -> Vec<&dyn AstNode> {
        let path = self.children.node_path_at_position(pos);
        if !path.is_empty() {
            let mut nodes: Vec<&dyn AstNode> = Vec::with_capacity(path.len() + 1);
            nodes.push(self);
            for item in path {
                nodes.push(item);
            }
            nodes
        } else if self.location.contains(pos) {
            vec![self]
        } else {
            Vec::new()
        }
    }

    /// Formats information about nodes located at a given position
    pub fn format_at_position(&self, position: Position) -> String {
        self.children.format_at_position(position)
    }

    // ========================================================================
    // REFERENCE RESOLUTION APIs (Issue #291)
    // ========================================================================

    /// Find the first annotation with a matching label.
    ///
    /// This searches recursively through all annotations in the session tree.
    ///
    /// # Arguments
    /// * `label` - The label string to search for
    ///
    /// # Returns
    /// The first annotation whose label matches exactly, or None if not found.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Find annotation with label "42" for reference [42]
    /// if let Some(annotation) = session.find_annotation_by_label("42") {
    ///     // Jump to this annotation in go-to-definition
    /// }
    /// ```
    pub fn find_annotation_by_label(&self, label: &str) -> Option<&Annotation> {
        self.iter_annotations_recursive()
            .find(|ann| ann.data.label.value == label)
    }

    /// Find all annotations with a matching label.
    ///
    /// This searches recursively through all annotations in the session tree.
    /// Multiple annotations might share the same label.
    ///
    /// # Arguments
    /// * `label` - The label string to search for
    ///
    /// # Returns
    /// A vector of all annotations whose labels match exactly.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Find all annotations labeled "note"
    /// let notes = session.find_annotations_by_label("note");
    /// for note in notes {
    ///     // Process each note annotation
    /// }
    /// ```
    pub fn find_annotations_by_label(&self, label: &str) -> Vec<&Annotation> {
        self.iter_annotations_recursive()
            .filter(|ann| ann.data.label.value == label)
            .collect()
    }

    /// Iterate all inline references at any depth.
    ///
    /// This method recursively walks the session tree, parses inline content,
    /// and yields all reference inline nodes (e.g., \[42\], \[@citation\], \[^note\]).
    ///
    /// Note: This method does not currently return source ranges for individual
    /// references. Use the paragraph's location as a starting point for finding
    /// references in the source.
    ///
    /// # Returns
    /// An iterator of references to ReferenceInline nodes
    ///
    /// # Example
    /// ```rust,ignore
    /// for reference in session.iter_all_references() {
    ///     match &reference.reference_type {
    ///         ReferenceType::FootnoteNumber { number } => {
    ///             // Find annotation with this number
    ///         }
    ///         ReferenceType::Citation(data) => {
    ///             // Process citation
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    pub fn iter_all_references(
        &self,
    ) -> Box<dyn Iterator<Item = crate::lex::inlines::ReferenceInline> + '_> {
        use crate::lex::inlines::InlineNode;

        // Helper to extract refs from TextContent
        let extract_refs = |text_content: &TextContent| {
            let inlines = text_content.inline_items();
            inlines
                .into_iter()
                .filter_map(|node| {
                    if let InlineNode::Reference { data, .. } = node {
                        Some(data)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };

        // Title refs
        let title_refs = extract_refs(&self.title);

        // Collect all paragraphs recursively
        let paragraphs: Vec<_> = self.iter_paragraphs_recursive().collect();

        // For each paragraph, iterate through lines and collect references
        let para_refs: Vec<_> = paragraphs
            .into_iter()
            .flat_map(|para| {
                // Iterate through each text line in the paragraph
                para.lines
                    .iter()
                    .filter_map(|item| {
                        if let super::content_item::ContentItem::TextLine(text_line) = item {
                            Some(&text_line.content)
                        } else {
                            None
                        }
                    })
                    .flat_map(extract_refs)
                    .collect::<Vec<_>>()
            })
            .collect();

        Box::new(title_refs.into_iter().chain(para_refs))
    }

    /// Find all references to a specific target label.
    ///
    /// This method searches for inline references that point to the given target.
    /// For example, find all `[42]` references when looking for footnote "42".
    ///
    /// # Arguments
    /// * `target` - The target label to search for
    ///
    /// # Returns
    /// A vector of references to ReferenceInline nodes that match the target
    ///
    /// # Example
    /// ```rust,ignore
    /// // Find all references to footnote "42"
    /// let refs = session.find_references_to("42");
    /// println!("Found {} references to footnote 42", refs.len());
    /// ```
    pub fn find_references_to(&self, target: &str) -> Vec<crate::lex::inlines::ReferenceInline> {
        use crate::lex::inlines::ReferenceType;

        self.iter_all_references()
            .filter(|reference| match &reference.reference_type {
                ReferenceType::FootnoteNumber { number } => target == number.to_string(),
                ReferenceType::FootnoteLabeled { label } => target == label,
                ReferenceType::Session { target: ref_target } => target == ref_target,
                ReferenceType::General { target: ref_target } => target == ref_target,
                ReferenceType::Citation(data) => data.keys.iter().any(|key| key == target),
                _ => false,
            })
            .collect()
    }
}

impl AstNode for Session {
    fn node_type(&self) -> &'static str {
        "Session"
    }
    fn display_label(&self) -> String {
        self.title.as_string().to_string()
    }
    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        visitor.visit_session(self);
        super::super::traits::visit_children(visitor, &self.children);
        visitor.leave_session(self);
    }
}

impl VisualStructure for Session {
    fn is_source_line_node(&self) -> bool {
        true
    }

    fn has_visual_header(&self) -> bool {
        true
    }
}

impl Container for Session {
    fn label(&self) -> &str {
        self.title.as_string()
    }
    fn children(&self) -> &[ContentItem] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<ContentItem> {
        self.children.as_mut_vec()
    }
}

impl fmt::Display for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Session('{}', {} items)",
            self.title.as_string(),
            self.children.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::paragraph::Paragraph;
    use super::*;

    // ========================================================================
    // SESSION-SPECIFIC TESTS
    // Container query/traversal functionality is tested in container.rs
    // These tests focus on session-specific behavior only
    // ========================================================================

    #[test]
    fn test_session_creation() {
        let mut session = Session::with_title("Introduction".to_string());
        session
            .children_mut()
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Content".to_string(),
            )));
        assert_eq!(session.label(), "Introduction");
        assert_eq!(session.children.len(), 1);
    }

    #[test]
    fn test_session_location_builder() {
        let location = super::super::super::range::Range::new(
            0..0,
            super::super::super::range::Position::new(1, 0),
            super::super::super::range::Position::new(1, 10),
        );
        let session = Session::with_title("Title".to_string()).at(location.clone());
        assert_eq!(session.location, location);
    }

    #[test]
    fn test_session_header_and_body_locations() {
        let title_range = Range::new(0..5, Position::new(0, 0), Position::new(0, 5));
        let child_range = Range::new(10..20, Position::new(1, 0), Position::new(2, 0));
        let title = TextContent::from_string("Title".to_string(), Some(title_range.clone()));
        let child = Paragraph::from_line("Child".to_string()).at(child_range.clone());
        let child_item = ContentItem::Paragraph(child);
        let session = Session::new(title, vec![SessionContent::from(child_item)]).at(Range::new(
            0..25,
            Position::new(0, 0),
            Position::new(2, 0),
        ));

        assert_eq!(session.header_location(), Some(&title_range));
        assert_eq!(session.body_location().unwrap().span, child_range.span);
    }

    #[test]
    fn test_session_annotations() {
        let mut session = Session::with_title("Test".to_string());
        assert_eq!(session.annotations().len(), 0);

        session
            .annotations_mut()
            .push(Annotation::marker(super::super::label::Label::new(
                "test".to_string(),
            )));
        assert_eq!(session.annotations().len(), 1);
        assert_eq!(session.iter_annotations().count(), 1);
    }

    #[test]
    fn test_session_delegation_to_container() {
        // Smoke test to verify delegation works
        let mut session = Session::with_title("Root".to_string());
        session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Para 1".to_string(),
            )));
        session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Para 2".to_string(),
            )));

        // Verify delegation methods work
        assert_eq!(session.iter_paragraphs().count(), 2);
        assert_eq!(session.first_paragraph().unwrap().text(), "Para 1");
        assert_eq!(session.count_by_type(), (2, 0, 0, 0));
    }

    mod sequence_marker_integration {
        use super::*;
        use crate::lex::ast::elements::{DecorationStyle, Form, Separator};
        use crate::lex::loader::DocumentLoader;

        #[test]
        fn parse_extracts_numerical_period_marker() {
            let source = "1. First Session:\n\n    Content here";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let session = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::Session(session) = item {
                        Some(session)
                    } else {
                        None
                    }
                })
                .expect("expected session");

            assert!(session.marker.is_some());
            let marker = session.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Numerical);
            assert_eq!(marker.separator, Separator::Period);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "1.");
        }

        #[test]
        fn parse_extracts_numerical_paren_marker() {
            let source = "1) Second Session:\n\n    Content here";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let session = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::Session(session) = item {
                        Some(session)
                    } else {
                        None
                    }
                })
                .expect("expected session");

            assert!(session.marker.is_some());
            let marker = session.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Numerical);
            assert_eq!(marker.separator, Separator::Parenthesis);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "1)");
        }

        #[test]
        fn parse_extracts_alphabetical_marker() {
            let source = "a. Alpha Session:\n\n    Content here";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let session = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::Session(session) = item {
                        Some(session)
                    } else {
                        None
                    }
                })
                .expect("expected session");

            assert!(session.marker.is_some());
            let marker = session.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Alphabetical);
            assert_eq!(marker.separator, Separator::Period);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "a.");
        }

        #[test]
        fn parse_extracts_roman_marker() {
            let source = "I. Roman Session:\n\n    Content here";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let session = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::Session(session) = item {
                        Some(session)
                    } else {
                        None
                    }
                })
                .expect("expected session");

            assert!(session.marker.is_some());
            let marker = session.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Roman);
            assert_eq!(marker.separator, Separator::Period);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "I.");
        }

        #[test]
        fn parse_extracts_extended_numerical_marker() {
            let source = "1.2.3 Extended Session:\n\n    Content here";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let session = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::Session(session) = item {
                        Some(session)
                    } else {
                        None
                    }
                })
                .expect("expected session");

            assert!(session.marker.is_some());
            let marker = session.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Numerical);
            assert_eq!(marker.separator, Separator::Period);
            assert_eq!(marker.form, Form::Extended);
            assert_eq!(marker.raw_text.as_string(), "1.2.3");
        }

        #[test]
        fn parse_extracts_double_paren_marker() {
            let source = "(1) Parens Session:\n\n    Content here";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let session = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::Session(session) = item {
                        Some(session)
                    } else {
                        None
                    }
                })
                .expect("expected session");

            assert!(session.marker.is_some());
            let marker = session.marker.as_ref().unwrap();
            assert_eq!(marker.style, DecorationStyle::Numerical);
            assert_eq!(marker.separator, Separator::DoubleParens);
            assert_eq!(marker.form, Form::Short);
            assert_eq!(marker.raw_text.as_string(), "(1)");
        }

        #[test]
        fn session_without_marker_has_none() {
            let source = "Plain Session:\n\n    Content here";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let session = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::Session(session) = item {
                        Some(session)
                    } else {
                        None
                    }
                })
                .expect("expected session");

            assert!(session.marker.is_none());
        }

        #[test]
        fn title_text_excludes_marker() {
            let source = "1. Introduction:\n\n    Content here";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let session = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::Session(session) = item {
                        Some(session)
                    } else {
                        None
                    }
                })
                .expect("expected session");

            // The title includes the colon, but the marker is stripped
            assert_eq!(session.title_text(), "Introduction:");
            assert_eq!(session.full_title(), "1. Introduction:");
        }

        #[test]
        fn title_text_without_marker_returns_full_title() {
            let source = "Plain Title:\n\n    Content here";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let session = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::Session(session) = item {
                        Some(session)
                    } else {
                        None
                    }
                })
                .expect("expected session");

            // No marker, so title_text() returns the full title
            assert_eq!(session.title_text(), "Plain Title:");
            assert_eq!(session.full_title(), "Plain Title:");
        }

        #[test]
        fn plain_dash_not_valid_for_sessions() {
            // Sessions should not support plain dash markers
            // This test verifies that "- " is not treated as a marker
            let source = "- Not A Marker:\n\n    Content here";
            let doc = DocumentLoader::from_string(source)
                .parse()
                .expect("parse failed");

            let session = doc
                .root
                .children
                .get(0)
                .and_then(|item| {
                    if let ContentItem::Session(session) = item {
                        Some(session)
                    } else {
                        None
                    }
                })
                .expect("expected session");

            // The dash should not be parsed as a marker for sessions
            assert!(
                session.marker.is_none(),
                "Dash should not be treated as a marker for sessions"
            );
            assert_eq!(session.full_title(), "- Not A Marker:");
        }
    }
}
