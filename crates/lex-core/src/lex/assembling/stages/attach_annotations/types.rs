//! Shared types for annotation attachment

use crate::lex::ast::range::Range;

/// An entry representing an element in the content tree for attachment processing.
#[derive(Clone, Copy)]
pub struct Entry {
    pub kind: EntryKind,
    pub start_line: usize,
    pub end_line: usize,
}

/// The kind of entry - either content or annotation.
#[derive(Clone, Copy)]
pub enum EntryKind {
    Content(usize),
    Annotation(usize),
}

/// Result of searching for the next content element.
pub struct NextSearchResult {
    pub next: Option<(usize, usize)>,
    pub distance_to_end: usize,
}

/// A candidate target for annotation attachment with its distance.
pub(super) struct Candidate {
    pub distance: usize,
    pub target: AttachmentTarget,
}

/// The target where an annotation will be attached.
pub enum AttachmentTarget {
    Content(usize),
    Container,
}

/// Pending attachment to be processed.
pub struct PendingAttachment {
    pub annotation_index: usize,
    pub target: AttachmentTarget,
}

/// The kind of container being processed.
pub enum ContainerKind {
    DocumentRoot,
    Regular,
    Detached,
}

impl ContainerKind {
    pub fn is_document(&self) -> bool {
        matches!(self, ContainerKind::DocumentRoot)
    }
}

/// Span information for a container.
#[derive(Clone, Copy)]
pub struct ContainerSpan {
    pub end_line: usize,
}

impl ContainerSpan {
    pub fn from_range(range: &Range) -> Self {
        ContainerSpan {
            end_line: range.end.line,
        }
    }
}
