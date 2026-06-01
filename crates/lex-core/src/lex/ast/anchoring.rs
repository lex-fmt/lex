//! Whole-element anchoring: reference lines and their resolved anchors.
//!
//! A *reference line* is a line whose only content (after indentation) is a
//! single bracketed reference, e.g. a line that is exactly `[./readme.txt]`.
//! Per `specs/.../references-general.lex` §2.3.2–§2.3.4 such a line anchors the
//! *entire head line of the element directly above it* (a session title, a list
//! item's own line, a definition's subject term, a verbatim subject, or a
//! paragraph line). It never attaches downward, and when there is no content
//! line directly above it (first line of its container, or preceded by a blank
//! line) it *self-links* — it stands alone and links its own text, exactly like
//! a lone inline reference.
//!
//! Reference lines are removed from the line stream *before* structural parsing
//! (see [`crate::lex::anchoring`]), so they are transparent to the
//! definition-vs-session decision. The removal pass also resolves each line's
//! anchor here, against the original (pre-removal) source, so every range below
//! is in original-source coordinates — which is what editors and serializers
//! need, since the document the user sees still contains the reference lines.

use super::range::Range;
use crate::lex::inlines::ReferenceInline;

/// A reference line and its resolved anchor.
///
/// Collected on [`crate::lex::ast::Document`] as a queryable, document-level
/// list (`Document::reference_lines()`), so downstream consumers — the babel
/// serializers and the LSP `documentLink` provider — can read the anchor
/// without re-deriving the line-adjacency rules.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceLine {
    /// The bracketed reference itself (raw text + classified type).
    pub reference: ReferenceInline,
    /// Source range covering the `[bracketed]` reference, in original-source
    /// coordinates (brackets inclusive).
    pub reference_range: Range,
    /// The resolved anchor for this reference line.
    pub anchor: ReferenceAnchor,
}

/// The resolved anchor of a reference line.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceAnchor {
    /// Anchors the whole head line of the element directly above.
    WholeElement {
        /// The element's head-line text the link wraps (list marker and the
        /// definition's trailing `:` excluded — the head line *only*).
        anchor_text: String,
        /// Source range of `anchor_text`, in original-source coordinates.
        anchor_range: Range,
        /// What kind of element head line was anchored.
        element: AnchoredElement,
    },
    /// No content line directly above: the reference line links its own text,
    /// exactly like a lone inline reference (§2.3.2).
    SelfLink,
}

impl ReferenceAnchor {
    /// True when this reference line took a whole-element anchor.
    pub fn is_whole_element(&self) -> bool {
        matches!(self, ReferenceAnchor::WholeElement { .. })
    }
}

/// The head-line shape a reference line anchored.
///
/// The anchor is resolved against the original source line directly above the
/// reference line, so the classification reflects what that line *looks like*,
/// which is also exactly what determines how much of it the anchor covers:
///
/// - [`AnchoredElement::ListItem`] — the line opens with a list marker (`- `,
///   `1. `, `a) `, …); the marker is excluded from the anchor.
/// - [`AnchoredElement::Subject`] — the line ends with a `:` subject marker (a
///   definition term, a colon-style session title, or a verbatim subject); the
///   trailing colon is excluded from the anchor.
/// - [`AnchoredElement::WholeLine`] — any other head line (a plain paragraph
///   line, or a session title written without a colon); the whole line is the
///   anchor.
///
/// Session title vs. paragraph vs. verbatim subject are *not* distinguished
/// here because they are indistinguishable from the head line alone and, more
/// importantly, anchor identically (the whole line, modulo the colon rule). The
/// behaviorally-meaningful distinctions — marker stripping and colon stripping
/// — are the ones captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchoredElement {
    /// The head line opens with a list marker, excluded from the anchor.
    ListItem,
    /// The head line ends with a `:` subject marker, excluded from the anchor.
    Subject,
    /// A plain head line; the whole line is the anchor.
    WholeLine,
}
