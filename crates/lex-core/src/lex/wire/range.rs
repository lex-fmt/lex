//! `Range` / `Position` conversion between lex-core and `lex_extension`.
//!
//! Forward path: drops `span` (byte offsets) and `origin_path`
//! (`origin_path` is lifted to the wire node's `origin` field by the
//! caller). Reverse path reconstructs `span = 0..0` since byte offsets
//! are advisory in spliced content.

use crate::lex::ast::range::{Position as CorePosition, Range as CoreRange};
use lex_extension::wire::{Position as WirePosition, Range as WireRange};

pub(crate) fn position_to_wire(p: &CorePosition) -> WirePosition {
    WirePosition::new(p.line as u32, p.column as u32)
}

pub(crate) fn position_from_wire(p: &WirePosition) -> CorePosition {
    CorePosition::new(p.line() as usize, p.column() as usize)
}

pub(crate) fn range_to_wire(r: &CoreRange) -> WireRange {
    WireRange::new(position_to_wire(&r.start), position_to_wire(&r.end))
}

pub(crate) fn range_from_wire(r: &WireRange) -> CoreRange {
    CoreRange::new(
        0..0,
        position_from_wire(&r.start),
        position_from_wire(&r.end),
    )
}

/// Lift a lex-core `Range`'s `origin_path` to the wire `origin` string.
#[allow(dead_code)] // consumed by to_wire.rs starting in the next module
pub(crate) fn origin_string(r: &CoreRange) -> Option<String> {
    r.origin_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_round_trip() {
        let core = CorePosition::new(12, 34);
        let wire = position_to_wire(&core);
        assert_eq!(wire, WirePosition::new(12, 34));
        let back = position_from_wire(&wire);
        assert_eq!(back.line, 12);
        assert_eq!(back.column, 34);
    }

    #[test]
    fn range_round_trip() {
        let core = CoreRange::new(10..20, CorePosition::new(1, 2), CorePosition::new(1, 12));
        let wire = range_to_wire(&core);
        let back = range_from_wire(&wire);
        // span is dropped, but line/col are preserved
        assert_eq!(back.start.line, 1);
        assert_eq!(back.start.column, 2);
        assert_eq!(back.end.line, 1);
        assert_eq!(back.end.column, 12);
    }
}
