//! Output format implementations for AST and token serialization
//!
//! This module contains different format implementations for serializing
//! AST Documents to various output formats (tag, treeviz).
//!
//! Token streams are serialized back to source text by the canonical
//! detokenizer in [`crate::lex::token::formatting`].

pub mod registry;
pub mod tag;
pub mod treeviz;

pub use registry::{FormatError, FormatRegistry, Formatter};
pub use tag::{serialize_document as serialize_ast_tag, TagFormatter};
pub use treeviz::{to_treeviz_str, TreevizFormatter};
