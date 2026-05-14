//! Free hydration helpers for verbatim-bodied IR nodes.
//!
//! Lex source like `:: image src=foo.jpg ::` or `:: table ::` parses
//! to a generic `ContentItem::Verbatim` at the lex-core layer. The
//! IR translation (`from_lex_verbatim`) routes those through the
//! extension registry's `on_resolve` dispatch (#583), which produces
//! a typed `WireNode`; the wire codec then round-trips back to a
//! lex-core `Verbatim` carrying the canonical params. From there
//! these helpers extract the params into typed IR nodes
//! (`DocNode::Image`, `DocNode::Video`, `DocNode::Audio`).
//!
//! Table serialization (`Table` IR → pipe-table source) goes through
//! [`table::serialize_pipe_table`]; the reverse direction (pipe-table
//! source → `WireNode::Table`) is owned by `lex_core::lex::builtins::
//! tabular::parse_pipe_table_to_wire`.
//!
//! ## Cleanup note
//!
//! Pre-#594 this module also hosted a `VerbatimHandler` trait and a
//! `VerbatimRegistry` that the `LexSerializer` consulted to reformat
//! pipe-table bodies during `lexd format`. That entire path is
//! unreachable since PR #587 made `:: table ::` parse to a structural
//! `DocNode::Table` (with its own `LexSerializer::visit_table` arm)
//! and `NormalizeLabels` hard-rejected the legacy `doc.*` aliases
//! that used to route through the registry.

pub mod media;
pub mod table;
