//! Distance calculation and attachment decision logic
//!
//!     This module handles the core logic for determining where annotations should attach
//!     based on proximity (blank line distance) to surrounding content elements.
//!
//!     Annotations attach to AST nodes based on "human understanding" distance between
//!     elements. This is harder than it seems. Keeping Lex ethos of not enforcing structure,
//!     this needs to deal with several ambiguous cases, including some complex logic for
//!     calculating this distance.
//!
//!     The distance is measured by the number of blank lines separating an annotation from
//!     content elements. The closest element (fewest blank lines) wins. If equidistant, the
//!     next element wins. Special rules apply for document-level and container-end cases.

use std::cmp::Ordering;

use super::types::{
    AttachmentTarget, Candidate, ContainerKind, ContainerSpan, Entry, EntryKind, NextSearchResult,
};

/// Find the previous content element and its distance from the given entry.
///
/// # Returns
/// - `Some((distance, content_index))` if a previous content element exists
/// - `None` if the entry is at the start or only annotations precede it
pub fn find_previous_content(entries: &[Entry], entry_index: usize) -> Option<(usize, usize)> {
    if entry_index == 0 {
        return None;
    }

    let mut distance = 0;
    let mut cursor = entry_index;

    while cursor > 0 {
        let prev_idx = cursor - 1;
        distance += blank_lines_between(&entries[prev_idx], &entries[cursor]);
        cursor = prev_idx;

        match entries[cursor].kind {
            EntryKind::Content(idx) => return Some((distance, idx)),
            EntryKind::Annotation(_) => continue,
        }
    }

    None
}

/// Find the next content element and its distance from the given entry.
///
/// Also calculates the distance to the container end, which is used for
/// container-end attachment rules.
///
/// # Returns
/// A `NextSearchResult` containing:
/// - `next`: `Some((distance, content_index))` if a next content element exists, `None` otherwise
/// - `distance_to_end`: distance from the entry to the container boundary
pub fn find_next_content(
    entries: &[Entry],
    entry_index: usize,
    container_span: &ContainerSpan,
) -> NextSearchResult {
    let mut distance = 0;
    let mut cursor = entry_index;

    while cursor + 1 < entries.len() {
        let next_idx = cursor + 1;
        distance += blank_lines_between(&entries[cursor], &entries[next_idx]);
        cursor = next_idx;

        match entries[cursor].kind {
            EntryKind::Content(idx) => {
                return NextSearchResult {
                    next: Some((distance, idx)),
                    distance_to_end: distance,
                }
            }
            EntryKind::Annotation(_) => continue,
        }
    }

    NextSearchResult {
        next: None,
        distance_to_end: blank_lines_to_end(&entries[cursor], container_span),
    }
}

/// Calculate the blank line gap immediately after an entry.
///
/// This is used to determine document-level attachment rules.
pub fn blank_gap_after(
    entries: &[Entry],
    entry_index: usize,
    container_span: &ContainerSpan,
) -> usize {
    if entry_index + 1 < entries.len() {
        blank_lines_between(&entries[entry_index], &entries[entry_index + 1])
    } else {
        blank_lines_to_end(&entries[entry_index], container_span)
    }
}

/// Decide where an annotation should attach based on proximity to surrounding content.
///
/// # Attachment Rules
/// 1. Document-start: Annotations at document start followed by a blank line attach to Document
/// 2. Closest wins: Otherwise, attach to the closest content element
/// 3. Tie-breaker: If equidistant, the next element wins
/// 4. Container-end: When no next content exists, may attach to container if allowed
///
/// # Arguments
/// - `previous`: Distance and index of previous content element, if any
/// - `next`: Distance and index of next content element, if any
/// - `distance_to_end`: Distance from annotation to container end
/// - `blank_after`: Number of blank lines immediately after the annotation
/// - `kind`: The kind of container (Document, Regular, or Detached)
/// - `container_allowed`: Whether the container itself can receive annotations
///
/// # Returns
/// The attachment target, or `None` if no valid target exists
pub fn decide_attachment(
    previous: Option<(usize, usize)>,
    next: Option<(usize, usize)>,
    distance_to_end: usize,
    blank_after: usize,
    kind: &ContainerKind,
    container_allowed: bool,
) -> Option<AttachmentTarget> {
    // Rule 1: Document-level attachment
    if kind.is_document() && previous.is_none() && blank_after > 0 {
        return Some(AttachmentTarget::Container);
    }

    // Build candidates
    let prev_candidate = previous.map(|(distance, idx)| Candidate {
        distance,
        target: AttachmentTarget::Content(idx),
    });

    let next_candidate = match next {
        Some((distance, idx)) => Some(Candidate {
            distance,
            target: AttachmentTarget::Content(idx),
        }),
        None if container_allowed => Some(Candidate {
            distance: distance_to_end,
            target: AttachmentTarget::Container,
        }),
        None => None,
    };

    // Compare candidates and decide
    match (prev_candidate, next_candidate) {
        (Some(prev), Some(next)) => match prev.distance.cmp(&next.distance) {
            Ordering::Less => Some(prev.target),
            Ordering::Greater | Ordering::Equal => Some(next.target),
        },
        (Some(prev), None) => Some(prev.target),
        (None, Some(next)) => Some(next.target),
        (None, None) => None,
    }
}

/// Calculate the number of blank lines between two AST entries.
///
/// For multi-line elements (paragraphs, annotations spanning multiple lines),
/// we need to handle the case where elements might be adjacent or overlapping
/// in line numbers. The calculation uses the end line of the left element and
/// determines the effective start of the right element.
///
/// # Edge Cases
/// - If right starts before left ends (overlapping/adjacent multi-line elements):
///   Use right's end line as the effective start to ensure correct distance.
/// - If elements are on consecutive lines with no blank lines between: returns 0.
/// - Otherwise: counts the gap between left.end_line and right's effective start.
pub fn blank_lines_between(left: &Entry, right: &Entry) -> usize {
    // For multi-line elements, determine the effective starting line of the right element.
    // If right starts at or before left ends (overlapping ranges), use right's end line
    // as the effective start. This handles cases like multi-line annotations adjacent
    // to multi-line paragraphs.
    let effective_start = if right.start_line <= left.end_line {
        right.end_line // Overlapping/adjacent elements
    } else {
        right.start_line
    };

    // Calculate blank lines: if effective_start is more than one line after left.end_line,
    // there are blank lines in between. The formula is: gap - 1 to exclude the line
    // boundaries themselves.
    if effective_start > left.end_line + 1 {
        effective_start - left.end_line - 1
    } else {
        0
    }
}

/// Calculate the number of blank lines from an entry to the end of its container.
///
/// This is used for container-end attachment rules: when an annotation is the last
/// element in a container, we measure its distance to the container's closing boundary.
///
/// # Arguments
/// - `entry`: The AST entry (typically an annotation at container end)
/// - `span`: The container's span information (contains end line)
///
/// # Returns
/// The number of blank lines between the entry and container end, or 0 if adjacent.
pub fn blank_lines_to_end(entry: &Entry, span: &ContainerSpan) -> usize {
    if span.end_line > entry.end_line + 1 {
        span.end_line - entry.end_line - 1
    } else {
        0
    }
}
