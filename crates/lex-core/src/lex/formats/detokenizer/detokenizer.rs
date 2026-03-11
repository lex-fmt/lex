//! Legacy detokenizer module.
//!
//! The token library now owns the implementation. This module
//! simply re-exports the canonical API to avoid breaking existing paths
//! and snapshot identifiers.

pub use crate::lex::token::formatting::{detokenize, ToLexString};
