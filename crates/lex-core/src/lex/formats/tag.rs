//! Tag format module declaration

#[allow(clippy::module_inception)]
pub mod tag;

pub use tag::{serialize_document, TagFormatter};
