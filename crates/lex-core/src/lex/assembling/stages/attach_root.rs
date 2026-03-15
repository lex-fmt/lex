//! Document attachment stage
//!
//! This stage receives the build output (title + root session) from the building phase
//! and assembles it into a `Document` node. Downstream assembling steps (like
//! annotation attachment) can then operate on the complete document structure.

use crate::lex::ast::Document;
use crate::lex::building::ast_tree::BuildOutput;
use crate::lex::transforms::{Runnable, TransformError};

/// Attach the build output (title + root session) to a `Document` node.
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

impl Runnable<BuildOutput, Document> for AttachRoot {
    fn run(&self, output: BuildOutput) -> Result<Document, TransformError> {
        Ok(Document::from_title_and_root(output.title, output.root))
    }
}
