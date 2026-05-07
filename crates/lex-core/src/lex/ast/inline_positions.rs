//! Inline-position visitor for [`TextContent`].
//!
//! Walks the inline tree of a parsed text run while tracking byte offsets
//! through the raw source, and fires visitor callbacks at each leaf and
//! container boundary. Each callback receives a precise [`Range`] (with both
//! byte span and `line:column` positions) so consumers can emit LSP semantic
//! tokens, document links, text objects, references for goto-def, etc.
//! without re-implementing cursor and escape arithmetic.
//!
//! ## Why this exists
//!
//! Two consumers in the workspace need the same cursor logic:
//!
//! - [`super::Document::find_all_links`] /
//!   [`super::Session::find_all_links`] — emit a `DocumentLink` per
//!   URL/File reference, with a range covering exactly the `[bracketed]`
//!   text.
//! - `lex-analysis::semantic_tokens` — emits per-marker semantic tokens
//!   (`*` open, content span, `*` close, …) for every inline element type.
//!
//! Before consolidation, each consumer carried its own walker with its own
//! copy of the escape-handling and marker-stepping logic. Any change to the
//! inline parser (e.g., new escape rule) had to land in two places and was
//! easy to miss. This module is the single source of truth.
//!
//! ## Inline-tree shape
//!
//! The inline parser produces nodes from these variants (see
//! [`InlineNode`]):
//!
//! | Variant | Source shape | Notes |
//! |---------|--------------|-------|
//! | `Plain` | `text` | Subject to escape rules; raw bytes can be longer than the unescaped char count. |
//! | `Strong` | `*content*` | `content` is recursive `[InlineNode]`; markers are single ASCII byte. |
//! | `Emphasis` | `_content_` | Same as Strong with `_` marker. |
//! | `Code` | `` `text` `` | `text` is literal — no escape processing inside. |
//! | `Math` | `#text#` | Same as Code with `#` marker. |
//! | `Reference` | `[content]` | `content` is literal; classified into `ReferenceType` already. |
//!
//! Containers (Strong/Emphasis) emit `enter_*` before recursing into
//! children and `leave_*` after — the `content` range passed to `leave_*`
//! covers the span between (but excluding) the markers. Literals
//! (Code/Math/Reference) get a single combined call with separate
//! `open_marker`, `content`, `close_marker` ranges so consumers can decorate
//! markers and content independently.

use super::range::{Position, Range};
use super::text_content::TextContent;
use crate::lex::inlines::{InlineNode, ReferenceInline};

/// Visitor for inline-tree walks performed by
/// [`walk_text_content_positions`]. Each method has a default no-op
/// implementation — callers override only what they care about.
///
/// Containers (`Strong`, `Emphasis`) fire `enter_*` before child recursion
/// and `leave_*` after, with the resolved content/close ranges passed at
/// `leave_*` time. Visitors that need to suppress something while inside a
/// container can track an `in_formatted` counter incremented on `enter_*`
/// and decremented on `leave_*`; the walker guarantees balanced nesting.
pub trait InlinePositionVisitor {
    fn visit_plain(&mut self, _range: &Range, _text: &str) {}
    fn enter_strong(&mut self, _open_marker: &Range) {}
    fn leave_strong(&mut self, _content: &Range, _close_marker: &Range) {}
    fn enter_emphasis(&mut self, _open_marker: &Range) {}
    fn leave_emphasis(&mut self, _content: &Range, _close_marker: &Range) {}
    fn visit_code(
        &mut self,
        _open_marker: &Range,
        _content: &Range,
        _close_marker: &Range,
        _text: &str,
    ) {
    }
    fn visit_math(
        &mut self,
        _open_marker: &Range,
        _content: &Range,
        _close_marker: &Range,
        _text: &str,
    ) {
    }
    fn visit_reference(
        &mut self,
        _open_marker: &Range,
        _content: &Range,
        _close_marker: &Range,
        _data: &ReferenceInline,
    ) {
    }
}

/// Walk `text`'s parsed inline tree, firing visitor callbacks with precise
/// source-position ranges.
///
/// Returns immediately without invoking the visitor when `text.location` is
/// `None` (no source range to anchor positions) or the raw text is empty.
/// Inline nodes come from [`TextContent::inlines`] when already parsed
/// (zero-allocation borrow — the common case after the standard
/// `parse_document` pipeline ran [`crate::lex::transforms::stages::ParseInlines`]),
/// falling back to [`TextContent::inline_items`] for programmatically
/// constructed ASTs that haven't been parsed yet.
///
/// Concretely the cursor advances through `text.as_string()` byte-for-byte,
/// applying the inline parser's escape rules (`\X` where `X` is
/// non-alphanumeric → 2 raw bytes for 1 unescaped char, any other backslash
/// stays literal). Marker characters (`*`, `_`, `` ` ``, `#`, `[`, `]`) are
/// counted by their UTF-8 width.
pub fn walk_text_content_positions<V: InlinePositionVisitor>(text: &TextContent, visitor: &mut V) {
    let Some(base_range) = text.location.as_ref() else {
        return;
    };
    let raw = text.as_string();
    if raw.is_empty() {
        return;
    }
    // Borrow when inlines were pre-parsed; only allocate when we have to
    // parse fresh. The standard `parse_document` pipeline always pre-parses,
    // so production traffic hits the borrow path.
    let owned;
    let nodes: &[InlineNode] = match text.inlines() {
        Some(borrowed) => borrowed,
        None => {
            owned = text.inline_items();
            &owned
        }
    };
    let mut walker = InlinePositionWalker {
        raw,
        base_range,
        cursor: 0,
    };
    walker.walk_nodes(nodes, visitor);
}

struct InlinePositionWalker<'a> {
    raw: &'a str,
    base_range: &'a Range,
    cursor: usize,
}

impl<'a> InlinePositionWalker<'a> {
    fn walk_nodes<V: InlinePositionVisitor>(&mut self, nodes: &[InlineNode], v: &mut V) {
        for node in nodes {
            self.walk_node(node, v);
        }
    }

    fn walk_node<V: InlinePositionVisitor>(&mut self, node: &InlineNode, v: &mut V) {
        match node {
            InlineNode::Plain { text, .. } => {
                let start = self.cursor;
                self.advance_unescaped(text);
                let end = self.cursor;
                if start < end {
                    let range = self.make_range(start, end);
                    v.visit_plain(&range, text);
                }
            }
            InlineNode::Strong { content, .. } => self.walk_strong(content, v),
            InlineNode::Emphasis { content, .. } => self.walk_emphasis(content, v),
            InlineNode::Code { text, .. } => self.walk_literal(text, '`', v, EmitLiteral::Code),
            InlineNode::Math { text, .. } => self.walk_literal(text, '#', v, EmitLiteral::Math),
            InlineNode::Reference { data, .. } => self.walk_reference(data, v),
        }
    }

    fn walk_strong<V: InlinePositionVisitor>(&mut self, children: &[InlineNode], v: &mut V) {
        let m = '*'.len_utf8();
        let open_start = self.cursor;
        self.cursor += m;
        let open = self.make_range(open_start, self.cursor);
        v.enter_strong(&open);

        let content_start = self.cursor;
        self.walk_nodes(children, v);
        let content_end = self.cursor;

        let close_start = self.cursor;
        self.cursor += m;
        let close = self.make_range(close_start, self.cursor);
        let content = self.make_range(content_start, content_end);
        v.leave_strong(&content, &close);
    }

    fn walk_emphasis<V: InlinePositionVisitor>(&mut self, children: &[InlineNode], v: &mut V) {
        let m = '_'.len_utf8();
        let open_start = self.cursor;
        self.cursor += m;
        let open = self.make_range(open_start, self.cursor);
        v.enter_emphasis(&open);

        let content_start = self.cursor;
        self.walk_nodes(children, v);
        let content_end = self.cursor;

        let close_start = self.cursor;
        self.cursor += m;
        let close = self.make_range(close_start, self.cursor);
        let content = self.make_range(content_start, content_end);
        v.leave_emphasis(&content, &close);
    }

    fn walk_literal<V: InlinePositionVisitor>(
        &mut self,
        text: &str,
        marker: char,
        v: &mut V,
        kind: EmitLiteral,
    ) {
        let m = marker.len_utf8();
        let open_start = self.cursor;
        self.cursor += m;
        let open = self.make_range(open_start, self.cursor);

        let content_start = self.cursor;
        self.cursor += text.len();
        let content = self.make_range(content_start, self.cursor);

        let close_start = self.cursor;
        self.cursor += m;
        let close = self.make_range(close_start, self.cursor);

        match kind {
            EmitLiteral::Code => v.visit_code(&open, &content, &close, text),
            EmitLiteral::Math => v.visit_math(&open, &content, &close, text),
        }
    }

    fn walk_reference<V: InlinePositionVisitor>(&mut self, data: &ReferenceInline, v: &mut V) {
        let open_start = self.cursor;
        self.cursor += 1;
        let open = self.make_range(open_start, self.cursor);

        let content_start = self.cursor;
        self.cursor += data.raw.len();
        let content = self.make_range(content_start, self.cursor);

        let close_start = self.cursor;
        self.cursor += 1;
        let close = self.make_range(close_start, self.cursor);

        v.visit_reference(&open, &content, &close, data);
    }

    /// Mirror the inline parser's escape handling so the cursor advances
    /// through raw bytes by the same amount the parser consumed when
    /// producing each unescaped char in the `Plain` node. `\X` with a
    /// non-alphanumeric `X` is consumed as 2 raw bytes for 1 unescaped char;
    /// any other backslash stays literal.
    fn advance_unescaped(&mut self, text: &str) {
        for _expected in text.chars() {
            if self.cursor >= self.raw.len() {
                break;
            }
            let raw_ch = self.raw[self.cursor..].chars().next().unwrap();
            if raw_ch == '\\' {
                if self.cursor + 1 >= self.raw.len() {
                    self.cursor += 1;
                } else {
                    let next_ch = self.raw[self.cursor + 1..].chars().next();
                    match next_ch {
                        Some(nc) if !nc.is_alphanumeric() => {
                            self.cursor += 1 + nc.len_utf8();
                        }
                        _ => {
                            self.cursor += 1;
                        }
                    }
                }
            } else {
                self.cursor += raw_ch.len_utf8();
            }
        }
    }

    fn make_range(&self, start: usize, end: usize) -> Range {
        let start_pos = self.position_at(start);
        let end_pos = self.position_at(end);
        Range::new(
            (self.base_range.span.start + start)..(self.base_range.span.start + end),
            start_pos,
            end_pos,
        )
    }

    fn position_at(&self, offset: usize) -> Position {
        let mut line = self.base_range.start.line;
        let mut column = self.base_range.start.column;
        for ch in self.raw[..offset].chars() {
            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += ch.len_utf8();
            }
        }
        Position::new(line, column)
    }
}

enum EmitLiteral {
    Code,
    Math,
}
