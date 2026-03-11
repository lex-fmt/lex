//! Document attachment stage
//!
//! This stage receives the fully built root session tree from the building phase
//! and attaches it to a `Document` node. Downstream assembling steps (like
//! annotation attachment) can then operate on the complete document structure.

use crate::lex::ast::{Document, Session};
use crate::lex::transforms::{Runnable, TransformError};

/// Attach the root session returned by the builder to a `Document` node.
pub struct AttachRoot;

impl AttachRoot {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AttachRoot {
    fn default() -> Self {
        Self::new()
    }
}

impl Runnable<Session, Document> for AttachRoot {
    fn run(&self, root: Session) -> Result<Document, TransformError> {
        Ok(Document::from_root(root))
    }
}
