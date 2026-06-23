//! Physical-line classification and head-line extraction for the
//! reference-line pre-pass.
//!
//! This module owns the line-level primitives the pre-pass
//! ([`extract_reference_lines`](super::extract_reference_lines)) reads over:
//! splitting the source into physical lines, recognising a bare bracketed
//! reference, deriving the anchor head-line text for a content line, and
//! computing which lines fall inside a verbatim block body (which must stay
//! raw, never ejected as reference lines — lex#755).

use crate::lex::inlines::{parse_inlines, InlineNode};
use crate::lex::lexing::line_classification::classify_line_tokens;
use crate::lex::token::{LineType, Token};

use super::AnchoredElement;

/// A single physical source line with its byte bounds.
pub(super) struct PhysicalLine<'a> {
    /// Byte offset of the first character of the line (after the previous `\n`).
    pub(super) start: usize,
    /// Byte offset just past the line's terminating `\n` (== the line's content
    /// end when the final line has no trailing newline).
    pub(super) end: usize,
    /// The line's text, excluding the trailing newline.
    pub(super) text: &'a str,
}

impl PhysicalLine<'_> {
    /// The line trimmed of surrounding whitespace.
    pub(super) fn trimmed(&self) -> &str {
        self.text.trim()
    }

    /// True when the line has no non-whitespace content.
    pub(super) fn is_blank(&self) -> bool {
        self.trimmed().is_empty()
    }
}

/// Split `source` into physical lines, preserving byte bounds.
pub(super) fn physical_lines(source: &str) -> Vec<PhysicalLine<'_>> {
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
pub(super) fn bracketed_inner(trimmed: &str) -> Option<&str> {
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
pub(super) fn head_line_anchor(line: &PhysicalLine<'_>) -> HeadLine {
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

pub(super) struct HeadLine {
    pub(super) text: String,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) element: AnchoredElement,
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

/// Per-physical-line classification used by the verbatim-region scan: the
/// line's [`LineType`] and its indentation depth (number of leading 4-space
/// indentation units).
struct ClassifiedLine {
    line_type: LineType,
    indent: usize,
}

/// Classify one physical line the same way the lexer's line grouper does:
/// tokenize it and run [`classify_line_tokens`], and count leading
/// [`Token::Indentation`] tokens for the indentation depth.
///
/// `line_text` is sliced directly from the original source and so still carries
/// its own trailing newline (every line except possibly the last) — no per-line
/// allocation. The classifier's blank-line / colon handling tolerates the
/// presence or absence of that newline either way.
fn classify_physical_line(line_text: &str) -> ClassifiedLine {
    let tokens: Vec<Token> = crate::lex::lexing::base_tokenization::tokenize(line_text)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    let indent = tokens
        .iter()
        .take_while(|t| matches!(t, Token::Indentation))
        .count();
    ClassifiedLine {
        line_type: classify_line_tokens(&tokens),
        indent,
    }
}

/// Compute which physical lines fall inside a verbatim block (subject through
/// closing `:: label ::` marker, inclusive). Reference-line extraction must
/// skip these lines: a verbatim block's body is raw, so a `[token]` line inside
/// it (e.g. a TOML table header `[server]`) must stay literal — never be
/// ejected and re-emitted as a whole-element anchor / auto-link (lex#755).
///
/// This mirrors the structural verbatim grammar
/// ([`match_verbatim_block`](crate::lex::parsing::parser::GrammarMatcher::match_verbatim_block)):
/// a subject line, then one or more groups of body content, terminated by a
/// `DataMarkerLine` (`:: label ::`) at the subject's indentation. Body content
/// may be deeper-indented (inflow) or at the subject's indentation (fullwidth /
/// groups); a group is another subject + body before the single shared closing
/// marker. Crucially the scan only marks a region verbatim once it has *seen*
/// the closing marker — a subject with no closing marker is an ordinary
/// session/definition and its lines stay eligible for reference extraction.
///
/// # The anchoring-slot exception
///
/// One position inside a verbatim region is deliberately *not* protected: the
/// reference line that anchors the block's subject. Per §2.3 (see this module's
/// header) a link-like reference on its own line directly below a subject — at
/// the subject's indentation, with the indented body following — anchors that
/// subject and is transparent to structure:
///
/// ```text
/// Example Source:
/// [./example.rs]       <- anchors "Example Source"; removed, NOT body
///     fn main() {}     <- the actual (deeper-indented) body
/// :: rust ::
/// ```
///
/// That slot — the first non-blank line directly after a subject, at the
/// subject's indentation — keeps its existing whole-element-anchor behavior.
/// Every *other* line in the region (notably the deeper-indented inflow body
/// where TOML headers like `[server]` live, lex#755) is protected and stays
/// literal.
pub(super) fn verbatim_protected_lines(source: &str, lines: &[PhysicalLine<'_>]) -> Vec<bool> {
    // Fast path: a verbatim block necessarily ends in a `:: label ::` data
    // marker, so a source with no `::` at all has no verbatim block and no line
    // to protect. This skips per-line tokenization on the overwhelmingly common
    // reference-bearing-but-verbatim-free document.
    if !source.contains("::") {
        return vec![false; lines.len()];
    }

    let classified: Vec<ClassifiedLine> = lines
        .iter()
        .map(|l| {
            // Each line's `[start, end)` is a byte range carved from `source` by
            // `physical_lines`, so it is always valid and on char boundaries; a
            // miss is an invariant violation, not a recoverable case.
            let line_text = source
                .get(l.start..l.end)
                .expect("physical-line byte range must be valid in its own source");
            classify_physical_line(line_text)
        })
        .collect();

    let mut protected = vec![false; lines.len()];
    let len = lines.len();
    let mut idx = 0;

    while idx < len {
        // Skip blank lines between blocks.
        if matches!(classified[idx].line_type, LineType::BlankLine) {
            idx += 1;
            continue;
        }

        // A verbatim block opens on a subject line.
        if !matches!(
            classified[idx].line_type,
            LineType::SubjectLine | LineType::SubjectOrListItemLine
        ) {
            idx += 1;
            continue;
        }

        let subject_idx = idx;
        let subject_indent = classified[subject_idx].indent;

        // Scan forward for a closing data marker at the subject's indentation.
        // Allow further subject lines (verbatim groups) and any body lines in
        // between. Stop — without claiming a verbatim block — if we hit a
        // content line shallower than the subject that is not the closing
        // marker, which means the structure closed before any closing marker
        // appeared (so it was an ordinary session/definition, not verbatim).
        let mut cursor = subject_idx + 1;
        let mut closing: Option<usize> = None;
        while cursor < len {
            let line = &classified[cursor];
            match line.line_type {
                LineType::BlankLine => {
                    cursor += 1;
                }
                LineType::DataMarkerLine if line.indent == subject_indent => {
                    closing = Some(cursor);
                    break;
                }
                _ => {
                    if line.indent < subject_indent {
                        break;
                    }
                    cursor += 1;
                }
            }
        }

        if let Some(closing_idx) = closing {
            // Protect every line of the region except the subject lines
            // themselves (they are normal anchorable head lines) and the
            // anchoring slot directly after a subject (see the doc comment).
            let mut after_subject = false;
            for i in subject_idx..=closing_idx {
                let is_subject = matches!(
                    classified[i].line_type,
                    LineType::SubjectLine | LineType::SubjectOrListItemLine
                );
                let is_blank = matches!(classified[i].line_type, LineType::BlankLine);

                if is_subject {
                    protected[i] = false;
                    after_subject = true;
                    continue;
                }
                if is_blank {
                    // Blank lines never carry a reference; leave them
                    // unprotected and keep looking for the anchoring slot, which
                    // may follow a blank line after the subject.
                    continue;
                }
                // First non-blank, non-subject line after a subject at the
                // subject's indentation is the anchoring slot — leave it to the
                // existing whole-element-anchor handling — *but only when real
                // body content follows it*. The documented anchoring shape is
                // `subject` / `[ref]` / indented-body / `:: marker ::`: the
                // reference anchors the subject and the body follows. If instead
                // the bracket line is itself the block's body (nothing but the
                // closing marker follows), it must stay literal (lex#755), so we
                // protect it rather than ejecting it as an anchor.
                if after_subject
                    && classified[i].indent == subject_indent
                    && has_inflow_body_after(&classified, i, closing_idx, subject_indent)
                {
                    protected[i] = false;
                    after_subject = false;
                    continue;
                }
                after_subject = false;
                protected[i] = true;
            }
            idx = closing_idx + 1;
        } else {
            // Not a verbatim block; resume scanning after the subject.
            idx = subject_idx + 1;
        }
    }

    protected
}

/// True when *inflow* body content — a non-blank line indented strictly deeper
/// than the subject — lies between the anchoring-slot candidate at `slot_idx`
/// and the block's closing marker at `closing_idx`.
///
/// This is what tells the documented anchoring shape from a fullwidth body
/// whose first line happens to be a bracket. The documented anchoring shape is
/// `subject` / `[ref]` / *deeper-indented* body / `:: marker ::`: only then is
/// the slot line a reference that anchors the subject. If the only content that
/// follows is at the subject's own indentation (a fullwidth body), the bracket
/// line is itself body and must stay literal (lex#755) — anchoring it would
/// eject e.g. `[server]` from `Config:` / `[server]` / `port = 8080` /
/// `:: toml ::` and reintroduce the bug for fullwidth blocks.
fn has_inflow_body_after(
    classified: &[ClassifiedLine],
    slot_idx: usize,
    closing_idx: usize,
    subject_indent: usize,
) -> bool {
    ((slot_idx + 1)..closing_idx).any(|i| {
        !matches!(
            classified[i].line_type,
            LineType::BlankLine | LineType::DataMarkerLine
        ) && classified[i].indent > subject_indent
    })
}

/// Does a head line carry a genuine inline reference? Used only for the overlap
/// warning (§2.3.3).
///
/// This parses the line's inline content and inspects the resulting
/// [`InlineNode::Reference`] nodes rather than matching brackets textually, so
/// it does not false-positive on stray `[` characters (e.g. a verbatim subject
/// or prose that merely contains a bracket but no real reference).
pub(super) fn head_line_has_inline_reference(line: &PhysicalLine<'_>) -> bool {
    parse_inlines(line.text.trim())
        .iter()
        .any(|node| matches!(node, InlineNode::Reference { .. }))
}
