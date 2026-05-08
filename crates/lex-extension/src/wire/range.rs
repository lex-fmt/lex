//! Source-range types.
//!
//! The wire format encodes positions as `[line, column]` arrays — two-element
//! tuples — to keep payload size small and match the LSP convention. Both
//! values are 0-indexed; ranges are start-inclusive and end-exclusive.

use serde::{Deserialize, Serialize};

/// A `(line, column)` position. Wire form: `[line, column]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position(pub u32, pub u32);

impl Position {
    /// Construct a position from a line and column.
    pub fn new(line: u32, column: u32) -> Self {
        Self(line, column)
    }

    /// 0-indexed line.
    pub fn line(&self) -> u32 {
        self.0
    }

    /// 0-indexed column.
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
