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
