//! BlankLineGroup element
//!
//!     A BlankLineGroup represents one or more consecutive blank lines in the source.
//!
//! Blank Line Semantics
//!
//!     Blank lines are lines of text where only whitespace characters appear before the new line.
//!     They are semantically significant, but only that they exist, the exact whitespace content
//!     is not taken into account.
//!
//!     How many consecutive blank lines is not taken into account, only that there is at least
//!     one. Again, multiple blank lines are not discarded, but treated as a blank line group.
//!
//!     In lex, blank lines are meaningful as separators between elements, but any additional
//!     blank lines beyond the first are not semantically significant. That is:
//!
//! ```text
//! content
//! blank-line
//! content
//! ```
//!
//! is functionally identical to:
//!
//! ```text
//! content
//! blank-line
//! blank-line
//! content
//! ```
//!
//! By grouping consecutive blank lines into a single node, we:
//! - Preserve the information (count and source tokens)
//! - Simplify grammar matching (no need for blank-line+)
//! - Make the AST less noisy while maintaining fidelity

use super::super::range::{Position, Range};
use super::super::traits::{AstNode, Visitor, VisualStructure};
use crate::lex::lexing::Token;
use std::fmt;

/// A group of one or more consecutive blank lines
#[derive(Debug, Clone, PartialEq)]
pub struct BlankLineGroup {
    /// The number of blank lines in this group
    pub count: usize,
    /// The source tokens that make up this group
    pub source_tokens: Vec<Token>,
    /// The location of this group in the source
    pub location: Range,
}

impl BlankLineGroup {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }

    pub fn new(count: usize, source_tokens: Vec<Token>) -> Self {
        Self {
            count,
            source_tokens,
            location: Self::default_location(),
        }
    }

    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }
}

impl AstNode for BlankLineGroup {
    fn node_type(&self) -> &'static str {
        "BlankLineGroup"
    }

    fn display_label(&self) -> String {
        if self.count == 1 {
            "1 blank line".to_string()
        } else {
            format!("{} blank lines", self.count)
        }
    }

    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        visitor.visit_blank_line_group(self);
        visitor.leave_blank_line_group(self);
    }
}

impl VisualStructure for BlankLineGroup {
    fn is_source_line_node(&self) -> bool {
        true
    }
}

impl fmt::Display for BlankLineGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlankLineGroup({})", self.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blank_line_group_creation() {
        let group = BlankLineGroup::new(3, vec![]);
        assert_eq!(group.count, 3);
        assert_eq!(group.display_label(), "3 blank lines");
    }

    #[test]
    fn test_blank_line_group_single() {
        let group = BlankLineGroup::new(1, vec![]);
        assert_eq!(group.count, 1);
        assert_eq!(group.display_label(), "1 blank line");
    }
}
