//! Location utilities for AST node building
//!
//!     Provides shared location handling utilities used by the parser/AST builder. These
//!     utilities handle the conversion from byte ranges to line/column positions and compute
//!     bounding boxes for container nodes (sessions, lists, definitions, etc.).
//!
//!     During AST building, the location from tokens is used to calculate the location for
//!     the ast node. The location is transformed from byte range to a dual byte range +
//!     line:column position.
//!
//!     Byte ranges from tokens are converted to line:column positions using SourceLocation
//!     (one-time setup, O(log n) per conversion). Location aggregation from child nodes is
//!     done via compute_location_from_locations(), which creates a bounding box that
//!     encompasses all child locations.
//!
//!     Relationship to `ast/range.rs`:
//!         This module builds on top of `ast/range.rs`:
//!             - `ast/range.rs` provides the foundation types (`Position`, `Range`, `SourceLocation`)
//!             - This module provides high-level helpers for AST construction
//!
//!         The separation maintains clean architecture:
//!             - `ast/range.rs` = Pure types with no AST dependencies
//!             - `building/location.rs` = Builder utilities that work with AST nodes

use std::ops::Range as ByteRange;

use crate::lex::ast::range::SourceLocation;
use crate::lex::ast::traits::AstNode;
use crate::lex::ast::{ContentItem, Range};

// ============================================================================
// BYTE RANGE TO AST RANGE CONVERSION
// ============================================================================

/// Convert a byte range to an AST Range (line:column positions)
///
/// This is the canonical implementation used throughout the AST building pipeline.
/// Converts byte offsets from token ranges to line/column coordinates
/// using the SourceLocation utility (O(log n) binary search).
///
/// # Arguments
///
/// * `range` - Byte offset range from the source string
/// * `source` - Original source string (needed to count newlines)
///
/// # Returns
///
/// An AST Range with line/column positions
pub(super) fn byte_range_to_ast_range(
    range: ByteRange<usize>,
    source_location: &SourceLocation,
) -> Range {
    source_location.byte_range_to_ast_range(&range)
}

// ============================================================================
// AST RANGE AGGREGATION
// ============================================================================

/// Compute location bounds from multiple locations
///
/// Creates a bounding box that encompasses all provided locations by taking:
/// - The minimum start line/column across all locations
/// - The maximum end line/column across all locations
///
/// This matches both parsers' approach for location aggregation.
///
/// Note: This function is public for use by parser implementations.
pub fn compute_location_from_locations(locations: &[Range]) -> Range {
    if locations.is_empty() {
        return Range::default();
    }

    let first = &locations[0];
    let mut start_pos = first.start;
    let mut start_byte = first.span.start;
    let mut end_pos = first.end;
    let mut end_byte = first.span.end;

    for loc in &locations[1..] {
        if loc.start < start_pos {
            start_pos = loc.start;
            start_byte = loc.span.start;
        } else if loc.start == start_pos {
            start_byte = start_byte.min(loc.span.start);
        }

        if loc.end > end_pos {
            end_pos = loc.end;
            end_byte = loc.span.end;
        } else if loc.end == end_pos {
            end_byte = end_byte.max(loc.span.end);
        }
    }

    Range::new(start_byte..end_byte, start_pos, end_pos)
}

/// Aggregate location from a primary location and child content items
///
/// Creates a bounding box that encompasses the primary location and all child content.
/// This is commonly used when building container nodes (sessions, lists, definitions)
/// that need to include the location of their title/header and all child items.
///
/// # Example
/// ```ignore
/// let location = aggregate_locations(title_location, &session_content);
/// ```
pub(super) fn aggregate_locations(primary: Range, children: &[ContentItem]) -> Range {
    let mut sources = vec![primary];
    sources.extend(children.iter().map(|item| item.range().clone()));
    compute_location_from_locations(&sources)
}

/// Create a default location (0,0)..(0,0)
///
/// Used when source span information is not available.
pub fn default_location() -> Range {
    Range {
        span: 0..0,
        start: crate::lex::ast::range::Position { line: 0, column: 0 },
        end: crate::lex::ast::range::Position { line: 0, column: 0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::range::Position;

    fn mk_range(span: std::ops::Range<usize>, start: (usize, usize), end: (usize, usize)) -> Range {
        Range::new(
            span,
            Position::new(start.0, start.1),
            Position::new(end.0, end.1),
        )
    }

    #[test]
    fn compute_location_respects_order() {
        let locations = vec![
            mk_range(5..10, (2, 3), (2, 7)),
            mk_range(1..4, (1, 0), (1, 4)),
            mk_range(10..15, (3, 0), (3, 2)),
        ];

        let result = compute_location_from_locations(&locations);

        assert_eq!(result.start, Position::new(1, 0));
        assert_eq!(result.end, Position::new(3, 2));
        assert_eq!(result.span, 1..15);
    }

    #[test]
    fn compute_location_handles_ties() {
        let locations = vec![
            mk_range(2..5, (0, 2), (0, 5)),
            mk_range(1..3, (0, 2), (0, 3)),
        ];

        let result = compute_location_from_locations(&locations);
        assert_eq!(result.start, Position::new(0, 2));
        assert_eq!(result.span.start, 1);
        assert_eq!(result.end, Position::new(0, 5));
        assert_eq!(result.span.end, 5);
    }

    #[test]
    fn compute_location_empty_returns_default() {
        let result = compute_location_from_locations(&[]);
        assert_eq!(result, Range::default());
    }
}
