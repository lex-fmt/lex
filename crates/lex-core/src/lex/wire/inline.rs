//! Inline-content conversion between lex-core's `TextContent` /
//! `InlineNode` and `lex_extension::WireInline`.
//!
//! Forward path:
//!
//! - When `TextContent` has parsed inlines available
//!   ([`TextContent::inline_nodes`]), the codec walks the inline tree
//!   and produces matching `WireInline` variants
//!   (`Plain → Text`, `Strong → Bold`, `Emphasis → Italic`, `Code →
//!   Code`, `Math → Math`, `Reference → Reference`). Inline-attached
//!   annotations are dropped (Phase 2 fidelity is a future codec
//!   improvement).
//! - Otherwise (the raw-string Phase-1 representation) the codec
//!   emits a single [`WireInline::Text`] carrying the raw source. The
//!   parser re-interprets formatting markers when this round-trips
//!   back through `from_wire`.
//! - Empty text yields an empty `Vec` (no inline element is emitted).
//!
//! Reverse path always produces a `TextContent::from_string` whose
//! body is the concatenation of the wire inlines re-serialised to
//! `.lex` source form (`*x*` for bold, `_y_` for italic, `` `code` ``,
//! `#math#`, `[ref]`). That string parses identically when fed back
//! to the inline parser.

use crate::lex::ast::elements::inlines::{InlineContent, InlineNode, ReferenceInline};
use crate::lex::ast::TextContent;
use lex_extension::wire::{RefKind, WireInline};

/// Forward: `TextContent` → list of `WireInline`s.
///
/// Returns an empty vector for empty text. Walks parsed inline nodes
/// when they're available; otherwise emits a single `Text` inline
/// carrying the raw source string.
pub(crate) fn text_content_to_wire(tc: &TextContent) -> Vec<WireInline> {
    if let Some(nodes) = tc.inline_nodes() {
        return nodes.iter().map(inline_node_to_wire).collect();
    }
    let raw = tc.as_string();
    if raw.is_empty() {
        return Vec::new();
    }
    vec![WireInline::Text {
        text: raw.to_string(),
    }]
}

/// Forward: walk a parsed inline tree (`Vec<InlineNode>`) into wire
/// inlines.
pub(crate) fn inline_nodes_to_wire(nodes: &InlineContent) -> Vec<WireInline> {
    nodes.iter().map(inline_node_to_wire).collect()
}

fn inline_node_to_wire(node: &InlineNode) -> WireInline {
    match node {
        InlineNode::Plain { text, .. } => WireInline::Text { text: text.clone() },
        InlineNode::Strong { content, .. } => WireInline::Bold {
            children: inline_nodes_to_wire(content),
        },
        InlineNode::Emphasis { content, .. } => WireInline::Italic {
            children: inline_nodes_to_wire(content),
        },
        InlineNode::Code { text, .. } => WireInline::Code { text: text.clone() },
        InlineNode::Math { text, .. } => WireInline::Math { text: text.clone() },
        InlineNode::Reference { data, .. } => reference_to_wire(data),
        // No catch-all needed — InlineNode is exhaustive in lex-core.
    }
}

fn reference_to_wire(data: &ReferenceInline) -> WireInline {
    // Emit a generic reference; the discriminator best-effort maps to
    // the documented RefKind values. Detail-level fidelity beyond the
    // raw target string is a future improvement.
    WireInline::Reference {
        ref_kind: RefKind::General,
        target: data.raw.clone(),
        label: None,
    }
}

/// Reverse: wire inlines → a single `TextContent` carrying the
/// re-serialised source form.
pub(crate) fn text_content_from_wire(inlines: &[WireInline]) -> TextContent {
    let mut buf = String::new();
    for inline in inlines {
        write_inline_source(inline, &mut buf);
    }
    TextContent::from_string(buf, None)
}

fn write_inline_source(inline: &WireInline, buf: &mut String) {
    match inline {
        WireInline::Text { text } => buf.push_str(text),
        WireInline::Bold { children } => {
            buf.push('*');
            for c in children {
                write_inline_source(c, buf);
            }
            buf.push('*');
        }
        WireInline::Italic { children } => {
            buf.push('_');
            for c in children {
                write_inline_source(c, buf);
            }
            buf.push('_');
        }
        WireInline::Code { text } => {
            buf.push('`');
            buf.push_str(text);
            buf.push('`');
        }
        WireInline::Math { text } => {
            buf.push('#');
            buf.push_str(text);
            buf.push('#');
        }
        WireInline::Reference { target, label, .. } => {
            buf.push('[');
            // Emit `[label]` form when a label is present; otherwise
            // bare `[target]`. Re-parsing reconstructs the appropriate
            // ReferenceInline variant.
            buf.push_str(label.as_deref().unwrap_or(target));
            buf.push(']');
        }
        // WireInline is `#[non_exhaustive]` — guard against future
        // variants by emitting an empty span. Round-trip will be lossy
        // for any new inline kind, but this avoids panicking.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    //! Direct unit tests for the inline codec helpers.
    //!
    //! These complement the higher-level wire round-trip tests in
    //! `wire::tests` by exercising each `WireInline` arm explicitly —
    //! the round-trip suite currently focuses on block-level nodes and
    //! only touches inlines through plain `Text`, which leaves
    //! `Bold`/`Italic`/`Code`/`Math`/`Reference` and the empty-text
    //! short-circuit uncovered.
    use super::*;
    use crate::lex::ast::elements::inlines::ReferenceInline;
    use crate::lex::ast::TextContent;
    use crate::lex::inlines::parse_inlines;
    use lex_extension::wire::{RefKind, WireInline};

    /// Empty `TextContent` must yield an empty wire vector — not a
    /// single empty `Text` inline, which would be a wasteful (and
    /// semantically distinct) round-trip artefact.
    #[test]
    fn text_content_to_wire_empty_yields_empty_vec() {
        let tc = TextContent::empty();
        assert!(text_content_to_wire(&tc).is_empty());
    }

    /// Raw text without parsed inlines (the Phase-1 representation)
    /// emits a single `WireInline::Text` carrying the original string
    /// verbatim. Re-parsing on the far side restores any formatting
    /// markers.
    #[test]
    fn text_content_to_wire_raw_emits_single_text_inline() {
        let tc = TextContent::from_string("plain *bold* text".into(), None);
        let wire = text_content_to_wire(&tc);
        assert_eq!(wire.len(), 1);
        match &wire[0] {
            WireInline::Text { text } => assert_eq!(text, "plain *bold* text"),
            other => panic!("expected Text inline carrying the raw string, got {other:?}"),
        }
    }

    /// Once inlines are parsed, the codec walks the tree and emits
    /// matching wire variants rather than collapsing to raw text.
    #[test]
    fn text_content_to_wire_walks_parsed_inlines() {
        let mut tc = TextContent::from_string("hello *loud* world".into(), None);
        tc.ensure_inline_parsed();
        let wire = text_content_to_wire(&tc);
        // Expect: Text("hello "), Bold([Text("loud")]), Text(" world").
        assert_eq!(wire.len(), 3);
        assert!(matches!(&wire[0], WireInline::Text { text } if text == "hello "));
        match &wire[1] {
            WireInline::Bold { children } => {
                assert_eq!(children.len(), 1);
                assert!(matches!(&children[0], WireInline::Text { text } if text == "loud"));
            }
            other => panic!("expected Bold inline, got {other:?}"),
        }
        assert!(matches!(&wire[2], WireInline::Text { text } if text == " world"));
    }

    /// Each `InlineNode` variant has a matching `WireInline` arm —
    /// pin them down individually so a future refactor that drops or
    /// re-maps one arm fails loudly.
    #[test]
    fn each_inline_node_variant_maps_to_its_wire_arm() {
        let plain = InlineNode::plain("p".into());
        assert!(matches!(inline_nodes_to_wire(&vec![plain])[0],
            WireInline::Text { ref text } if text == "p"));

        let strong = InlineNode::strong(vec![InlineNode::plain("s".into())]);
        match &inline_nodes_to_wire(&vec![strong])[0] {
            WireInline::Bold { children } => {
                assert!(matches!(&children[0], WireInline::Text { text } if text == "s"));
            }
            other => panic!("Strong → Bold expected, got {other:?}"),
        }

        let emphasis = InlineNode::emphasis(vec![InlineNode::plain("e".into())]);
        match &inline_nodes_to_wire(&vec![emphasis])[0] {
            WireInline::Italic { children } => {
                assert!(matches!(&children[0], WireInline::Text { text } if text == "e"));
            }
            other => panic!("Emphasis → Italic expected, got {other:?}"),
        }

        let code = InlineNode::code("x".into());
        assert!(matches!(inline_nodes_to_wire(&vec![code])[0],
            WireInline::Code { ref text } if text == "x"));

        let math = InlineNode::math("y".into());
        assert!(matches!(inline_nodes_to_wire(&vec![math])[0],
            WireInline::Math { ref text } if text == "y"));

        let reference = InlineNode::reference(ReferenceInline::new("ref".into()));
        match &inline_nodes_to_wire(&vec![reference])[0] {
            WireInline::Reference {
                ref_kind,
                target,
                label,
            } => {
                // The forward codec emits `RefKind::General` and no label —
                // detail-level fidelity is documented as a future
                // improvement, so pin the current behaviour explicitly.
                assert_eq!(*ref_kind, RefKind::General);
                assert_eq!(target, "ref");
                assert!(label.is_none());
            }
            other => panic!("Reference → Reference expected, got {other:?}"),
        }
    }

    /// Nested formatting (`*outer _inner_*`) preserves structure: the
    /// inner emphasis appears under the outer bold's children, not
    /// flattened.
    #[test]
    fn nested_strong_with_inner_emphasis_preserves_structure() {
        let node = InlineNode::strong(vec![
            InlineNode::plain("o ".into()),
            InlineNode::emphasis(vec![InlineNode::plain("i".into())]),
        ]);
        let wire = inline_nodes_to_wire(&vec![node]);
        let WireInline::Bold { children } = &wire[0] else {
            panic!("expected Bold");
        };
        assert_eq!(children.len(), 2);
        assert!(matches!(&children[0], WireInline::Text { text } if text == "o "));
        let WireInline::Italic { children: inner } = &children[1] else {
            panic!("expected nested Italic");
        };
        assert!(matches!(&inner[0], WireInline::Text { text } if text == "i"));
    }

    /// Reverse path: each `WireInline` arm re-serialises to a `.lex`
    /// source-form string that the inline parser re-reads identically.
    /// This is the contract `from_wire_*` relies on.
    #[test]
    fn from_wire_emits_source_form_per_variant() {
        let cases: &[(&[WireInline], &str)] = &[
            (&[WireInline::Text { text: "raw".into() }], "raw"),
            (
                &[WireInline::Bold {
                    children: vec![WireInline::Text { text: "b".into() }],
                }],
                "*b*",
            ),
            (
                &[WireInline::Italic {
                    children: vec![WireInline::Text { text: "i".into() }],
                }],
                "_i_",
            ),
            (&[WireInline::Code { text: "c".into() }], "`c`"),
            (&[WireInline::Math { text: "m".into() }], "#m#"),
            (
                &[WireInline::Reference {
                    ref_kind: RefKind::General,
                    target: "target".into(),
                    label: None,
                }],
                "[target]",
            ),
            (
                &[WireInline::Reference {
                    ref_kind: RefKind::General,
                    target: "url".into(),
                    label: Some("see".into()),
                }],
                "[see]",
            ),
        ];
        for (inlines, expected) in cases {
            let tc = text_content_from_wire(inlines);
            assert_eq!(
                tc.as_string(),
                *expected,
                "reverse codec emitted unexpected source for {inlines:?}"
            );
        }
    }

    /// Reverse with multiple inlines concatenates their source forms
    /// in order — no separator, matching how the inline parser will
    /// re-tokenise them.
    #[test]
    fn from_wire_concatenates_multiple_inlines() {
        let inlines = vec![
            WireInline::Text { text: "a ".into() },
            WireInline::Bold {
                children: vec![WireInline::Text { text: "b".into() }],
            },
            WireInline::Text { text: " c".into() },
        ];
        let tc = text_content_from_wire(&inlines);
        assert_eq!(tc.as_string(), "a *b* c");
    }

    /// End-to-end identity for `.lex` source containing every
    /// supported inline kind: parse → forward → reverse should
    /// reproduce the original source character-for-character.
    /// This is the property the codec exists to guarantee for the
    /// `lex.include` splicing path.
    #[test]
    fn round_trip_preserves_source_for_each_inline_kind() {
        let source = "plain *bold* _italic_ `code` #math# [ref]";
        let mut tc = TextContent::from_string(source.into(), None);
        tc.ensure_inline_parsed();
        let wire = text_content_to_wire(&tc);
        let back = text_content_from_wire(&wire);
        assert_eq!(back.as_string(), source);
    }

    /// `parse_inlines` is the source of truth for what the codec sees
    /// — use it directly to confirm the forward path doesn't depend
    /// on any TextContent caching subtlety.
    #[test]
    fn inline_nodes_to_wire_handles_parser_output_directly() {
        let nodes = parse_inlines("a `code` b");
        let wire = inline_nodes_to_wire(&nodes);
        // Whatever the parser produced, the wire form must end with a
        // Text("b") inline (the trailing plain segment) — a sanity
        // check that the codec doesn't drop trailing nodes.
        assert!(
            matches!(wire.last(), Some(WireInline::Text { text }) if text == " b"),
            "expected trailing Text inline, got {:?}",
            wire.last()
        );
        // And a Code inline must appear somewhere in the middle.
        assert!(
            wire.iter()
                .any(|w| matches!(w, WireInline::Code { text } if text == "code")),
            "Code inline missing from wire output: {wire:?}",
        );
    }
}
