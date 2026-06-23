//! Word-anchor resolution (§2.3.1) for inline references.
//!
//! Given a single line's inline node sequence, this resolves each top-level
//! [`InlineNode::Reference`](crate::lex::inlines::InlineNode)'s `word_anchor`:
//! the word the reference is "about", computed from the surrounding plain-text
//! word stream. This is a distinct role from the reference-line pre-pass — it
//! works on already-parsed inline nodes, not physical source lines.

use crate::lex::inlines::{AnchorDirection, WordAnchor};

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
