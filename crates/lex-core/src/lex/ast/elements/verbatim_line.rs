//! Verbatim line element
//!
//! A verbatim line represents a single line of verbatim content within a verbatim block.
//! This is the "lead item" for verbatim blocks, similar to how sessions have titles
//! and definitions have subjects.
//!
//! The verbatim line handles the indentation wall - stripping the common indentation
//! from all content lines to preserve content integrity regardless of nesting level.
//!
//! Structure:
//! - content: The raw text content of the verbatim line
//! - location: The byte range and position information
//!
//! Note: Verbatim lines are typically collected as children of a VerbatimBlock, but
//! a verbatim block can forgo content entirely (e.g., for binary markers).

use super::super::range::{Position, Range};
use super::super::text_content::TextContent;
use super::super::traits::AstNode;
use super::super::traits::Visitor;
use super::super::traits::VisualStructure;
use std::fmt;

/// A verbatim line represents a single line of verbatim content
#[derive(Debug, Clone, PartialEq)]
pub struct VerbatimLine {
    pub content: TextContent,
    pub location: Range,
}

impl VerbatimLine {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }

    pub fn new(content: String) -> Self {
        Self {
            content: TextContent::from_string(content, None),
            location: Self::default_location(),
        }
    }

    pub fn from_text_content(content: TextContent) -> Self {
        Self {
            content,
            location: Self::default_location(),
        }
    }

    /// Preferred builder
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }
}

impl AstNode for VerbatimLine {
    fn node_type(&self) -> &'static str {
        "VerbatimLine"
    }

    fn display_label(&self) -> String {
        let content_text = self.content.as_string();
        if content_text.chars().count() > 50 {
            format!("{}…", content_text.chars().take(50).collect::<String>())
        } else {
            content_text.to_string()
        }
    }

    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        visitor.visit_verbatim_line(self);
        // VerbatimLine has no children - it's a leaf node
        visitor.leave_verbatim_line(self);
    }
}

impl VisualStructure for VerbatimLine {
    fn is_source_line_node(&self) -> bool {
        true
    }
}

impl fmt::Display for VerbatimLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VerbatimLine({} chars)", self.content.as_string().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verbatim_line_creation() {
        let line = VerbatimLine::new("    code line".to_string());
        assert_eq!(line.content.as_string(), "    code line");
    }

    #[test]
    fn test_verbatim_line_with_location() {
        let location = Range::new(0..12, Position::new(1, 0), Position::new(1, 12));
        let line = VerbatimLine::new("    code line".to_string()).at(location.clone());
        assert_eq!(line.location, location);
    }
}
