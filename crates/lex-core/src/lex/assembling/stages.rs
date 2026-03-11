//! Individual assembling stages
//!
//! This module contains the assembling stages that process AST nodes after parsing.
//! Each stage implements the `Runnable` trait.

pub mod attach_annotations;
pub mod attach_root;

pub use attach_annotations::AttachAnnotations;
pub use attach_root::AttachRoot;
