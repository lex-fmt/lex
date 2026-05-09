//! `Range` / `Position` conversion between lex-core and `lex_extension`.
//!
//! Forward path: drops `span` (byte offsets) and `origin_path`
//! (`origin_path` is lifted to the wire node's `origin` field by the
//! caller). Reverse path reconstructs `span = 0..0` since byte offsets
//! are advisory in spliced content; when callers thread the wire
//! `origin` string through [`range_from_wire_with_origin`], the
//! `origin_path` round-trips back into `Range.origin_path` so spliced
//! nodes carry the correct origin downstream.
//!

use std::path::PathBuf;
use std::sync::Arc;

use crate::lex::ast::range::{Position as CorePosition, Range as CoreRange};
use lex_extension::wire::{Position as WirePosition, Range as WireRange};

pub(crate) fn position_to_wire(p: &CorePosition) -> WirePosition {
    // Wire format pins line/column to u32. lex-core stores them as
    // usize because they index bytes within typical 64-bit address
    // space; values that exceed u32::MAX would mean a single document
    // containing >4 billion lines, which doesn't happen in practice.
    // Saturate-and-debug-assert so a future regression surfaces in
    // dev rather than producing a wrapped value silently.
    let line = u32::try_from(p.line).unwrap_or_else(|_| {
        debug_assert!(false, "position line {} exceeds u32::MAX", p.line);
        u32::MAX
    });
    let column = u32::try_from(p.column).unwrap_or_else(|_| {
        debug_assert!(false, "position column {} exceeds u32::MAX", p.column);
        u32::MAX
    });
    WirePosition::new(line, column)
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

/// Like [`range_from_wire`] but also restores `origin_path` from the
/// wire `origin` string. Use this when the parent context carries an
/// origin (e.g., the resolve pass splicing the result of a
/// `dispatch_resolve` call) so spliced nodes downstream can locate
/// their authoring file for things like file-reference resolution
/// and footnote scoping.
pub(crate) fn range_from_wire_with_origin(r: &WireRange, origin: Option<&str>) -> CoreRange {
    let mut range = range_from_wire(r);
    if let Some(s) = origin {
        range.origin_path = Some(Arc::new(PathBuf::from(s)));
    }
    range
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
