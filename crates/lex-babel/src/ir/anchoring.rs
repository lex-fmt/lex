//! Reference-anchoring transforms for IR construction.
//!
//! Lex derives a link's *anchor* (the text the link wraps) implicitly from the
//! reference's position, instead of writing it out like Markdown's `[text](url)`
//! (references-general.lex §2.3). lex-core resolves the anchors authoritatively
//! during parsing; this module turns that resolved data into IR `Link` nodes so
//! every serializer renders anchored links uniformly.
//!
//! Two anchor scopes (§2.3):
//!
//! - **Word anchor** (inline reference): a reference that shares its line with
//!   other text anchors a single adjacent word. lex-core records the resolved
//!   word on [`ReferenceInline::word_anchor`]. [`apply_word_anchors`] consumes
//!   it: it wraps that word in a `Link` and drops the bracketed reference, so
//!   `website [https://lex.ing]` renders `<a href="https://lex.ing">website</a>`
//!   rather than `website [https://lex.ing]`.
//!
//! - **Whole-element anchor** (reference line): a reference that is the only
//!   content on its line anchors the entire head line of the element above it.
//!   lex-core extracts these into `Document::reference_lines()` with the head
//!   line's source range. [`AnchorIndex`] indexes them by source span so the
//!   IR builder can wrap the matching element's head-line content in a `Link`.
//!   Reference lines with no element above (`SelfLink`) render as a standalone
//!   link of their own text.
//!
//! Marker-style references (footnotes `[1]`, citations `[@k]`, annotation refs
//! `[::label]`) are `AnchorKind::MarkerOnly`: lex-core never gives them a word
//! or whole-element anchor, so they fall through both transforms untouched and
//! keep their existing marker rendering.

use lex_core::lex::ast::anchoring::ReferenceAnchor;
use lex_core::lex::ast::elements::inlines::{AnchorDirection, AnchorKind, InlineNode};
use lex_core::lex::ast::elements::Document as LexDocument;
use lex_core::lex::ast::TextContent;

use super::nodes::InlineContent;

/// The href a linkable reference points at.
///
/// Whole-element and word anchors only ever apply to link-like reference types
/// (`Url`, `File`, `Session`, `General`); the raw bracket content is the
/// destination verbatim. (Citation `#ref-` rewriting lives in the serializers
/// and only applies to marker-style citations, which never reach here.)
pub(crate) fn reference_href(raw: &str) -> String {
    raw.to_string()
}

/// Replace each inline reference that carries a resolved word anchor with a
/// `Link` wrapping the anchored word, dropping the bracketed reference text.
///
/// `nodes` is the inline-node sequence for a single line (as produced by
/// [`TextContent::inline_items`]); the resolved [`WordAnchor`] is read straight
/// off each `Reference` node. References without a word anchor — marker-style
/// references, or a link-like reference with no adjacent word — are converted
/// normally by `convert`.
///
/// `convert` maps a single non-reference [`InlineNode`] to IR `InlineContent`
/// (the caller passes `convert_inline_node`); it is only invoked for
/// non-reference nodes and for reference nodes without a word anchor.
pub(crate) fn apply_word_anchors(
    nodes: &[InlineNode],
    convert: &dyn Fn(&InlineNode) -> InlineContent,
) -> Vec<InlineContent> {
    let mut out: Vec<InlineContent> = Vec::with_capacity(nodes.len());
    // Index of a following-word node already consumed into a link, so the main
    // loop skips re-emitting it.
    let mut skip: Option<usize> = None;

    for (i, node) in nodes.iter().enumerate() {
        if skip == Some(i) {
            continue;
        }
        let InlineNode::Reference { data, .. } = node else {
            out.push(convert(node));
            continue;
        };
        // lex-core records a word anchor on *every* reference, but only link-like
        // references render as a span of linked text (references-general.lex
        // §2.3.4). A marker-style reference (footnote / citation / annotation
        // ref) keeps its marker rendering, so leave it as a plain reference even
        // though it carries a resolved word.
        let anchorable = data.reference_type.anchoring() == AnchorKind::WholeLineCapable;
        let Some(word_anchor) = data.word_anchor.as_ref().filter(|_| anchorable) else {
            out.push(convert(node));
            continue;
        };

        let href = reference_href(&data.raw);
        match word_anchor.direction {
            // Preceding: the anchored word is the last word of the text before
            // the reference. Re-split the nearest preceding text node and wrap
            // its final word, leaving the prefix in place. The bracketed
            // reference itself emits nothing.
            AnchorDirection::Preceding => {
                if !wrap_preceding_word(&mut out, &word_anchor.word, &href) {
                    // Could not find the word in already-emitted output (e.g. it
                    // sat inside a formatting span); emit a standalone link so
                    // the destination is never lost.
                    out.push(InlineContent::Link {
                        text: word_anchor.word.clone(),
                        href,
                    });
                }
            }
            // Following: the anchored word is the first word of the text after
            // the reference. Emit the link now, then replace the following text
            // node with its remainder (first word removed) and skip it in the
            // main loop so it isn't emitted twice.
            AnchorDirection::Following => {
                out.push(InlineContent::Link {
                    text: word_anchor.word.clone(),
                    href,
                });
                skip = consume_following_word(nodes, i, &word_anchor.word, &mut out);
            }
        }
    }

    out
}

/// Wrap the final word of the most recently emitted text node in a `Link`.
///
/// Walks back over already-emitted `InlineContent`, finds the last `Text` node
/// whose trailing word matches `word` (ignoring surrounding punctuation, which
/// lex-core strips from the stored anchor word), and splits it so the prefix
/// stays as text and the word becomes a `Link`. Returns `false` when no such
/// text node exists.
fn wrap_preceding_word(out: &mut Vec<InlineContent>, word: &str, href: &str) -> bool {
    for idx in (0..out.len()).rev() {
        let InlineContent::Text(text) = &out[idx] else {
            // Only split plain text. A formatting span can't be split here.
            if matches!(out[idx], InlineContent::Link { .. }) {
                // A previous reference already consumed text here; keep looking.
                continue;
            }
            return false;
        };
        let trimmed_end = text.trim_end();
        if trimmed_end.is_empty() {
            continue;
        }
        let last_space = trimmed_end.rfind(char::is_whitespace);
        let (prefix, last_word) = match last_space {
            Some(pos) => (&trimmed_end[..=pos], &trimmed_end[pos + 1..]),
            None => ("", trimmed_end),
        };
        if strip_word_punct(last_word) != word {
            return false;
        }

        let prefix = prefix.to_string();
        let last_word = last_word.to_string();

        // Replace the text node with: [prefix?] Link(word). The whitespace that
        // separated the word from the (now-removed) bracketed reference is
        // dropped: a following text node already carries the leading space to
        // the next word, and at end of line a trailing space is not wanted —
        // this keeps `website [url] today` rendering with a single space.
        let mut replacement: Vec<InlineContent> = Vec::with_capacity(2);
        if !prefix.is_empty() {
            replacement.push(InlineContent::Text(prefix));
        }
        replacement.push(InlineContent::Link {
            text: last_word,
            href: href.to_string(),
        });
        out.splice(idx..=idx, replacement);
        return true;
    }
    false
}

/// Remove the first word of the text that immediately follows a first-on-line
/// reference, pushing the remainder (the text after the anchored word) so the
/// line reads `<link> rest…`. Returns the index of the node that was consumed,
/// so the caller can skip re-emitting it; `None` if no matching following text
/// node was found (the caller then leaves later nodes to be emitted normally).
fn consume_following_word(
    nodes: &[InlineNode],
    ref_index: usize,
    word: &str,
    out: &mut Vec<InlineContent>,
) -> Option<usize> {
    let j = ref_index + 1;
    let InlineNode::Plain { text, .. } = nodes.get(j)? else {
        return None;
    };
    let rest = text.trim_start();
    let first_space = rest.find(char::is_whitespace);
    let (first_word, suffix) = match first_space {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, ""),
    };
    if strip_word_punct(first_word) != word {
        return None;
    }
    // Drop the anchored first word (and the whitespace that separated it from
    // the reference); keep the suffix (which already starts with the space
    // before the next word).
    if !suffix.is_empty() {
        out.push(InlineContent::Text(suffix.to_string()));
    }
    Some(j)
}

/// Strip leading/trailing non-alphanumeric characters from a word, matching the
/// punctuation policy lex-core applies when storing the anchor word.
fn strip_word_punct(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric())
}

/// A resolved whole-element anchor: the head-line source span it covers and the
/// link to wrap it in.
#[derive(Debug, Clone)]
pub(crate) struct WholeElementAnchor {
    /// Source byte span of the anchored head-line text (marker / trailing colon
    /// excluded), in original-source coordinates.
    pub span: std::ops::Range<usize>,
    /// The link text — the head line's own text, as lex-core resolved it.
    pub anchor_text: String,
    /// The destination href (the reference's raw bracket content).
    pub href: String,
}

/// A self-linking reference line: it had no element above it, so it stands alone
/// and links its own text (§2.3.2).
#[derive(Debug, Clone)]
pub(crate) struct SelfLink {
    /// Source byte span of the `[bracketed]` reference, used to position the
    /// standalone link among the document's top-level children in source order.
    pub span: std::ops::Range<usize>,
    /// The link text + href (both the reference's raw bracket content).
    pub raw: String,
}

/// Index of a document's reference-line anchors, built once from
/// `Document::reference_lines()` and threaded through IR construction.
#[derive(Debug, Default)]
pub(crate) struct AnchorIndex {
    whole_element: Vec<WholeElementAnchor>,
    self_links: Vec<SelfLink>,
}

impl AnchorIndex {
    /// Build the index from a parsed lex document's resolved reference lines.
    pub(crate) fn from_document(doc: &LexDocument) -> Self {
        let mut whole_element = Vec::new();
        let mut self_links = Vec::new();
        for line in doc.reference_lines() {
            match &line.anchor {
                ReferenceAnchor::WholeElement {
                    anchor_text,
                    anchor_range,
                    ..
                } => {
                    // Whole-element anchors only apply to link-like reference
                    // types (lex-core guarantees this), so the raw content is
                    // the destination verbatim.
                    whole_element.push(WholeElementAnchor {
                        span: anchor_range.span.clone(),
                        anchor_text: anchor_text.clone(),
                        href: reference_href(&line.reference.raw),
                    });
                }
                ReferenceAnchor::SelfLink => {
                    self_links.push(SelfLink {
                        span: line.reference_range.span.clone(),
                        raw: line.reference.raw.clone(),
                    });
                }
            }
        }
        Self {
            whole_element,
            self_links,
        }
    }

    /// True when the document carried no reference lines at all (the common
    /// case), so callers can skip all anchor work.
    pub(crate) fn is_empty(&self) -> bool {
        self.whole_element.is_empty() && self.self_links.is_empty()
    }

    /// Find the whole-element anchor whose head-line span falls inside the given
    /// head-line `TextContent`'s source span, if any.
    ///
    /// lex-core resolves `anchor_range` to the marker/colon-stripped head-line
    /// text, which is always a sub-span of the element's head-line `TextContent`
    /// span (equal for sessions / definitions / paragraph lines; strictly
    /// contained for list items, whose `TextContent` includes the trailing
    /// newline). Containment therefore matches each anchor to exactly one head
    /// line.
    pub(crate) fn match_head_line(&self, head: &TextContent) -> Option<&WholeElementAnchor> {
        let loc = head.location.as_ref()?;
        let head_span = &loc.span;
        self.whole_element
            .iter()
            .find(|a| head_span.start <= a.span.start && a.span.end <= head_span.end)
    }

    /// Self-links whose reference falls within `[start, end)` of the source, in
    /// source order. Used to splice standalone self-link paragraphs into a
    /// container at the right position.
    pub(crate) fn self_links_in(&self, range: std::ops::Range<usize>) -> Vec<&SelfLink> {
        let mut v: Vec<&SelfLink> = self
            .self_links
            .iter()
            .filter(|s| s.span.start >= range.start && s.span.start < range.end)
            .collect();
        v.sort_by_key(|s| s.span.start);
        v
    }
}

/// Wrap a head line's inline content in a single `Link` for a whole-element
/// anchor. The original content is discarded in favour of the anchor's resolved
/// text (which already has the list marker / trailing colon stripped) — exactly
/// what should render as the link's visible text.
pub(crate) fn wrap_head_line(anchor: &WholeElementAnchor) -> Vec<InlineContent> {
    vec![InlineContent::Link {
        text: anchor.anchor_text.clone(),
        href: anchor.href.clone(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::ast::elements::inlines::{ReferenceInline, ReferenceType, WordAnchor};

    /// Minimal `convert` mirroring `from_lex::convert_inline_node` for the inline
    /// shapes the word-anchor tests exercise (plain text and bare references).
    fn convert(node: &InlineNode) -> InlineContent {
        match node {
            InlineNode::Plain { text, .. } => InlineContent::Text(text.clone()),
            InlineNode::Reference { data, .. } => InlineContent::Reference {
                raw: data.raw.clone(),
                kind: data.reference_type.clone(),
            },
            other => panic!("unexpected node in test: {other:?}"),
        }
    }

    fn plain(text: &str) -> InlineNode {
        InlineNode::plain(text.to_string())
    }

    /// A reference node with a resolved word anchor and the given type.
    fn reference(raw: &str, kind: ReferenceType, anchor: Option<WordAnchor>) -> InlineNode {
        let mut data = ReferenceInline::new(raw.to_string());
        data.reference_type = kind;
        data.word_anchor = anchor;
        InlineNode::reference(data)
    }

    #[test]
    fn preceding_word_becomes_link_and_bracket_drops() {
        // "the project website [https://lex.ing] today"
        let nodes = vec![
            plain("the project website "),
            reference(
                "https://lex.ing",
                ReferenceType::Url {
                    target: "https://lex.ing".into(),
                },
                Some(WordAnchor {
                    word: "website".into(),
                    direction: AnchorDirection::Preceding,
                }),
            ),
            plain(" today"),
        ];
        let out = apply_word_anchors(&nodes, &convert);
        assert_eq!(
            out,
            vec![
                InlineContent::Text("the project ".into()),
                InlineContent::Link {
                    text: "website".into(),
                    href: "https://lex.ing".into(),
                },
                InlineContent::Text(" today".into()),
            ]
        );
    }

    #[test]
    fn following_word_becomes_link_and_is_removed_from_text() {
        // "[https://lex.ing] is the home page."
        let nodes = vec![
            reference(
                "https://lex.ing",
                ReferenceType::Url {
                    target: "https://lex.ing".into(),
                },
                Some(WordAnchor {
                    word: "is".into(),
                    direction: AnchorDirection::Following,
                }),
            ),
            plain(" is the home page."),
        ];
        let out = apply_word_anchors(&nodes, &convert);
        assert_eq!(
            out,
            vec![
                InlineContent::Link {
                    text: "is".into(),
                    href: "https://lex.ing".into(),
                },
                InlineContent::Text(" the home page.".into()),
            ]
        );
    }

    #[test]
    fn abutting_preceding_word() {
        // "Hello[./file.txt] World"
        let nodes = vec![
            plain("Hello"),
            reference(
                "./file.txt",
                ReferenceType::File {
                    target: "./file.txt".into(),
                },
                Some(WordAnchor {
                    word: "Hello".into(),
                    direction: AnchorDirection::Preceding,
                }),
            ),
            plain(" World"),
        ];
        let out = apply_word_anchors(&nodes, &convert);
        assert_eq!(
            out,
            vec![
                InlineContent::Link {
                    text: "Hello".into(),
                    href: "./file.txt".into(),
                },
                InlineContent::Text(" World".into()),
            ]
        );
    }

    #[test]
    fn marker_reference_keeps_its_bracket_even_with_word_anchor() {
        // lex-core stores a word anchor on every reference, but a marker-style
        // reference (footnote) must NOT be word-anchored (§2.3.4): it stays a
        // bare Reference for the serializer to render as a marker.
        let nodes = vec![
            plain("See "),
            reference(
                "42",
                ReferenceType::FootnoteNumber { number: 42 },
                Some(WordAnchor {
                    word: "See".into(),
                    direction: AnchorDirection::Preceding,
                }),
            ),
            plain(" later."),
        ];
        let out = apply_word_anchors(&nodes, &convert);
        assert_eq!(
            out,
            vec![
                InlineContent::Text("See ".into()),
                InlineContent::Reference {
                    raw: "42".into(),
                    kind: ReferenceType::FootnoteNumber { number: 42 },
                },
                InlineContent::Text(" later.".into()),
            ]
        );
    }

    #[test]
    fn link_like_reference_without_word_anchor_stays_reference() {
        // A lone link-like reference (no adjacent word resolved) is left as a
        // Reference; the serializer renders it as a self-link of its own text.
        let nodes = vec![reference(
            "https://example.com",
            ReferenceType::Url {
                target: "https://example.com".into(),
            },
            None,
        )];
        let out = apply_word_anchors(&nodes, &convert);
        assert_eq!(
            out,
            vec![InlineContent::Reference {
                raw: "https://example.com".into(),
                kind: ReferenceType::Url {
                    target: "https://example.com".into(),
                },
            }]
        );
    }
}
