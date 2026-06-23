//! Converts a flat event stream back to a nested IR tree structure.
//!
//! # The High-Level Concept
//!
//! The core challenge is to reconstruct a tree structure from a linear sequence of events.
//! The algorithm uses a stack to keep track of the current nesting level. The stack acts as
//! a memory of "open" containers. When we encounter a `Start` event for a container (like a
//! heading or list), we push it onto the stack, making it the new "current" container. When
//! we see its corresponding `End` event, we pop it off, returning to the parent container.
//!
//! # Auto-Closing Headings (For Flat Formats)
//!
//! This converter includes special logic for headings to support flat document formats
//! (Markdown, HTML, LaTeX) where headings don't have explicit close markers. When a new
//! `StartHeading(level)` event is encountered, the converter automatically closes any
//! currently open headings at the same or deeper level before opening the new heading.
//!
//! This means format parsers can simply emit `StartHeading` events without worrying about
//! emitting matching `EndHeading` events - the generic converter handles the hierarchy.
//!
//! Example event stream from Markdown parser:
//! ```text
//! StartDocument
//! StartHeading(1)         <- Opens h1
//! StartHeading(2)         <- Auto-closes nothing, opens h2 nested in h1
//! StartHeading(1)         <- Auto-closes h2 and previous h1, opens new h1
//! EndDocument             <- Auto-closes remaining h1
//! ```
//!
//! # The Algorithm
//!
//! 1. **Initialization:**
//!    - Create the root `Document` node
//!    - Create an empty stack
//!    - Push the root onto the stack as the current container
//!
//! 2. **Processing `Start` Events:**
//!    - Create a new empty `DocNode` for that element
//!    - Add it as a child to the current parent (top of stack)
//!    - Push it onto the stack as the new current container
//!
//! 3. **Processing Content Events (Inline):**
//!    - Add the content to the current parent (top of stack)
//!    - Do NOT modify the stack (content is a leaf)
//!
//! 4. **Processing `End` Events:**
//!    - Pop the node off the stack
//!    - Validate that the popped node matches the End event
//!
//! 5. **Completion:**
//!    - The stack should contain only the root Document node
//!    - This root contains the complete reconstructed AST
//!
//! # Module layout
//!
//! This module is organised as a façade (precedent:
//! `crates/lex-core/src/lex/includes.rs`). The public surface
//! ([`ConversionError`], [`events_to_tree`]) lives here; the
//! implementation is split into cohesive submodules:
//!
//! - `stack_node` — the [`StackNode`](stack_node::StackNode) enum and its
//!   build/finalize machinery.
//! - `build` — [`events_to_tree`](build::events_to_tree) (the main
//!   algorithm) plus its heading auto-close helpers.

mod build;
mod stack_node;

pub use build::events_to_tree;

// The `tests` submodule (`flat_to_nested/tests.rs`) imports the converter
// surface through `use super::*`. Besides `ConversionError` and the
// re-exported `events_to_tree`, it relies on `Event` and the IR node types
// being reachable as `super::*`; surface them for the test build only so the
// public API gains no new paths.
#[cfg(test)]
use crate::ir::events::Event;
#[cfg(test)]
use crate::ir::nodes::*;

/// Error type for flat-to-nested conversion
#[derive(Debug, Clone, PartialEq)]
pub enum ConversionError {
    /// Stack was empty when trying to pop
    UnexpectedEnd(String),
    /// Mismatched start/end events
    MismatchedEvents { expected: String, found: String },
    /// Unexpected inline content in wrong context
    UnexpectedInline(String),
    /// Events remaining after document end
    ExtraEvents,
    /// Stack not empty at end (unclosed containers)
    UnclosedContainers(usize),
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversionError::UnexpectedEnd(msg) => write!(f, "Unexpected end event: {msg}"),
            ConversionError::MismatchedEvents { expected, found } => {
                write!(f, "Mismatched events: expected {expected}, found {found}")
            }
            ConversionError::UnexpectedInline(msg) => {
                write!(f, "Unexpected inline content: {msg}")
            }
            ConversionError::ExtraEvents => write!(f, "Extra events after document end"),
            ConversionError::UnclosedContainers(count) => {
                write!(f, "Unclosed containers: {count} nodes remain on stack")
            }
        }
    }
}

impl std::error::Error for ConversionError {}

#[cfg(test)]
mod tests;
