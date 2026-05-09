//! Wire-AST codec: lex-core internal AST ↔ `lex_extension::WireNode`.
//!
//! This module bridges lex-core's typed AST (`Document`, `ContentItem` and
//! friends) to the wire-format types defined in the public `lex-extension`
//! crate. The codec is what lets the registry-driven resolve pass round-trip
//! handler-returned wire ASTs back into typed lex-core nodes for splicing.
//!
//! # Direction
//!
//! - [`to_wire_node`] — forward: total over the AST shapes a parsed lex
//!   document can produce. Output is a [`lex_extension::WireNode`] tree.
//! - [`from_wire_node`] — reverse: fallible. Recognised `WireNode`
//!   variants become lex-core [`crate::lex::ast::ContentItem`]s; unknown
//!   shapes return [`FromWireError::UnsupportedKind`].
//!
//! # Lossy in places, by design
//!
//! The forward codec preserves *block structure* but drops several
//! representation-only details that the wire format does not have
//! slots for:
//!
//! - `Range::span` (byte offsets) — the wire format encodes only
//!   `(line, column)`. Reverse codec reconstructs `span = 0..0` since
//!   spliced content's byte offsets are advisory.
//! - **Inline-attached annotations** on inline nodes — wire
//!   `WireInline` doesn't carry annotation slots.
//! - **Block-level annotations** on `Paragraph`, `Session`, `List`,
//!   `Table`, etc. — none of the wire `WireNode` variants carry an
//!   `annotations` field, so attached annotations are dropped in the
//!   forward direction. (Standalone `ContentItem::Annotation` nodes
//!   *are* round-tripped fully via `WireNode::Annotation`.)
//! - **Document-level metadata** — `Document.title` and
//!   `Document.annotations` are dropped: the forward codec returns a
//!   `WireNode::Document` whose `children` are the root session's
//!   children, and only those.
//! - **Marker structure** on sessions and lists — the wire format
//!   stringifies the marker (`"1.1."`, `"(a)"`); the parser
//!   reconstructs the typed marker on the next parse.
//! - **`TextContent`** uses the parsed-inline path
//!   ([`TextContent::inline_nodes`]) when available (Phase 2),
//!   producing matching `WireInline` variants; otherwise emits the
//!   raw source as a single `WireInline::Text`. Reverse codec
//!   re-serialises through a `.lex` source-form string that the
//!   parser re-interprets identically.
//!
//! For the consumer that matters today (`LexIncludeHandler` in PR 3c),
//! these losses are invisible: the spliced content renders to the same
//! `.lex` source as the original.
//!
//! # Versioning
//!
//! This codec speaks `lex_extension::WIRE_VERSION = 1`. Wire-format
//! changes that bump that constant require codec updates here.

mod error;
pub mod from_wire;
mod inline;
mod range;
pub mod to_wire;

#[cfg(test)]
mod tests;

pub use error::FromWireError;
pub use from_wire::{from_wire_node, from_wire_subtree};
pub use to_wire::{to_wire_document, to_wire_node};
