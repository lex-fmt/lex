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

use crate::lex::ast::anchoring::{AnchoredElement, ReferenceAnchor, ReferenceLine};
use crate::lex::ast::diagnostics::{Diagnostic, DiagnosticSeverity};
use crate::lex::ast::range::SourceLocation;
use crate::lex::inlines::{
    determine_reference_type, parse_inlines, AnchorDirection, AnchorKind, InlineNode,
    ReferenceInline, WordAnchor,
};
use std::ops::Range;

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

/// A single physical source line with its byte bounds.
struct PhysicalLine<'a> {
    /// Byte offset of the first character of the line (after the previous `\n`).
    start: usize,
    /// Byte offset just past the line's terminating `\n` (== the line's content
    /// end when the final line has no trailing newline).
    end: usize,
    /// The line's text, excluding the trailing newline.
    text: &'a str,
}

impl PhysicalLine<'_> {
    /// The line trimmed of surrounding whitespace.
    fn trimmed(&self) -> &str {
        self.text.trim()
    }

    /// True when the line has no non-whitespace content.
    fn is_blank(&self) -> bool {
        self.trimmed().is_empty()
    }
}

/// Split `source` into physical lines, preserving byte bounds.
fn physical_lines(source: &str) -> Vec<PhysicalLine<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for line in source.split_inclusive('\n') {
        let end = start + line.len();
        lines.push(PhysicalLine {
            start,
            end,
            text: line.strip_suffix('\n').unwrap_or(line),
        });
        start = end;
    }
    lines
}

/// If `trimmed` is exactly a single bracketed reference (`[<inner>]` with no
/// other `[`/`]` and a non-empty inner), return the inner content. Otherwise
/// `None`. The inner is returned verbatim (not trimmed) so byte math lines up.
fn bracketed_inner(trimmed: &str) -> Option<&str> {
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() || inner.contains('[') || inner.contains(']') {
        return None;
    }
    Some(inner)
}

/// Derive the anchor head-line text for a content line, applying the
/// element-specific head-line rules (§2.3.2):
/// - list item: drop the leading list marker (`- `, `1. `, `a) `, …).
/// - definition / session subject: drop the trailing `:` marker.
///
/// Returns `(anchor_text, byte_start, byte_end)` where the byte range is into
/// the original source and covers exactly `anchor_text`.
fn head_line_anchor(line: &PhysicalLine<'_>) -> HeadLine {
    let text = line.text;
    // Byte offset of the first non-indentation, non-leading-whitespace char.
    let content_offset = text.len() - text.trim_start().len();
    // Trimmed body (both ends) and its start offset in the original source.
    let mut start = line.start + content_offset;
    let mut body = text.trim();
    let mut end = start + body.len();

    let mut element = AnchoredElement::WholeLine;

    // List marker: `-`, `*`, `+`, or an ordered marker like `1.` / `a)` /
    // `I.`. We only need to strip the marker + following whitespace.
    if let Some(marker_len) = list_marker_len(body) {
        let after = &body[marker_len..];
        let ws = after.len() - after.trim_start().len();
        start += marker_len + ws;
        body = after.trim_start();
        end = start + body.len();
        element = AnchoredElement::ListItem;
    }

    // Trailing `:` subject marker (definition term / session title written with
    // a colon, verbatim subject). Strip a single trailing colon (and the
    // whitespace before it, if any). This is a *subject* rule only — a list
    // item like `- Note:` keeps its literal text `Note:`, so we never strip the
    // colon once the list-marker branch has classified the line as a ListItem.
    if element != AnchoredElement::ListItem {
        if let Some(stripped) = body.strip_suffix(':') {
            body = stripped.trim_end();
            end = start + body.len();
            element = AnchoredElement::Subject;
        }
    }

    HeadLine {
        text: body.to_string(),
        start,
        end,
        element,
    }
}

struct HeadLine {
    text: String,
    start: usize,
    end: usize,
    element: AnchoredElement,
}

/// Length in bytes of a leading list marker on `body`, if present.
///
/// Recognises the unordered markers `-`, `*`, `+` and ordered markers of the
/// form `<seq><.|)>` where `<seq>` is digits or ASCII letters (covers `1.`,
/// `a)`, `IV.`) — matching the markers the structural parser accepts. The
/// returned length covers the marker punctuation only (not trailing
/// whitespace), and a marker must be followed by whitespace (so `-5` and
/// `note:` are not markers).
fn list_marker_len(body: &str) -> Option<usize> {
    let first = body.chars().next()?;

    // Unordered: a single bullet char followed by whitespace.
    if matches!(first, '-' | '*' | '+') {
        if body[first.len_utf8()..].starts_with(char::is_whitespace) {
            return Some(first.len_utf8());
        }
        return None;
    }

    // Ordered: a run of alphanumerics terminated by `.` or `)`.
    let mut seq_end = 0;
    for (i, c) in body.char_indices() {
        if c.is_ascii_alphanumeric() {
            seq_end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if seq_end == 0 {
        return None;
    }
    let term = body[seq_end..].chars().next()?;
    if matches!(term, '.' | ')') {
        let marker_len = seq_end + term.len_utf8();
        if marker_len == body.len() || body[marker_len..].starts_with(char::is_whitespace) {
            return Some(marker_len);
        }
    }
    None
}

/// Run the reference-line pre-pass over `source`.
pub fn extract_reference_lines(source: &str) -> AnchoringPrepass {
    let lines = physical_lines(source);
    let loc = SourceLocation::new(source);

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

/// Resolve word anchors (§2.3.1) for every top-level inline reference in a
/// single line's inline node sequence, mutating each `Reference` node's
/// `word_anchor` in place.
///
/// Rules:
/// - Default: the word immediately *preceding* the reference.
/// - If the reference is the first token on the line (only whitespace before
///   it) and text follows on the same line, the word immediately *following*.
/// - A reference directly abutting a preceding word counts as that word
///   (`Hello[./f] World` → "Hello") — the preceding-word logic already does
///   this because abutting text has no whitespace before the word boundary.
///
/// A reference that is the only token on its line gets no word anchor (it would
/// have been a reference line if link-like; a lone marker reference simply has
/// no word to anchor). Whitespace-only text on one side is treated as empty.
pub(crate) fn resolve_word_anchors(nodes: &mut [crate::lex::inlines::InlineNode]) {
    use crate::lex::inlines::InlineNode;

    // Fast path: nothing to anchor if the line carries no reference. This avoids
    // the flatten/allocate work on the overwhelmingly common reference-free line
    // (this runs for every `TextContent`).
    if !nodes
        .iter()
        .any(|n| matches!(n, InlineNode::Reference { .. }))
    {
        return;
    }

    // Flatten each top-level node to its plain text so word boundaries can be
    // computed across formatting spans.
    let texts: Vec<String> = nodes.iter().map(flatten_inline_text).collect();

    let n = nodes.len();
    for i in 0..n {
        if !matches!(nodes[i], InlineNode::Reference { .. }) {
            continue;
        }

        let before: String = texts[..i].concat();
        let after: String = texts[i + 1..].concat();

        let first_on_line = before.trim().is_empty();
        let anchor = if first_on_line {
            // Following word (only when text actually follows).
            after
                .split_whitespace()
                .next()
                .and_then(clean_anchor_word)
                .map(|word| WordAnchor {
                    word,
                    direction: AnchorDirection::Following,
                })
        } else {
            // Preceding word: the last whitespace-delimited token of `before`.
            before
                .split_whitespace()
                .next_back()
                .and_then(clean_anchor_word)
                .map(|word| WordAnchor {
                    word,
                    direction: AnchorDirection::Preceding,
                })
        };

        if let InlineNode::Reference { data, .. } = &mut nodes[i] {
            data.word_anchor = anchor;
        }
    }
}

/// Strip surrounding punctuation from a candidate anchor word, honoring
/// [`WordAnchor::word`]'s contract that the stored word carries no surrounding
/// punctuation (`website, [url]` anchors `"website"`, not `"website,"`).
///
/// Leading and trailing non-alphanumeric characters are removed; interior
/// punctuation (e.g. `lex.ing`, `can't`) is preserved. Returns `None` when
/// nothing alphanumeric remains, so a punctuation-only token produces no anchor.
fn clean_anchor_word(word: &str) -> Option<String> {
    let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric());
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
}

/// Flatten an inline node to its plain text content (recursing into formatting
/// spans). References contribute no text (their bracketed content is not part
/// of the surrounding word stream).
fn flatten_inline_text(node: &crate::lex::inlines::InlineNode) -> String {
    use crate::lex::inlines::InlineNode;
    match node {
        InlineNode::Plain { text, .. }
        | InlineNode::Code { text, .. }
        | InlineNode::Math { text, .. } => text.clone(),
        InlineNode::Strong { content, .. } | InlineNode::Emphasis { content, .. } => {
            content.iter().map(flatten_inline_text).collect()
        }
        InlineNode::Reference { .. } => String::new(),
    }
}

/// Does a head line carry a genuine inline reference? Used only for the overlap
/// warning (§2.3.3).
///
/// This parses the line's inline content and inspects the resulting
/// [`InlineNode::Reference`] nodes rather than matching brackets textually, so
/// it does not false-positive on stray `[` characters (e.g. a verbatim subject
/// or prose that merely contains a bracket but no real reference).
fn head_line_has_inline_reference(line: &PhysicalLine<'_>) -> bool {
    parse_inlines(line.text.trim())
        .iter()
        .any(|node| matches!(node, InlineNode::Reference { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::traits::AstNode;
    use crate::lex::inlines::{AnchorDirection, ReferenceType};
    use crate::lex::parsing::parse_document;

    /// Helper: resolved reference lines for a source.
    fn ref_lines(src: &str) -> Vec<ReferenceLine> {
        parse_document(src).unwrap().reference_lines
    }

    /// Helper: the single whole-element anchor text for a one-reference-line
    /// source. Panics if there isn't exactly one whole-element anchor.
    fn sole_whole_anchor(src: &str) -> (String, AnchoredElement) {
        let lines = ref_lines(src);
        assert_eq!(
            lines.len(),
            1,
            "expected exactly one reference line: {lines:?}"
        );
        match &lines[0].anchor {
            ReferenceAnchor::WholeElement {
                anchor_text,
                element,
                ..
            } => (anchor_text.clone(), *element),
            other => panic!("expected whole-element anchor, got {other:?}"),
        }
    }

    // -- Fixture §1: inline word anchors -----------------------------------

    fn word_anchor(src: &str) -> WordAnchor {
        let doc = parse_document(src).unwrap();
        let r = doc
            .iter_all_references()
            .find(|r| r.word_anchor.is_some())
            .expect("a reference with a word anchor");
        r.word_anchor.clone().unwrap()
    }

    #[test]
    fn inline_preceding_word_anchor() {
        // "the project website [https://lex.ing] is fast."
        let wa = word_anchor("the project website [https://lex.ing] is fast.\n\n");
        assert_eq!(wa.word, "website");
        assert_eq!(wa.direction, AnchorDirection::Preceding);
    }

    #[test]
    fn inline_following_word_anchor() {
        // "[https://lex.ing] is the home page." — first on line → following.
        let wa = word_anchor("[https://lex.ing] is the home page.\n\n");
        assert_eq!(wa.word, "is");
        assert_eq!(wa.direction, AnchorDirection::Following);
    }

    #[test]
    fn inline_abutting_word_anchor() {
        // "Hello[./file.txt] World" — abutting → preceding "Hello".
        let wa = word_anchor("Hello[./file.txt] World\n\n");
        assert_eq!(wa.word, "Hello");
        assert_eq!(wa.direction, AnchorDirection::Preceding);
    }

    #[test]
    fn inline_preceding_word_anchor_trims_trailing_punctuation() {
        // "website, [https://x]" — the preceding token is "website," but the
        // stored word must carry no surrounding punctuation (per the
        // `WordAnchor::word` contract): "website".
        let wa = word_anchor("the project website, [https://x] is fast.\n\n");
        assert_eq!(wa.word, "website");
        assert_eq!(wa.direction, AnchorDirection::Preceding);
    }

    #[test]
    fn inline_following_word_anchor_trims_punctuation() {
        // First-on-line reference, following token has trailing punctuation.
        let wa = word_anchor("[https://x] (home) page.\n\n");
        assert_eq!(wa.word, "home");
        assert_eq!(wa.direction, AnchorDirection::Following);
    }

    #[test]
    fn inline_word_anchor_preserves_interior_punctuation() {
        // Interior dots/apostrophes are part of the word, not surrounding it.
        let wa = word_anchor("visit lex.ing [https://lex.ing] now.\n\n");
        assert_eq!(wa.word, "lex.ing");
        assert_eq!(wa.direction, AnchorDirection::Preceding);
    }

    #[test]
    fn inline_punctuation_only_neighbor_yields_no_anchor() {
        // The token preceding the reference is punctuation-only; after trimming
        // nothing alphanumeric remains, so no word anchor is produced.
        let doc = parse_document("word -- [https://x] end.\n\n").unwrap();
        let r = doc
            .iter_all_references()
            .find(|r| matches!(r.reference_type, ReferenceType::Url { .. }))
            .expect("the url reference");
        assert!(
            r.word_anchor.is_none(),
            "punctuation-only neighbor must not produce an anchor: {:?}",
            r.word_anchor
        );
    }

    // -- Fixture §2: reference line on a session title ---------------------

    #[test]
    fn reference_line_anchors_session_title() {
        let src = "Getting Started\n[./readme.txt]\n\n    Welcome to the docs.\n\n";
        let (anchor, _kind) = sole_whole_anchor(src);
        assert_eq!(anchor, "Getting Started");
        // The reference line is removed; structure is a session with a body.
        let doc = parse_document(src).unwrap();
        assert_eq!(doc.root.children[0].node_type(), "Session");
    }

    // -- Fixture §3: reference line on a list item ------------------------

    #[test]
    fn reference_line_anchors_list_item() {
        let src = "- Food\n- Water\n[https://water.example]\n- Bread\n\n";
        let (anchor, kind) = sole_whole_anchor(src);
        assert_eq!(anchor, "Water");
        assert_eq!(kind, AnchoredElement::ListItem);
        // List structure is preserved (3 items, the reference line removed).
        let doc = parse_document(src).unwrap();
        assert_eq!(doc.root.children[0].node_type(), "List");
    }

    #[test]
    fn reference_line_on_list_item_keeps_trailing_colon() {
        // A list item ending in `:` is not a subject — the colon is part of the
        // item text. Anchoring must keep it literal (`Note:`), never strip it
        // the way a definition/verbatim subject would.
        let src = "- Note:\n[./n.txt]\n- Other\n\n";
        let (anchor, kind) = sole_whole_anchor(src);
        assert_eq!(anchor, "Note:");
        assert_eq!(kind, AnchoredElement::ListItem);
    }

    // -- Fixture §4: reference line on a definition term (transparency) ----

    #[test]
    fn reference_line_keeps_definition_a_definition() {
        // The critical transparency case: with the reference line *removed*
        // (not blanked), `API Endpoint:` stays adjacent to its indented body,
        // so it remains a definition — not a session.
        let src =
            "API Endpoint:\n[./endpoint.txt]\n    A URL that provides access to a resource.\n\n";
        let (anchor, kind) = sole_whole_anchor(src);
        assert_eq!(anchor, "API Endpoint");
        assert_eq!(kind, AnchoredElement::Subject);

        let doc = parse_document(src).unwrap();
        assert_eq!(
            doc.root.children[0].node_type(),
            "Definition",
            "reference line must be transparent: blanking it would wrongly \
             turn the definition into a session"
        );
    }

    #[test]
    fn reference_line_as_blank_would_make_a_session() {
        // Control: the *same* source but with a genuine blank line in place of
        // the reference line parses as a session. This pins down exactly what
        // the transparency rule prevents.
        let src = "API Endpoint:\n\n    A URL that provides access to a resource.\n\n";
        let doc = parse_document(src).unwrap();
        assert_eq!(doc.root.children[0].node_type(), "Session");
    }

    // -- Fixture §5: reference line on a verbatim subject -----------------

    #[test]
    fn reference_line_anchors_verbatim_subject() {
        let src = "Example Source:\n[./example.rs]\n    fn main() {}\n:: rust ::\n\n";
        let (anchor, kind) = sole_whole_anchor(src);
        assert_eq!(anchor, "Example Source");
        assert_eq!(kind, AnchoredElement::Subject);

        let doc = parse_document(src).unwrap();
        assert_eq!(doc.root.children[0].node_type(), "VerbatimBlock");
    }

    // -- Fixture §6: reference line on a paragraph -----------------------

    #[test]
    fn reference_line_anchors_paragraph_line() {
        // A multi-line paragraph above so the line above the reference is a
        // genuine paragraph line (not promoted to a document title).
        let src =
            "First paragraph line.\nThe release notes cover every change.\n[./CHANGELOG.md]\n\n";
        let (anchor, kind) = sole_whole_anchor(src);
        assert_eq!(anchor, "The release notes cover every change.");
        assert_eq!(kind, AnchoredElement::WholeLine);
    }

    // -- Fixture §7: self-link fallback ----------------------------------

    #[test]
    fn reference_line_self_links_when_blank_above() {
        let src = "See the upstream project:\n\n[https://github.com/lex-fmt/lex]\n\n";
        let lines = ref_lines(src);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].anchor, ReferenceAnchor::SelfLink);
    }

    #[test]
    fn reference_line_self_links_at_start_of_container() {
        // First line of the document → no content above → self-link.
        let src = "[https://lex.ing]\n\n";
        let lines = ref_lines(src);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].anchor, ReferenceAnchor::SelfLink);
    }

    // -- Fixture §8: marker-style references on a reference line ----------

    #[test]
    fn marker_reference_on_its_own_line_is_not_a_reference_line() {
        // `[::summary-note]` is marker-style: it does NOT take a whole-element
        // anchor; it stays in the stream and resolves as usual.
        let src = "Closing remarks.\n[::summary-note]\n\n:: summary-note ::\n    Resolved.\n\n";
        let lines = ref_lines(src);
        assert!(
            lines.is_empty(),
            "marker-style reference must not become a whole-element anchor: {lines:?}"
        );
        // It remains an inline reference in the document.
        let doc = parse_document(src).unwrap();
        assert!(doc
            .iter_all_references()
            .any(|r| matches!(r.reference_type, ReferenceType::AnnotationReference { .. })));
    }

    #[test]
    fn footnote_on_its_own_line_is_not_a_reference_line() {
        let src = "Some claim.\n[42]\n\n:: 42 :: A footnote.\n\n";
        assert!(ref_lines(src).is_empty());
    }

    // -- §2.3.3: overlap / stacking diagnostics --------------------------

    #[test]
    fn stacked_reference_lines_warn_and_keep_first() {
        // Two reference lines over the same paragraph line.
        let src = "First line.\nClaim line here.\n[./a.txt]\n[./b.txt]\n\n";
        let doc = parse_document(src).unwrap();
        let lines = &doc.reference_lines;
        assert_eq!(lines.len(), 2, "both reference lines are collected");
        // Exactly one stacked-reference-line warning is emitted.
        let warns: Vec<_> = doc
            .diagnostics()
            .into_iter()
            .filter(|d| d.code.as_deref() == Some("stacked-reference-line"))
            .collect();
        assert_eq!(warns.len(), 1, "one stacking warning: {warns:?}");
    }

    #[test]
    fn reference_line_over_head_line_with_inline_reference_warns() {
        // The head line already carries an inline reference, so the
        // whole-element anchor would nest two links over the same text.
        let src = "See more here.\nVisit [https://a.example] now.\n[./b.txt]\n\n";
        let doc = parse_document(src).unwrap();
        let warns: Vec<_> = doc
            .diagnostics()
            .into_iter()
            .filter(|d| d.code.as_deref() == Some("overlapping-reference-line"))
            .collect();
        assert_eq!(warns.len(), 1, "one overlap warning: {warns:?}");
        // The whole-line anchor is still honored (§2.3.3).
        assert!(doc.reference_lines[0].anchor.is_whole_element());
    }

    #[test]
    fn head_line_with_stray_bracket_does_not_warn() {
        // The head line contains a `[` but no genuine inline reference (it is a
        // code span, not a reference). The string/bracket heuristic used to
        // false-positive here; the AST-based check must not fire the overlap
        // warning.
        let src = "Intro.\nThe array index `a[0]` matters.\n[./b.txt]\n\n";
        let doc = parse_document(src).unwrap();
        let warns: Vec<_> = doc
            .diagnostics()
            .into_iter()
            .filter(|d| d.code.as_deref() == Some("overlapping-reference-line"))
            .collect();
        assert!(
            warns.is_empty(),
            "a stray bracket is not an inline reference: {warns:?}"
        );
        // The whole-line anchor is still resolved.
        assert!(doc.reference_lines[0].anchor.is_whole_element());
    }

    // -- Type-level anchoring split (§2.3.4) -----------------------------

    #[test]
    fn anchor_kind_split_matches_spec() {
        use crate::lex::inlines::AnchorKind;
        assert_eq!(
            ReferenceType::Url { target: "x".into() }.anchoring(),
            AnchorKind::WholeLineCapable
        );
        assert_eq!(
            ReferenceType::File { target: "x".into() }.anchoring(),
            AnchorKind::WholeLineCapable
        );
        assert_eq!(
            ReferenceType::Session { target: "1".into() }.anchoring(),
            AnchorKind::WholeLineCapable
        );
        assert_eq!(
            ReferenceType::General { target: "x".into() }.anchoring(),
            AnchorKind::WholeLineCapable
        );
        assert_eq!(
            ReferenceType::FootnoteNumber { number: 1 }.anchoring(),
            AnchorKind::MarkerOnly
        );
        assert_eq!(
            ReferenceType::AnnotationReference { label: "n".into() }.anchoring(),
            AnchorKind::MarkerOnly
        );
        assert_eq!(ReferenceType::NotSure.anchoring(), AnchorKind::MarkerOnly);
    }

    // -- Range fidelity ---------------------------------------------------

    #[test]
    fn anchor_range_covers_the_head_line_text() {
        let src = "Getting Started\n[./readme.txt]\n\n    Body.\n\n";
        let doc = parse_document(src).unwrap();
        let ReferenceAnchor::WholeElement { anchor_range, .. } = &doc.reference_lines[0].anchor
        else {
            panic!("expected whole-element anchor");
        };
        assert_eq!(&src[anchor_range.span.clone()], "Getting Started");
    }

    #[test]
    fn reference_range_covers_brackets_inclusive() {
        let src = "Getting Started\n[./readme.txt]\n\n    Body.\n\n";
        let doc = parse_document(src).unwrap();
        let range = &doc.reference_lines[0].reference_range;
        assert_eq!(&src[range.span.clone()], "[./readme.txt]");
    }

    // -- Original-coordinate invariant (regression for the cleaned-source
    //    coordinate bug) --------------------------------------------------

    /// Removing a reference line by *editing the source string* used to shift
    /// every byte offset after it, so parsed AST nodes that followed a reference
    /// line carried "cleaned-source" coordinates instead of original-source
    /// ones. The token-filtering pre-pass keeps tokens at their original ranges,
    /// so every node after a reference line must still report its position in
    /// the ORIGINAL source.
    ///
    /// This asserts a later element's parsed range start equals the byte offset
    /// of its text in the original source. It fails against the old
    /// cleaned-source approach (the offset is short by the removed line's
    /// length) and passes with token filtering.
    #[test]
    fn later_element_keeps_original_source_coordinates() {
        // A reference line near the top, then a clearly later paragraph. The
        // removed `[./top.txt]\n` line is 12 bytes; under the old cleaned-source
        // approach every node after it was shifted left by 12.
        let original =
            "Intro paragraph here.\n[./top.txt]\n\nLater Section paragraph text.\n\n".to_string();

        let doc = parse_document(&original).unwrap();

        // Find the parsed paragraph whose text starts with "Later Section".
        let later = doc
            .root
            .children
            .iter()
            .find(|c| {
                c.text()
                    .map(|t| t.contains("Later Section"))
                    .unwrap_or(false)
            })
            .expect("a 'Later Section' element after the reference line");

        let expected_start = original
            .find("Later Section")
            .expect("the literal text in the original source");

        assert_eq!(
            later.range().span.start,
            expected_start,
            "node after a reference line must carry an ORIGINAL-source offset \
             (got {}, expected {}); a mismatch means a cleaned-source coordinate \
             leaked into the AST",
            later.range().span.start,
            expected_start,
        );

        // And the slice at that range is the actual original text.
        assert!(original[later.range().span.clone()].starts_with("Later Section"));
    }

    // -- Cleaned-source / no-reference-line passthrough ------------------

    #[test]
    fn documents_without_reference_lines_have_empty_collection() {
        let doc = parse_document("Just a paragraph.\n\n").unwrap();
        assert!(doc.reference_lines.is_empty());
        assert!(doc.reference_line_diagnostics.is_empty());
    }

    #[test]
    fn list_marker_stripping_handles_ordered_markers() {
        let src = "1. First item\n[./x.txt]\n2. Second item\n\n";
        let (anchor, kind) = sole_whole_anchor(src);
        assert_eq!(anchor, "First item");
        assert_eq!(kind, AnchoredElement::ListItem);
    }
}
