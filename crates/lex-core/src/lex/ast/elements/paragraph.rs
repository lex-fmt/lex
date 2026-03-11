//! Paragraph element
//!
//! A paragraph is a block of one or more text lines. It represents any text,  except for a
//! unascaped annotation string/.
//!
//! The above is an example of a single line paragraph.
//! Whereas this is an example of a multi-line paragraph:
//!
//! Parsing Structure:
//!
//! | Element   | Prec. Blank | Head     | Tail                |
//! |-----------|-------------|----------|---------------------|
//! | Paragraph | Optional    | Any Line | BlankLine or Dedent |
//!
//! Special Case: Dialog - Lines starting with "-" can be formally specified as dialog
//! (paragraphs) rather than list items.
//!
//! Learn More:
//! - Paragraphs spec: specs/v1/elements/paragraph.lex
//!
//! Examples:
//! - A single paragraph spans multiple lines until a blank line
//! - Blank lines separate paragraphs; lists and sessions break flow

use super::super::range::{Position, Range};
use super::super::text_content::TextContent;
use super::super::traits::{AstNode, Container, TextNode, Visitor, VisualStructure};
use super::annotation::Annotation;
use super::content_item::ContentItem;
use std::fmt;

/// A text line within a paragraph
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub content: TextContent,
    pub location: Range,
}

impl TextLine {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }
    pub fn new(content: TextContent) -> Self {
        Self {
            content,
            location: Self::default_location(),
        }
    }

    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    pub fn text(&self) -> &str {
        self.content.as_string()
    }
}

impl AstNode for TextLine {
    fn node_type(&self) -> &'static str {
        "TextLine"
    }

    fn display_label(&self) -> String {
        let text = self.text();
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
        visitor.visit_text_line(self);
        visitor.leave_text_line(self);
    }
}

impl VisualStructure for TextLine {
    fn is_source_line_node(&self) -> bool {
        true
    }
}

impl fmt::Display for TextLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TextLine('{}')", self.text())
    }
}

/// A paragraph represents a block of text lines
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    /// Lines stored as ContentItems (each a TextLine wrapping TextContent)
    pub lines: Vec<ContentItem>,
    pub annotations: Vec<Annotation>,
    pub location: Range,
}

impl Paragraph {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }
    pub fn new(lines: Vec<ContentItem>) -> Self {
        debug_assert!(
            lines
                .iter()
                .all(|item| matches!(item, ContentItem::TextLine(_))),
            "Paragraph lines must be TextLine items"
        );
        Self {
            lines,
            annotations: Vec::new(),
            location: Self::default_location(),
        }
    }
    pub fn from_line(line: String) -> Self {
        Self {
            lines: vec![ContentItem::TextLine(TextLine::new(
                TextContent::from_string(line, None),
            ))],
            annotations: Vec::new(),
            location: Self::default_location(),
        }
    }
    /// Create a paragraph with a single line and attach a location
    pub fn from_line_at(line: String, location: Range) -> Self {
        let mut para = Self {
            lines: vec![ContentItem::TextLine(TextLine::new(
                TextContent::from_string(line, None),
            ))],
            annotations: Vec::new(),
            location: Self::default_location(),
        };
        para = para.at(location);
        para
    }

    /// Preferred builder
    pub fn at(mut self, location: Range) -> Self {
        self.location = location.clone();
        // When a paragraph's location is set in tests, we should also update
        // the location of the single child TextLine for consistency, as this
        // is what the parser would do.
        if self.lines.len() == 1 {
            if let Some(super::content_item::ContentItem::TextLine(text_line)) =
                self.lines.get_mut(0)
            {
                text_line.location = location;
            }
        }
        self
    }
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .filter_map(|item| {
                if let super::content_item::ContentItem::TextLine(tl) = item {
                    Some(tl.text().to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Annotations attached to this paragraph.
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Mutable access to paragraph annotations.
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
}

impl AstNode for Paragraph {
    fn node_type(&self) -> &'static str {
        "Paragraph"
    }
    fn display_label(&self) -> String {
        format!("{} line(s)", self.lines.len())
    }
    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        visitor.visit_paragraph(self);
        // Visit child TextLines
        super::super::traits::visit_children(visitor, &self.lines);
        visitor.leave_paragraph(self);
    }
}

impl TextNode for Paragraph {
    fn text(&self) -> String {
        self.lines
            .iter()
            .filter_map(|item| {
                if let super::content_item::ContentItem::TextLine(tl) = item {
                    Some(tl.text().to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn lines(&self) -> &[TextContent] {
        // This is a compatibility method - we no longer store raw TextContent
        // Return empty slice since we've moved to ContentItem::TextLine
        &[]
    }
}

impl VisualStructure for Paragraph {
    fn collapses_with_children(&self) -> bool {
        true
    }
}

impl fmt::Display for Paragraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Paragraph({} lines)", self.lines.len())
    }
}

#[cfg(test)]
mod tests {
    use super::super::content_item::ContentItem;
    use super::*;

    #[test]
    fn test_paragraph_creation() {
        let para = Paragraph::new(vec![
            ContentItem::TextLine(TextLine::new(TextContent::from_string(
                "Hello".to_string(),
                None,
            ))),
            ContentItem::TextLine(TextLine::new(TextContent::from_string(
                "World".to_string(),
                None,
            ))),
        ]);
        assert_eq!(para.lines.len(), 2);
        assert_eq!(para.text(), "Hello\nWorld");
    }

    #[test]
    fn test_paragraph() {
        let location = Range::new(
            0..0,
            super::super::super::range::Position::new(0, 0),
            super::super::super::range::Position::new(0, 5),
        );
        let para = Paragraph::from_line("Hello".to_string()).at(location.clone());

        assert_eq!(para.location, location);
    }
}
