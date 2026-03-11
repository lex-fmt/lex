//! Position and location tracking for source code locations
//!
//! This module defines the data structures for representing positions and locations in source code,
//! as well as utilities for converting byte offsets to line/column positions.
//!
//! ## Types
//!
//! - [`Position`] - A line:column position in source code
//! - [`Range`] - A source code range with start/end positions and byte span
//! - [`SourceLocation`] - Utility for converting byte offsets to positions
//!
//! ## Key Design
//!
//! - Mandatory locations: All AST nodes have required `location: Range` fields
//! - No null locations: Default position is (0, 0) to (0, 0), never None
//! - Byte ranges preserved: Stores both byte spans and line:column positions
//! - Unicode-aware: Handles multi-byte UTF-8 characters correctly via `char_indices()`
//! - Efficient conversion: O(log n) binary search for byte-to-position conversion
//!
//! ## Usage
//!
//! The typical flow is:
//! 1. Lexer produces `(Token, std::ops::Range<usize>)` pairs (byte offsets)
//! 2. Parser converts byte ranges to `Range` using `SourceLocation::byte_range_to_ast_range()`
//! 3. AST nodes store these `Range` values for error reporting and tooling
//!
//! See `src/lex/building/location.rs` for the canonical location conversion and aggregation utilities.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Range as ByteRange;

/// Represents a position in source code (line and column)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// Represents a location in source code (start and end positions)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Range {
    pub span: ByteRange<usize>,
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(span: ByteRange<usize>, start: Position, end: Position) -> Self {
        Self { span, start, end }
    }

    /// Check if a position is contained within this location
    pub fn contains(&self, pos: Position) -> bool {
        (self.start.line < pos.line
            || (self.start.line == pos.line && self.start.column <= pos.column))
            && (self.end.line > pos.line
                || (self.end.line == pos.line && self.end.column >= pos.column))
    }

    /// Check if another location overlaps with this location
    pub fn overlaps(&self, other: &Range) -> bool {
        self.contains(other.start)
            || self.contains(other.end)
            || other.contains(self.start)
            || other.contains(self.end)
    }

    /// Build a bounding box that contains all provided ranges.
    pub fn bounding_box<'a, I>(mut ranges: I) -> Option<Range>
    where
        I: Iterator<Item = &'a Range>,
    {
        let first = ranges.next()?.clone();
        let mut span_start = first.span.start;
        let mut span_end = first.span.end;
        let mut start_pos = first.start;
        let mut end_pos = first.end;

        for range in ranges {
            if range.start < start_pos {
                start_pos = range.start;
                span_start = range.span.start;
            } else if range.start == start_pos {
                span_start = span_start.min(range.span.start);
            }

            if range.end > end_pos {
                end_pos = range.end;
                span_end = range.span.end;
            } else if range.end == end_pos {
                span_end = span_end.max(range.span.end);
            }
        }

        Some(Range::new(span_start..span_end, start_pos, end_pos))
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

impl Default for Range {
    fn default() -> Self {
        Self::new(
            ByteRange { start: 0, end: 0 },
            Position::default(),
            Position::default(),
        )
    }
}

/// Provides fast conversion from byte offsets to line/column positions
pub struct SourceLocation {
    /// Byte offsets where each line starts
    line_starts: Vec<usize>,
}

impl SourceLocation {
    /// Create a new SourceLocation from source code
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];

        for (byte_pos, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(byte_pos + 1);
            }
        }

        Self { line_starts }
    }

    /// Convert a byte offset to a line/column position
    pub fn byte_to_position(&self, byte_offset: usize) -> Position {
        let line = self
            .line_starts
            .binary_search(&byte_offset)
            .unwrap_or_else(|i| i - 1);

        let column = byte_offset - self.line_starts[line];

        Position::new(line, column)
    }

    /// Convert a byte range to a location
    pub fn byte_range_to_ast_range(&self, range: &ByteRange<usize>) -> Range {
        Range::new(
            range.clone(),
            self.byte_to_position(range.start),
            self.byte_to_position(range.end),
        )
    }

    /// Get the total number of lines in the source
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Get the byte offset for the start of a line
    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // @audit: no_source

    // @audit: no_source
    #[test]
    fn test_position_creation() {
        let pos = Position::new(5, 10);
        assert_eq!(pos.line, 5);
        assert_eq!(pos.column, 10);
    }

    // @audit: no_source
    #[test]
    fn test_position_comparison() {
        let pos1 = Position::new(1, 5);
        let pos2 = Position::new(1, 5);
        let pos3 = Position::new(2, 3);

        assert_eq!(pos1, pos2);
        assert_ne!(pos1, pos3);
        assert!(pos1 < pos3);
    }

    // @audit: no_source
    #[test]
    fn test_location_creation() {
        let start = Position::new(0, 0);
        let end = Position::new(2, 5);
        let location = Range::new(0..0, start, end);

        assert_eq!(location.start, start);
        assert_eq!(location.end, end);
    }

    // @audit: no_source
    #[test]
    fn test_location_contains_single_line() {
        let location = Range::new(0..0, Position::new(0, 0), Position::new(0, 10));

        assert!(location.contains(Position::new(0, 0)));
        assert!(location.contains(Position::new(0, 5)));
        assert!(location.contains(Position::new(0, 10)));

        assert!(!location.contains(Position::new(0, 11)));
        assert!(!location.contains(Position::new(1, 0)));
    }

    // @audit: no_source
    #[test]
    fn test_location_contains_multiline() {
        let location = Range::new(0..0, Position::new(1, 5), Position::new(2, 10));

        // Before location
        assert!(!location.contains(Position::new(1, 4)));
        assert!(!location.contains(Position::new(0, 5)));

        // In location
        assert!(location.contains(Position::new(1, 5)));
        assert!(location.contains(Position::new(1, 10)));
        assert!(location.contains(Position::new(2, 0)));
        assert!(location.contains(Position::new(2, 10)));

        // After location
        assert!(!location.contains(Position::new(2, 11)));
        assert!(!location.contains(Position::new(3, 0)));
    }

    // @audit: no_source
    #[test]
    fn test_location_overlaps() {
        let location1 = Range::new(0..0, Position::new(0, 0), Position::new(1, 5));
        let location2 = Range::new(0..0, Position::new(1, 0), Position::new(2, 5));
        let location3 = Range::new(0..0, Position::new(3, 0), Position::new(4, 5));

        assert!(location1.overlaps(&location2));
        assert!(location2.overlaps(&location1));
        assert!(!location1.overlaps(&location3));
        assert!(!location3.overlaps(&location1));
    }

    #[test]
    fn test_bounding_box_ranges() {
        let ranges = [
            Range::new(2..5, Position::new(0, 2), Position::new(0, 5)),
            Range::new(10..20, Position::new(3, 0), Position::new(4, 3)),
        ];

        let bbox = Range::bounding_box(ranges.iter()).unwrap();
        assert_eq!(bbox.span, 2..20);
        assert_eq!(bbox.start, Position::new(0, 2));
        assert_eq!(bbox.end, Position::new(4, 3));
    }

    #[test]
    fn test_bounding_box_empty_iter() {
        let iter = std::iter::empty::<&Range>();
        assert!(Range::bounding_box(iter).is_none());
    }

    // @audit: no_source
    #[test]
    fn test_position_display() {
        let pos = Position::new(5, 10);
        assert_eq!(format!("{pos}"), "5:10");
    }

    // @audit: no_source
    #[test]
    fn test_location_display() {
        let location = Range::new(0..0, Position::new(1, 0), Position::new(2, 5));
        assert_eq!(format!("{location}"), "1:0..2:5");
    }

    // @audit: no_source
    #[test]
    fn test_byte_to_position_single_line() {
        let loc = SourceLocation::new("Hello");
        assert_eq!(loc.byte_to_position(0), Position::new(0, 0));
        assert_eq!(loc.byte_to_position(1), Position::new(0, 1));
        assert_eq!(loc.byte_to_position(4), Position::new(0, 4));
    }

    // @audit: no_source
    #[test]
    fn test_byte_to_position_multiline() {
        let loc = SourceLocation::new("Hello\nworld\ntest");

        // First line
        assert_eq!(loc.byte_to_position(0), Position::new(0, 0));
        assert_eq!(loc.byte_to_position(5), Position::new(0, 5));

        // Second line
        assert_eq!(loc.byte_to_position(6), Position::new(1, 0));
        assert_eq!(loc.byte_to_position(10), Position::new(1, 4));

        // Third line
        assert_eq!(loc.byte_to_position(12), Position::new(2, 0));
        assert_eq!(loc.byte_to_position(15), Position::new(2, 3));
    }

    // @audit: no_source
    #[test]
    fn test_byte_to_position_with_unicode() {
        let loc = SourceLocation::new("Hello\nwörld");
        // Unicode characters take multiple bytes
        assert_eq!(loc.byte_to_position(6), Position::new(1, 0));
        assert_eq!(loc.byte_to_position(7), Position::new(1, 1));
    }

    #[test]
    fn test_range_to_location_single_line() {
        let loc = SourceLocation::new("Hello World");
        let location = loc.byte_range_to_ast_range(&(0..5));

        assert_eq!(location.start, Position::new(0, 0));
        assert_eq!(location.end, Position::new(0, 5));
    }

    #[test]
    fn test_range_to_location_multiline() {
        let loc = SourceLocation::new("Hello\nWorld\nTest");
        let location = loc.byte_range_to_ast_range(&(6..12));

        assert_eq!(location.start, Position::new(1, 0));
        assert_eq!(location.end, Position::new(2, 0));
    }

    #[test]
    fn test_line_count() {
        assert_eq!(SourceLocation::new("single").line_count(), 1);
        assert_eq!(SourceLocation::new("line1\nline2").line_count(), 2);
        assert_eq!(SourceLocation::new("line1\nline2\nline3").line_count(), 3);
    }

    #[test]
    fn test_line_start() {
        let loc = SourceLocation::new("Hello\nWorld\nTest");

        assert_eq!(loc.line_start(0), Some(0));
        assert_eq!(loc.line_start(1), Some(6));
        assert_eq!(loc.line_start(2), Some(12));
        assert_eq!(loc.line_start(3), None);
    }
}

#[cfg(test)]
mod ast_integration_tests {
    use crate::lex::ast::{
        elements::Session,
        range::{Position, Range},
        traits::{AstNode, Container},
    };

    #[test]
    fn test_start_position() {
        let location = Range::new(0..0, Position::new(1, 0), Position::new(1, 10));
        let session = Session::with_title("Title".to_string()).at(location);
        assert_eq!(session.start_position(), Position::new(1, 0));
    }

    #[test]
    fn test_find_nodes_at_position() {
        use crate::lex::ast::elements::ContentItem;
        use crate::lex::ast::elements::Document;
        use crate::lex::ast::find_nodes_at_position;

        let location1 = Range::new(0..0, Position::new(1, 0), Position::new(1, 10));
        let location2 = Range::new(0..0, Position::new(2, 0), Position::new(2, 10));
        let session1 = Session::with_title("Title1".to_string()).at(location1);
        let session2 = Session::with_title("Title2".to_string()).at(location2);
        let document = Document::with_content(vec![
            ContentItem::Session(session1),
            ContentItem::Session(session2),
        ]);
        let nodes = find_nodes_at_position(&document, Position::new(1, 5));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_type(), "Session");
        assert_eq!(nodes[0].display_label(), "Title1");
    }

    #[test]
    fn test_find_nested_nodes_at_position() {
        use crate::lex::ast::elements::{ContentItem, Document, Paragraph};
        use crate::lex::ast::find_nodes_at_position;

        let para_location = Range::new(0..0, Position::new(2, 0), Position::new(2, 10));
        let paragraph = Paragraph::from_line("Nested".to_string()).at(para_location);
        let session_location = Range::new(0..0, Position::new(1, 0), Position::new(3, 0));
        let mut session = Session::with_title("Title".to_string()).at(session_location);
        session
            .children_mut()
            .push(ContentItem::Paragraph(paragraph));
        let document = Document::with_content(vec![ContentItem::Session(session)]);
        let nodes = find_nodes_at_position(&document, Position::new(2, 5));
        // Now we get only the deepest element: TextLine
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_type(), "TextLine");
    }
}
