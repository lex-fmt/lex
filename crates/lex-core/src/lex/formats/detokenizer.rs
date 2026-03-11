//! Detokenizer format module declaration
//!
//! Unlike other formatters which work on AST Document objects,
//! the detokenizer operates at the token level, converting token
//! streams back to source text.

#[allow(clippy::module_inception)]
pub mod detokenizer;

pub use detokenizer::{detokenize, ToLexString};
