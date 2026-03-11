//! Inline AST nodes shared across formatting, literal, and reference elements.
//!
//! These nodes are intentionally lightweight so the inline parser can be used
//! from unit tests before it is integrated into the higher level AST builders.

mod base;
mod references;

pub use base::{InlineContent, InlineNode};
pub use references::{
    CitationData, CitationLocator, PageFormat, PageRange, ReferenceInline, ReferenceType,
};
