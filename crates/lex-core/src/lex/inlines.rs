//! Inline parsing primitives
//!
//!     This module exposes the inline AST nodes plus the parser for flat inline elements
//!     (formatting, code, math). Later stages layer references and citations on top of the
//!     same building blocks.
//!
//!     Immediately after building (before annotations are attached in the assembly stage),
//!     we parse the TextContent nodes for inline elements. This parsing is much simpler than
//!     block parsing, as it has formal start/end tokens and has no structural elements.
//!
//!     Inline parsing is done by a declarative engine that will process each element declaration.
//!     For some, this is a flat transformation (i.e. it only wraps up the text into a node, as
//!     in bold or italic). Others are more involved, as in references, in which the engine will
//!     execute a callback with the text content and return a node.
//!
//!     This solves elegantly the fact that most inlines are simple and very much the same
//!     structure, while allowing for more complex ones to handle their specific needs.
//!
//!     See [parser](parser) module for the inline parser implementation.

mod citations;
pub mod math;
mod parser;
mod references;

pub use crate::lex::ast::elements::inlines::{
    InlineContent, InlineNode, PageFormat, ReferenceInline, ReferenceType,
};
pub use crate::lex::token::InlineKind;
pub use parser::{
    parse_inlines, parse_inlines_with_parser, InlineParser, InlinePostProcessor, InlineSpec,
};
