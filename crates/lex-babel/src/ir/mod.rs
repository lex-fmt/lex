//! Intermediate Representation (IR) for lex documents.
//!
//! This module defines a format-agnostic representation of a lex document,
//! designed to facilitate conversion to various output formats like HTML,
//! Markdown, etc.
//!
//! # Round-trip contract
//!
//! `from_lex(to_lex(ir))` and `to_lex(from_lex(ast))` are structurally
//! lossless for the v1 element set with the explicit exceptions listed
//! below. Per-`DocNode` IR round-trip proptests live in
//! `crates/lex-babel/tests/ir_round_trip_proptest/mod.rs` (the
//! `tests/round_trip_proptest/` suite is the older AST round-trip
//! coverage) and are the executable form of this contract — they pin
//! the behaviour so changes here can't drift silently.
//!
//! ## Accepted losses (v1)
//!
//! These are documented, deliberate gaps where the IR doesn't faithfully
//! mirror the lex-core AST. Each is paired with a proptest pinning the
//! actual behaviour.
//!
//! - **Heading levels.** [`nodes::Heading`] carries a `level: usize`
//!   field, but `to_lex_heading` reconstructs nesting from parent
//!   context rather than the level itself — a heading rendered out of
//!   its session context can shift level. See `to_lex.rs`'s
//!   `to_lex_heading` for the reconstruction logic.
//! - **Inline-format nesting.** `Bold([Italic([Text("x")])])` flattens
//!   to the text `*_x_*` on the way back through
//!   `to_lex_inline_content` (see `to_lex.rs`). Round-tripping nested
//!   inline formatting strictly is out of scope for v1; the proptests
//!   assert text equivalence rather than structural equivalence inside
//!   inline nesting.
//! - **Video / Audio inline.** [`nodes::DocNode::Video`] and
//!   [`nodes::DocNode::Audio`] exist as block-level nodes; there are
//!   no `InlineContent::Video` / `InlineContent::Audio` variants.
//!   Inline-positioned video/audio (rare in practice) are unrepresentable
//!   in v1.
//! - **Table caption + fullwidth.** [`nodes::Table`] carries `caption`
//!   and `fullwidth` fields, but the pipe-table verbatim form emitted
//!   by `to_lex_table` (see `common/verbatim/table.rs`) does not encode
//!   them. A `Table { caption: Some(_), … }` round-trips with
//!   `caption: None`. Footnotes survive via nested `lex.footnote.*`
//!   annotations.
//! - **Linkable references resolve to `Link`.** A bare
//!   `InlineContent::Reference { kind: Url|File|Session, .. }` in a
//!   paragraph is rewritten to `InlineContent::Link { text, href }` by
//!   `common/links.rs::resolve_implicit_anchors` on every lex → IR
//!   conversion. This is a deliberate shape change (#570 anchor
//!   heuristic), not an information loss in user-visible terms: the
//!   reference target survives as `Link.href`. But the typed
//!   `ReferenceType` does *not* survive — `Link` carries only strings,
//!   and consumers that want the classification back must re-infer it
//!   from `href`. Round-trip tests asserting "same shape both sides"
//!   must account for the rewrite.
//!
//! Heading, inline-format nesting, table caption / fullwidth, and the
//! Reference → Link rewrite are the only *content-shape* divergences.
//! Everything else — reference sub-type classification (#614), verbatim
//! closing-data parameters (#614 follow-up), annotation
//! [`nodes::LabelForm`] (#593), table footnotes, document annotations
//! (#570 Phase 3b) — round-trips losslessly as of the #613 symmetry
//! work-stream.

pub mod events;
pub mod from_lex;
pub mod nodes;
pub mod to_events;
pub mod to_lex;
pub mod to_wire;
