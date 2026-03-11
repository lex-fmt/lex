//! AST Assertion Modules
//!
//! This module contains element-specific assertion types organized by element type.

mod annotation;
mod children;
mod data;
mod definition;
mod document;
mod inlines;
mod list;
mod paragraph;
mod session;
mod verbatim;

pub use annotation::AnnotationAssertion;
pub use children::ChildrenAssertion;
#[allow(unused_imports)]
pub use data::DataAssertion;
pub use definition::DefinitionAssertion;
pub use document::DocumentAssertion;
#[allow(unused_imports)]
pub use inlines::{InlineAssertion, InlineExpectation, ReferenceExpectation};
pub use list::{ListAssertion, ListItemAssertion};
pub use paragraph::ParagraphAssertion;
pub use session::SessionAssertion;
pub use verbatim::VerbatimBlockkAssertion;

use crate::lex::ast::traits::AstNode;
use crate::lex::ast::ContentItem;

// ============================================================================
// Helper Functions (shared across modules)
// ============================================================================

pub(super) fn summarize_items(items: &[ContentItem]) -> String {
    iter_visible(items)
        .map(|item| item.node_type())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn visible_len(items: &[ContentItem]) -> usize {
    iter_visible(items).count()
}

pub(super) fn visible_nth(items: &[ContentItem], index: usize) -> Option<&ContentItem> {
    iter_visible(items).nth(index)
}

pub(super) fn iter_visible(items: &[ContentItem]) -> impl Iterator<Item = &ContentItem> {
    items
        .iter()
        .filter(|item| !matches!(item, ContentItem::BlankLineGroup(_)))
}
