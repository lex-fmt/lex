//! Source-range types.
//!
//! The wire format encodes positions as `[line, column]` arrays — two-element
//! tuples — to keep payload size small. Both values are 0-indexed; ranges
//! are start-inclusive and end-exclusive.
//!
//! # Column semantics
//!
//! `column` is a **0-indexed UTF-8 byte offset** within the line, matching
//! lex-core's internal source representation (`byte_offset - line_start`).
//! It is *not* an LSP-style UTF-16 code unit count and *not* a Unicode
//! scalar count; multi-byte characters occupy more than one column. The
//! `lex-lsp` server converts to UTF-16 code units at the LSP protocol
//! boundary; that conversion is not the wire format's concern.

use serde::{Deserialize, Serialize};

/// A `(line, column)` position. Wire form: `[line, column]`.
///
/// `column` is a 0-indexed UTF-8 byte offset within the line — see the
/// module-level docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position(pub u32, pub u32);

impl Position {
    /// Construct a position from a line and column (0-indexed UTF-8 byte
    /// offset).
    pub fn new(line: u32, column: u32) -> Self {
        Self(line, column)
    }

    /// 0-indexed line.
    pub fn line(&self) -> u32 {
        self.0
    }

    /// 0-indexed UTF-8 byte offset within the line.
    pub fn column(&self) -> u32 {
        self.1
    }
}

/// A half-open source range. `start` inclusive, `end` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_serialises_as_array() {
        let p = Position::new(12, 4);
        let s = serde_json::to_string(&p).unwrap();
        assert_eq!(s, "[12,4]");
    }

    #[test]
    fn position_deserialises_from_array() {
        let p: Position = serde_json::from_str("[12,4]").unwrap();
        assert_eq!(p, Position::new(12, 4));
    }

    #[test]
    fn range_round_trips() {
        let r = Range::new(Position::new(1, 2), Position::new(3, 4));
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(s, r#"{"start":[1,2],"end":[3,4]}"#);
        let back: Range = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }
}
