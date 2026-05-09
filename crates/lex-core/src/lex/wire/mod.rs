//! Wire-AST codec: lex-core internal AST ↔ `lex_extension::WireNode`.
//!
//! This module bridges lex-core's typed AST (`Document`, `ContentItem` and
//! friends) to the wire-format types defined in the public `lex-extension`
//! crate. The codec is what lets the registry-driven resolve pass round-trip
//! handler-returned wire ASTs back into typed lex-core nodes for splicing.
//!
//! # Status: skeleton only
//!
//! This module is the placeholder landed in PR 3a (lex-fmt/lex#519); the
//! actual codec implementation lands in PR 3b (lex-fmt/lex#531). PR 3c
//! ([lex-fmt/lex#532](https://github.com/lex-fmt/lex/issues/532)) is the
//! first consumer (`LexIncludeHandler`); PR 3d
//! ([lex-fmt/lex#533](https://github.com/lex-fmt/lex/issues/533)) wires
//! the codec into the resolve pass.
//!
//! # Design overview (forward reference for PR 3b)
//!
//! - **Forward** (`Document → WireNode`): a total walk over lex-core's AST
//!   that produces a `WireNode::Document` rooted at the document's root
//!   session. Per-variant conversion for sessions, definitions, paragraphs,
//!   annotations, blank groups, lists, tables, and verbatim blocks.
//! - **Reverse** (`WireNode → Vec<ContentItem>`): fallible — wire input may
//!   be malformed. The reverse direction is what the registry-driven
//!   resolve pass uses to splice handler output back into the host AST.
//! - **Versioning**: this codec speaks `lex_extension::WIRE_VERSION = 1`.
//!   The codec lives next to the AST it converts so internal AST changes
//!   that affect the wire format are caught at compile time here, not
//!   downstream in handler crates.
