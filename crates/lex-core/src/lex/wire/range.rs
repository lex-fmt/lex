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
//! [`OriginInterner`] dedupes the `Arc<PathBuf>` allocations: large
//! payloads with thousands of nodes share a handful of distinct
//! origin strings (typically just the loaded file's path), so we
//! keep one `Arc<PathBuf>` per unique string and clone the `Arc`
//! into each node's `Range.origin_path` rather than allocating a
//! fresh `PathBuf` per node.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::lex::ast::range::{Position as CorePosition, Range as CoreRange};
use lex_extension::wire::{Position as WirePosition, Range as WireRange};

/// Shared origin pool used while decoding a single wire payload.
///
/// `from_wire_subtree` typically walks a tree where 99% of nodes
/// share the same `origin` string (the loaded file's path). Without
/// interning, every node would allocate a fresh `Arc<PathBuf>`,
/// inflating memory by O(node-count). The interner caches one
/// `Arc<PathBuf>` per distinct origin string seen during the walk
/// and clones the `Arc` into each node's `origin_path`.
#[derive(Default)]
pub(crate) struct OriginInterner {
    cache: HashMap<String, Arc<PathBuf>>,
}

impl OriginInterner {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Return the cached `Arc<PathBuf>` for `s`, creating it on first
    /// sight. Callers `Arc::clone` the result rather than building a
    /// new path each time.
    pub(crate) fn intern(&mut self, s: &str) -> Arc<PathBuf> {
        if let Some(arc) = self.cache.get(s) {
            return Arc::clone(arc);
        }
        let arc = Arc::new(PathBuf::from(s));
        self.cache.insert(s.to_string(), Arc::clone(&arc));
        arc
    }
}

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
/// wire `origin` string, sharing the underlying `Arc<PathBuf>` with
/// every other node that has the same origin via `interner`.
pub(crate) fn range_from_wire_with_origin(
    r: &WireRange,
    origin: Option<&str>,
    interner: &mut OriginInterner,
) -> CoreRange {
    let mut range = range_from_wire(r);
    if let Some(s) = origin {
        range.origin_path = Some(interner.intern(s));
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

    #[test]
    fn interner_shares_arc_for_repeated_origins() {
        // Two `range_from_wire_with_origin` calls with the same
        // origin string must produce ranges whose `origin_path`
        // points at the *same* Arc allocation (Arc::ptr_eq), not
        // separate `PathBuf` clones.
        let mut interner = OriginInterner::new();
        let r1 = WireRange::new(WirePosition::new(0, 0), WirePosition::new(0, 0));
        let r2 = WireRange::new(WirePosition::new(1, 0), WirePosition::new(1, 0));

        let a = range_from_wire_with_origin(&r1, Some("/repo/file.lex"), &mut interner);
        let b = range_from_wire_with_origin(&r2, Some("/repo/file.lex"), &mut interner);

        let a_arc = a.origin_path.expect("a has origin");
        let b_arc = b.origin_path.expect("b has origin");
        assert!(
            Arc::ptr_eq(&a_arc, &b_arc),
            "interner must share Arc<PathBuf> for identical origin strings"
        );
    }

    #[test]
    fn interner_keeps_distinct_arcs_for_different_origins() {
        let mut interner = OriginInterner::new();
        let r = WireRange::new(WirePosition::new(0, 0), WirePosition::new(0, 0));
        let a = range_from_wire_with_origin(&r, Some("/repo/a.lex"), &mut interner);
        let b = range_from_wire_with_origin(&r, Some("/repo/b.lex"), &mut interner);
        assert!(
            !Arc::ptr_eq(
                a.origin_path.as_ref().unwrap(),
                b.origin_path.as_ref().unwrap()
            ),
            "different origin strings must keep separate Arcs"
        );
    }

    #[test]
    fn interner_treats_none_origin_as_unstamped() {
        let mut interner = OriginInterner::new();
        let r = WireRange::new(WirePosition::new(0, 0), WirePosition::new(0, 0));
        let a = range_from_wire_with_origin(&r, None, &mut interner);
        assert!(a.origin_path.is_none());
    }
}
