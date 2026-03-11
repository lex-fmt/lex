//! Individual transformation stages
//!
//! This module contains the individual stages that can be composed into pipelines.
//! Each stage implements the `Runnable` trait.

pub mod indentation;
pub mod inline_parsing;
pub mod parsing;
pub mod tokenization;

pub use indentation::SemanticIndentation;
pub use inline_parsing::ParseInlines;
pub use parsing::Parsing;
pub use tokenization::CoreTokenization;
