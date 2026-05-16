//! Verbatim-bodied IR support helpers.
//!
//! Today this module hosts the table serializer
//! ([`table::serialize_pipe_table`]); the reverse direction
//! (pipe-table source → `WireNode::Table`) is owned by
//! `lex_core::lex::builtins::tabular::parse_pipe_table_to_wire`.
//!
//! ## Cleanup note
//!
//! - Pre-#594 this module hosted a `VerbatimHandler` trait and a
//!   `VerbatimRegistry` that the `LexSerializer` consulted to reformat
//!   pipe-table bodies during `lexd format`. That entire path was
//!   unreachable after PR #587 made `:: table ::` parse to a structural
//!   `DocNode::Table` and `NormalizeLabels` hard-rejected the legacy
//!   `doc.*` aliases.
//! - Pre-#615 this module hosted free `image_from_params` /
//!   `video_from_params` / `audio_from_params` helpers used by
//!   `from_lex_verbatim`'s wire→AST→IR fallback for media verbatim
//!   labels. The unified registry surface (#615) eliminated that
//!   fallback by typing the wire output directly into IR via
//!   `from_wire_typed`, so the helpers are gone.

pub mod table;
