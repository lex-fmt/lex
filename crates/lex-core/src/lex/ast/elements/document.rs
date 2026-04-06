//! Document element
//!
//!     The document node serves two purposes:
//!         - Contains the document tree.
//!         - Contains document-level annotations, including non-content metadata (like file name,
//!           parser version, etc).
//!
//!     Lex documents are plain text, utf-8 encoded files with the file extension .lex. Line width
//!     is not limited, and is considered a presentation detail. Best practice dictates only
//!     limiting line length when publishing, not while authoring.
//!
//!     The document node holds the document metadata and the content's root node, which is a
//!     session node. The structure of the document then is a tree of sessions, which can be nested
//!     arbitrarily. This creates powerful addressing capabilities as one can target any sub-session
//!     from an index.
//!
//!     Document Title:
//!     The document title is a first-class element, represented as a dedicated `DocumentTitle`
//!     AST node owned directly by the `Document`. It is parsed from a single unindented line
//!     at the start of the document, followed by blank lines, where no indented content follows
//!     (distinguishing it from a session title). See specs/elements/document.lex.
//!
//!     Document Start:
//!     A synthetic `DocumentStart` token is used to mark the boundary between document-level
//!     annotations (metadata) and the actual document content. This allows the parser and
//!     assembly logic to correctly identify where the body begins.
//!
//!     For more details on document structure and sessions, see the [ast](crate::lex::ast) module.
//!
//! Learn More:
//! - Paragraphs: specs/v1/elements/paragraph.lex
//! - Lists: specs/v1/elements/list.lex
//! - Sessions: specs/v1/elements/session.lex
//! - Annotations: specs/v1/elements/annotation.lex
//! - Definitions: specs/v1/elements/definition.lex
//! - Verbatim blocks: specs/v1/elements/verbatim.lex
//!
//! Examples:
//! - Document-level metadata via annotations
//! - All body content accessible via document.root.children

use super::super::range::{Position, Range};
use super::super::text_content::TextContent;
use super::super::traits::{AstNode, Container, Visitor};
use super::annotation::Annotation;
use super::content_item::ContentItem;
use super::session::Session;
use super::typed_content;
use std::fmt;

/// A first-class document title element.
///
/// Represents the title of a Lex document — a single unindented line at the start
/// of the document, followed by blank lines, with no indented content after.
/// This is distinct from session titles.
///
/// An optional subtitle is supported: when the title line ends with a colon and a
/// second non-blank, non-indented line follows before the blank separator, the
/// second line is parsed as a subtitle. The trailing colon is structural (stripped
/// from the title content).
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentTitle {
    pub content: TextContent,
    pub subtitle: Option<TextContent>,
    pub location: Range,
}

impl DocumentTitle {
    pub fn new(content: TextContent, location: Range) -> Self {
        Self {
            content,
            subtitle: None,
            location,
        }
    }

    pub fn with_subtitle(content: TextContent, subtitle: TextContent, location: Range) -> Self {
        Self {
            content,
            subtitle: Some(subtitle),
            location,
        }
    }

    pub fn from_string(text: String, location: Range) -> Self {
        Self {
            content: TextContent::from_string(text, Some(location.clone())),
            subtitle: None,
            location,
        }
    }

    pub fn as_str(&self) -> &str {
        self.content.as_string()
    }

    pub fn subtitle_str(&self) -> Option<&str> {
        self.subtitle.as_ref().map(|s| s.as_string())
    }
}

impl AstNode for DocumentTitle {
    fn node_type(&self) -> &'static str {
        "DocumentTitle"
    }

    fn display_label(&self) -> String {
        match &self.subtitle {
            Some(sub) => format!(
                "DocumentTitle(\"{}\", subtitle: \"{}\")",
                self.as_str(),
                sub.as_string()
            ),
            None => format!("DocumentTitle(\"{}\")", self.as_str()),
        }
    }

    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, _visitor: &mut dyn Visitor) {}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub annotations: Vec<Annotation>,
    pub title: Option<DocumentTitle>,
    // all content is attached to the root node
    pub root: Session,
}

impl Document {
    pub fn new() -> Self {
        Self {
            annotations: Vec::new(),
            title: None,
            root: Session::with_title(String::new()),
        }
    }

    pub fn with_content(content: Vec<ContentItem>) -> Self {
        let mut root = Session::with_title(String::new());
        let session_content = typed_content::into_session_contents(content);
        root.children = super::container::SessionContainer::from_typed(session_content);
        Self {
            annotations: Vec::new(),
            title: None,
            root,
        }
    }

    /// Construct a document from an existing root session.
    pub fn from_root(root: Session) -> Self {
        Self {
            annotations: Vec::new(),
            title: None,
            root,
        }
    }

    /// Construct a document from a title and root session.
    pub fn from_title_and_root(title: Option<DocumentTitle>, root: Session) -> Self {
        Self {
            annotations: Vec::new(),
            title,
            root,
        }
    }

    pub fn with_annotations_and_content(
        annotations: Vec<Annotation>,
        content: Vec<ContentItem>,
    ) -> Self {
        let mut root = Session::with_title(String::new());
        let session_content = typed_content::into_session_contents(content);
        root.children = super::container::SessionContainer::from_typed(session_content);
        Self {
            annotations,
            title: None,
            root,
        }
    }

    pub fn with_root_location(mut self, location: Range) -> Self {
        self.root.location = location;
        self
    }

    pub fn root_session(&self) -> &Session {
        &self.root
    }

    pub fn root_session_mut(&mut self) -> &mut Session {
        &mut self.root
    }

    pub fn into_root(self) -> Session {
        self.root
    }

    /// Get the document title text.
    ///
    /// Returns the title string if a DocumentTitle is present, empty string otherwise.
    pub fn title(&self) -> &str {
        match &self.title {
            Some(dt) => dt.as_str(),
            None => "",
        }
    }

    /// Set the document title.
    pub fn set_title(&mut self, title: String) {
        if title.is_empty() {
            self.title = None;
        } else {
            let location = Range::default();
            self.title = Some(DocumentTitle::from_string(title, location));
        }
    }

    /// Returns the path of nodes at the given position, starting from the document
    pub fn node_path_at_position(&self, pos: Position) -> Vec<&dyn AstNode> {
        let path = self.root.node_path_at_position(pos);
        if !path.is_empty() {
            let mut nodes: Vec<&dyn AstNode> = Vec::with_capacity(path.len() + 1);
            nodes.push(self);
            nodes.extend(path);
            nodes
        } else {
            Vec::new()
        }
    }

    /// Returns the deepest (most nested) element that contains the position
    pub fn element_at(&self, pos: Position) -> Option<&ContentItem> {
        self.root.element_at(pos)
    }

    /// Returns the visual line element at the given position
    pub fn visual_line_at(&self, pos: Position) -> Option<&ContentItem> {
        self.root.visual_line_at(pos)
    }

    /// Returns the block element at the given position
    pub fn block_element_at(&self, pos: Position) -> Option<&ContentItem> {
        self.root.block_element_at(pos)
    }

    /// All annotations attached directly to the document (document-level metadata).
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Mutable access to document-level annotations.
    pub fn annotations_mut(&mut self) -> &mut Vec<Annotation> {
        &mut self.annotations
    }

    /// Iterate over document-level annotation blocks in source order.
    pub fn iter_annotations(&self) -> std::slice::Iter<'_, Annotation> {
        self.annotations.iter()
    }

    /// Iterate over all content items nested inside document-level annotations.
    pub fn iter_annotation_contents(&self) -> impl Iterator<Item = &ContentItem> {
        self.annotations
            .iter()
            .flat_map(|annotation| annotation.children())
    }

    // ========================================================================
    // REFERENCE RESOLUTION APIs (Issue #291)
    // Delegates to the root session
    // ========================================================================

    /// Find the first annotation with a matching label.
    ///
    /// This searches recursively through all annotations in the document,
    /// including both document-level annotations and annotations in the content tree.
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
    /// if let Some(annotation) = document.find_annotation_by_label("42") {
    ///     // Jump to this annotation in go-to-definition
    /// }
    /// ```
    pub fn find_annotation_by_label(&self, label: &str) -> Option<&Annotation> {
        // First check document-level annotations
        self.annotations
            .iter()
            .find(|ann| ann.data.label.value == label)
            .or_else(|| self.root.find_annotation_by_label(label))
    }

    /// Find all annotations with a matching label.
    ///
    /// This searches recursively through all annotations in the document,
    /// including both document-level annotations and annotations in the content tree.
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
    /// let notes = document.find_annotations_by_label("note");
    /// for note in notes {
    ///     // Process each note annotation
    /// }
    /// ```
    pub fn find_annotations_by_label(&self, label: &str) -> Vec<&Annotation> {
        let mut results: Vec<&Annotation> = self
            .annotations
            .iter()
            .filter(|ann| ann.data.label.value == label)
            .collect();

        results.extend(self.root.find_annotations_by_label(label));
        results
    }

    /// Iterate all inline references at any depth.
    ///
    /// This method recursively walks the document tree, parses inline content,
    /// and yields all reference inline nodes (e.g., \[42\], \[@citation\], \[::note\]).
    ///
    /// # Returns
    /// An iterator of references to ReferenceInline nodes
    ///
    /// # Example
    /// ```rust,ignore
    /// for reference in document.iter_all_references() {
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
        let title_refs = self
            .title
            .iter()
            .flat_map(|t| {
                let title_inlines = t.content.inline_items();
                let subtitle_inlines = t
                    .subtitle
                    .iter()
                    .flat_map(|s| s.inline_items())
                    .collect::<Vec<_>>();
                title_inlines.into_iter().chain(subtitle_inlines)
            })
            .filter_map(|node| {
                if let crate::lex::inlines::InlineNode::Reference { data, .. } = node {
                    Some(data)
                } else {
                    None
                }
            });
        Box::new(title_refs.chain(self.root.iter_all_references()))
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
    /// let refs = document.find_references_to("42");
    /// println!("Found {} references to footnote 42", refs.len());
    /// ```
    pub fn find_references_to(&self, target: &str) -> Vec<crate::lex::inlines::ReferenceInline> {
        self.root.find_references_to(target)
    }
}

impl AstNode for Document {
    fn node_type(&self) -> &'static str {
        "Document"
    }

    fn display_label(&self) -> String {
        format!(
            "Document ({} annotations, {} items)",
            self.annotations.len(),
            self.root.children.len()
        )
    }

    fn range(&self) -> &Range {
        &self.root.location
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        for annotation in &self.annotations {
            annotation.accept(visitor);
        }
        if let Some(title) = &self.title {
            title.accept(visitor);
        }
        self.root.accept(visitor);
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Document({} annotations, {} items)",
            self.annotations.len(),
            self.root.children.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::range::Position;
    use super::super::paragraph::{Paragraph, TextLine};
    use super::super::session::Session;
    use super::*;
    use crate::lex::ast::text_content::TextContent;
    use crate::lex::ast::traits::AstNode;

    #[test]
    fn test_document_creation() {
        let doc = Document::with_content(vec![
            ContentItem::Paragraph(Paragraph::from_line("Para 1".to_string())),
            ContentItem::Session(Session::with_title("Section 1".to_string())),
        ]);
        assert_eq!(doc.annotations.len(), 0);
        assert_eq!(doc.root.children.len(), 2);
    }

    #[test]
    fn test_document_element_at() {
        let text_line1 = TextLine::new(TextContent::from_string("First".to_string(), None))
            .at(Range::new(0..0, Position::new(0, 0), Position::new(0, 5)));
        let para1 = Paragraph::new(vec![ContentItem::TextLine(text_line1)]).at(Range::new(
            0..0,
            Position::new(0, 0),
            Position::new(0, 5),
        ));

        let text_line2 = TextLine::new(TextContent::from_string("Second".to_string(), None))
            .at(Range::new(0..0, Position::new(1, 0), Position::new(1, 6)));
        let para2 = Paragraph::new(vec![ContentItem::TextLine(text_line2)]).at(Range::new(
            0..0,
            Position::new(1, 0),
            Position::new(1, 6),
        ));

        let doc = Document::with_content(vec![
            ContentItem::Paragraph(para1),
            ContentItem::Paragraph(para2),
        ]);

        let result = doc.root.element_at(Position::new(1, 3));
        assert!(result.is_some(), "Expected to find element at position");
        assert!(result.unwrap().is_text_line());
    }

    #[test]
    fn test_document_traits() {
        let doc = Document::with_content(vec![ContentItem::Paragraph(Paragraph::from_line(
            "Line".to_string(),
        ))]);

        assert_eq!(doc.node_type(), "Document");
        assert_eq!(doc.display_label(), "Document (0 annotations, 1 items)");
        assert_eq!(doc.root.children.len(), 1);
    }

    #[test]
    fn test_root_session_accessors() {
        let doc = Document::with_content(vec![ContentItem::Session(Session::with_title(
            "Section".to_string(),
        ))]);

        assert_eq!(doc.root_session().children.len(), 1);

        let mut doc = doc;
        doc.root_session_mut().title = TextContent::from_string("Updated".to_string(), None);
        assert_eq!(doc.root_session().title.as_string(), "Updated");

        let root = doc.into_root();
        assert_eq!(root.title.as_string(), "Updated");
    }

    #[test]
    fn test_document_title_field() {
        let mut doc = Document::new();
        assert!(doc.title.is_none());
        assert_eq!(doc.title(), "");

        doc.set_title("My Title".to_string());
        assert!(doc.title.is_some());
        assert_eq!(doc.title(), "My Title");

        doc.set_title(String::new());
        assert!(doc.title.is_none());
        assert_eq!(doc.title(), "");
    }

    #[test]
    fn test_from_title_and_root() {
        let title = DocumentTitle::from_string("Test Title".to_string(), Range::default());
        let root = Session::with_title(String::new());
        let doc = Document::from_title_and_root(Some(title), root);
        assert_eq!(doc.title(), "Test Title");
    }
}
