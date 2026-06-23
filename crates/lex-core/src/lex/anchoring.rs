//! Reference-line extraction and whole-element anchor resolution.
//!
//! Implements §2.3.2–§2.3.4 of the references spec
//! (`comms/specs/elements/inlines.docs/specs/references/references-general.lex`).
//!
//! # What a reference line is
//!
//! A *reference line* is a physical source line whose only content, after
//! stripping leading indentation, is a single bracketed reference — the line is
//! exactly `[<inner>]` with nothing else. When `<inner>` classifies to a
//! *link-like* reference type (`Url`, `File`, `Session`, `General`; see
//! [`ReferenceType::anchoring`]) the line takes a **whole-element anchor**: it
//! anchors the entire head line of the element directly above it. A
//! marker-style reference (`[1]`, `[@key]`, `[::label]`) on its own line is
//! *not* a reference line for anchoring purposes — it self-links / resolves as
//! usual, exactly like an inline marker.
//!
//! # Why this is a pre-pass
//!
//! A reference line is *transparent* to structural parsing (§2.3.3): it is
//! neither a content line nor a blank line. Its tokens are removed from the
//! token stream **before** structure is resolved, so the lines around it keep
//! their original adjacency. This matters because a blank line after a subject
//! is exactly what separates a *definition* from a *session*:
//!
//! ```text
//! API Endpoint:
//! [./endpoint.txt]
//!     A URL that provides access to a resource.
//! ```
//!
//! Removing (not blanking) the reference line keeps `API Endpoint:` immediately
//! adjacent to its indented body, so it stays a **definition** and the
//! reference line anchors the term "API Endpoint". Blanking it would wrongly
//! turn it into a session.
//!
//! To make removal a token-stream operation (rather than a source-string edit
//! that would shift every downstream byte offset), the pre-pass reports the
//! **byte range of each removed reference line** — the line plus its terminating
//! newline. The caller lexes the *original* source and drops every token whose
//! range falls inside a removed line before parsing. The newline is included so
//! the content line above and the line below become directly adjacent.
//!
//! # Coordinates
//!
//! Nothing here edits the source string, and the caller parses the original
//! source with token ranges intact, so **every AST range stays in
//! original-source coordinates** — including elements that appear *after* a
//! reference line. Every [`Range`] this module produces is likewise in
//! original-source coordinates. That is what the document the user edits still
//! contains, which is what editors (LSP `documentLink`) and serializers need.
//!
//! # Module layout
//!
//! The two roles live in sibling submodules, re-exported here so the
//! `lex_core::lex::anchoring::` paths are unchanged:
//!
//! - [`lines`] — physical-line classification and head-line extraction (the
//!   primitives this pre-pass reads over, incl. verbatim-body protection).
//! - [`words`] — inline word-anchor resolution (§2.3.1), a distinct role that
//!   operates on already-parsed inline nodes ([`resolve_word_anchors`]).

use crate::lex::ast::anchoring::{AnchoredElement, ReferenceAnchor, ReferenceLine};
use crate::lex::ast::diagnostics::{Diagnostic, DiagnosticSeverity};
use crate::lex::ast::range::SourceLocation;
use crate::lex::inlines::{determine_reference_type, AnchorKind, ReferenceInline};
use std::ops::Range;

mod lines;
mod words;

#[cfg(test)]
mod tests;

use lines::{
    bracketed_inner, head_line_anchor, head_line_has_inline_reference, physical_lines,
    verbatim_protected_lines,
};

pub(crate) use words::resolve_word_anchors;

/// Result of the reference-line pre-pass.
pub struct AnchoringPrepass {
    /// Byte ranges (in original-source coordinates) of every removed reference
    /// line — each covers the line *plus its terminating newline*. The caller
    /// drops every token whose range falls inside one of these ranges from the
    /// token stream before parsing, which keeps all surviving tokens (and thus
    /// all AST node ranges) in original-source coordinates.
    pub removed_line_ranges: Vec<Range<usize>>,
    /// Resolved reference lines, in source order.
    pub reference_lines: Vec<ReferenceLine>,
    /// Overlap / stacking warnings (§2.3.3).
    pub diagnostics: Vec<Diagnostic>,
}

impl AnchoringPrepass {
    /// True when no reference line was found, so the caller can skip the
    /// token-filtering work entirely.
    pub fn is_empty(&self) -> bool {
        self.removed_line_ranges.is_empty()
    }

    /// Drop every token whose range starts inside a removed reference line.
    ///
    /// A token belongs to a removed line when its start offset lies within that
    /// line's `[start, end)` byte range (the range includes the terminating
    /// newline, so the line's `BlankLine`/newline token is dropped too — that is
    /// what makes the surrounding content lines directly adjacent rather than
    /// separated by a blank line). Tokens keep their original ranges, so the
    /// survivors stay in original-source coordinates.
    pub fn filter_tokens<T>(&self, tokens: Vec<(T, Range<usize>)>) -> Vec<(T, Range<usize>)> {
        if self.removed_line_ranges.is_empty() {
            return tokens;
        }
        tokens
            .into_iter()
            .filter(|(_, range)| {
                !self
                    .removed_line_ranges
                    .iter()
                    .any(|removed| removed.contains(&range.start))
            })
            .collect()
    }
}

/// Run the reference-line pre-pass over `source`.
pub fn extract_reference_lines(source: &str) -> AnchoringPrepass {
    let lines = physical_lines(source);
    let loc = SourceLocation::new(source);

    // Verbatim block bodies are raw: a `[token]` line inside one must stay
    // literal, never be ejected as a reference line (lex#755). Compute the
    // protected line set once and skip those lines below.
    let in_verbatim = verbatim_protected_lines(source, &lines);

    let mut reference_lines: Vec<ReferenceLine> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Which line indices are reference lines, so we skip them when building the
    // cleaned source.
    let mut removed: Vec<bool> = vec![false; lines.len()];

    // Track, per source line index, whether an element head line at that index
    // has already been claimed by a reference line — so a second (stacked)
    // reference line over the same element is flagged.
    let mut anchored_line: Vec<bool> = vec![false; lines.len()];

    for idx in 0..lines.len() {
        // Lines inside a verbatim block body are raw — never a reference line.
        if in_verbatim[idx] {
            continue;
        }
        let line = &lines[idx];
        let trimmed = line.trimmed();
        let Some(inner) = bracketed_inner(trimmed) else {
            continue;
        };
        let reference_type = determine_reference_type(inner);

        // Build the reference's range (brackets inclusive) in original coords.
        let bracket_start = line.start + (line.text.len() - line.text.trim_start().len());
        let bracket_end = bracket_start + trimmed.len();
        let reference_range = loc.byte_range_to_ast_range(&(bracket_start..bracket_end));

        let reference = {
            let mut r = ReferenceInline::new(inner.to_string());
            r.reference_type = reference_type.clone();
            r
        };

        // Marker-style references on their own line are not reference lines for
        // anchoring: they remain in the stream and self-link / resolve as usual.
        if reference_type.anchoring() != AnchorKind::WholeLineCapable {
            continue;
        }

        // This is a reference line: it is removed from the structural stream.
        removed[idx] = true;

        // Find the content line directly above (upward-only). Skipping is *not*
        // allowed: a blank line directly above means self-link. But preceding
        // reference lines have already been removed from the logical stream, so
        // we look past lines we have already marked `removed` to find the
        // nearest physical predecessor — and if that predecessor is itself a
        // reference line we just removed, the *element* it anchored is the head
        // line, which would now be double-anchored (stacked): flag it.
        let mut above: Option<usize> = None;
        let mut stacked_over: Option<usize> = None;
        if idx > 0 {
            let mut j = idx - 1;
            loop {
                if removed[j] {
                    // A reference line directly above us: stacking. Its own
                    // anchor target (if any) is what we'd collide on.
                    stacked_over = Some(j);
                    if j == 0 {
                        break;
                    }
                    j -= 1;
                    continue;
                }
                if lines[j].is_blank() {
                    above = None;
                } else {
                    above = Some(j);
                }
                break;
            }
        }

        let anchor = match above {
            Some(above_idx) => {
                let head = head_line_anchor(&lines[above_idx]);
                let anchor_range = loc.byte_range_to_ast_range(&(head.start..head.end));

                // Overlap diagnostics (§2.3.3): at most one reference line may
                // anchor a given element. Two situations are illegal:
                //  (a) the head line is already claimed by an earlier
                //      reference line (stacked), or
                //  (b) the head line itself carries an inline reference (a
                //      nested link over the same text).
                if anchored_line[above_idx] || stacked_over.is_some() {
                    diagnostics.push(
                        Diagnostic::new(
                            reference_range.clone(),
                            DiagnosticSeverity::Warning,
                            format!(
                                "Multiple reference lines anchor the same element \
                                 ('{}'); only the first is honored",
                                head.text
                            ),
                        )
                        .with_code("stacked-reference-line"),
                    );
                } else if head_line_has_inline_reference(&lines[above_idx]) {
                    diagnostics.push(
                        Diagnostic::new(
                            reference_range.clone(),
                            DiagnosticSeverity::Warning,
                            format!(
                                "Reference line anchors an element whose head line \
                                 ('{}') already carries an inline reference; the \
                                 whole-line anchor is honored",
                                head.text
                            ),
                        )
                        .with_code("overlapping-reference-line"),
                    );
                }

                anchored_line[above_idx] = true;
                ReferenceAnchor::WholeElement {
                    anchor_text: head.text,
                    anchor_range,
                    element: head.element,
                }
            }
            None => ReferenceAnchor::SelfLink,
        };

        reference_lines.push(ReferenceLine {
            reference,
            reference_range,
            anchor,
        });
    }

    // Report the byte range of every removed reference line — the line *plus*
    // its terminating newline (`line.end` already points just past the `\n`).
    // The caller drops the tokens inside these ranges from the token stream, so
    // a reference line that self-links is *also* removed from structure (it is
    // transparent either way); its standalone rendering is reconstructed by
    // consumers from the collected `reference_lines` entry. This keeps the
    // structural parser unaware of reference lines (§2.3.3) without editing the
    // source string, so all surviving tokens keep original-source coordinates.
    let removed_line_ranges: Vec<Range<usize>> = lines
        .iter()
        .enumerate()
        .filter(|(idx, _)| removed[*idx])
        .map(|(_, line)| line.start..line.end)
        .collect();

    AnchoringPrepass {
        removed_line_ranges,
        reference_lines,
        diagnostics,
    }
}
