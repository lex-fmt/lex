//! Inline-level analysis utilities.
//!
//! Extracts positioned references from text content by walking the AST's InlineNode
//! tree and raw source text in parallel to compute correct byte positions.

use lex_core::lex::ast::{Position, Range, TextContent};
use lex_core::lex::inlines::{InlineNode, ReferenceInline, ReferenceType};

/// A reference found in inline text, with its source position and classified type.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedReference {
    pub range: Range,
    pub reference_type: ReferenceType,
    pub raw: String,
}

/// Extract all references from a text node with their source positions.
///
/// Walks the InlineNode tree (from `TextContent::inline_items()`) and the raw source
/// text in parallel. Non-reference nodes (Plain, Strong, Emphasis, Code, Math) are
/// skipped over — only Reference nodes produce output.
pub fn extract_references(text: &TextContent) -> Vec<PositionedReference> {
    let Some(base_range) = text.location.as_ref() else {
        return Vec::new();
    };
    let raw = text.as_string();
    if raw.is_empty() {
        return Vec::new();
    }
    let nodes = text.inline_items();
    let mut walker = ReferenceWalker {
        raw,
        base_range,
        cursor: 0,
        refs: Vec::new(),
    };
    walker.walk_nodes(&nodes);
    walker.refs
}

struct ReferenceWalker<'a> {
    raw: &'a str,
    base_range: &'a Range,
    cursor: usize,
    refs: Vec<PositionedReference>,
}

impl<'a> ReferenceWalker<'a> {
    fn walk_nodes(&mut self, nodes: &[InlineNode]) {
        for node in nodes {
            self.walk_node(node);
        }
    }

    fn walk_node(&mut self, node: &InlineNode) {
        match node {
            InlineNode::Plain { text, .. } => self.skip_plain(text),
            InlineNode::Strong { content, .. } => self.skip_container(content, '*'),
            InlineNode::Emphasis { content, .. } => self.skip_container(content, '_'),
            InlineNode::Code { text, .. } => self.skip_literal(text, '`'),
            InlineNode::Math { text, .. } => self.skip_literal(text, '#'),
            InlineNode::Reference { data, .. } => self.collect_reference(data),
        }
    }

    fn skip_plain(&mut self, text: &str) {
        self.advance_unescaped(text);
    }

    fn skip_container(&mut self, content: &[InlineNode], marker: char) {
        self.cursor += marker.len_utf8(); // opening marker
        self.walk_nodes(content);
        self.cursor += marker.len_utf8(); // closing marker
    }

    fn skip_literal(&mut self, text: &str, marker: char) {
        self.cursor += marker.len_utf8(); // opening marker
        self.cursor += text.len(); // verbatim content
        self.cursor += marker.len_utf8(); // closing marker
    }

    fn collect_reference(&mut self, data: &ReferenceInline) {
        self.cursor += 1; // opening '['

        let content_start = self.cursor;
        self.cursor += data.raw.len();
        let content_end = self.cursor;

        self.cursor += 1; // closing ']'

        if content_start < content_end {
            self.refs.push(PositionedReference {
                range: self.make_range(content_start, content_end),
                reference_type: data.reference_type.clone(),
                raw: data.raw.clone(),
            });
        }
    }

    /// Advance cursor through raw text matching unescaped plain text.
    fn advance_unescaped(&mut self, text: &str) {
        for _expected in text.chars() {
            if self.cursor >= self.raw.len() {
                break;
            }
            let raw_ch = self.raw[self.cursor..].chars().next().unwrap();
            if raw_ch == '\\' {
                if self.cursor + 1 >= self.raw.len() {
                    // Trailing backslash: treat as literal to avoid out-of-bounds slicing.
                    self.cursor += 1;
                } else {
                    let next_ch = self.raw[self.cursor + 1..].chars().next();
                    match next_ch {
                        Some(nc) if !nc.is_alphanumeric() => {
                            // Escaped: raw `\X` → unescaped `X`
                            self.cursor += 1 + nc.len_utf8();
                        }
                        _ => {
                            // Literal backslash
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

#[cfg(test)]
mod tests {
    use super::*;

    fn text_with_range(content: &str, line: usize, column: usize) -> TextContent {
        let start = Position::new(line, column);
        let end = Position::new(line, column + content.len());
        let range = Range::new(0..content.len(), start, end);
        TextContent::from_string(content.to_string(), Some(range))
    }

    #[test]
    fn extracts_references_with_classification() {
        let text = text_with_range("See [^note] and [@spec2024] plus [42]", 0, 0);
        let refs = extract_references(&text);
        assert_eq!(refs.len(), 3);
        assert!(refs
            .iter()
            .any(|r| matches!(r.reference_type, ReferenceType::AnnotationReference { .. })));
        assert!(refs
            .iter()
            .any(|r| matches!(r.reference_type, ReferenceType::Citation(_))));
        assert!(refs
            .iter()
            .any(|r| matches!(r.reference_type, ReferenceType::FootnoteNumber { .. })));
    }

    #[test]
    fn reference_ranges_are_correct() {
        let text = text_with_range("Hello [world] end", 0, 0);
        let refs = extract_references(&text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].raw, "world");
        // "world" starts at byte 7 (after "Hello ["), ends at byte 12
        assert_eq!(refs[0].range.span, 7..12);
    }

    #[test]
    fn references_inside_formatting() {
        let text = text_with_range("*bold [ref]* end", 0, 0);
        let refs = extract_references(&text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].raw, "ref");
    }

    #[test]
    fn escaped_brackets_not_references() {
        let text = text_with_range("\\[not a ref\\]", 0, 0);
        let refs = extract_references(&text);
        assert!(refs.is_empty());
    }

    #[test]
    fn empty_text_returns_nothing() {
        let text = text_with_range("", 0, 0);
        let refs = extract_references(&text);
        assert!(refs.is_empty());
    }

    #[test]
    fn no_location_returns_nothing() {
        let text = TextContent::from_string("Hello [world]".to_string(), None);
        let refs = extract_references(&text);
        assert!(refs.is_empty());
    }

    #[test]
    fn trailing_backslash_does_not_panic() {
        // Double backslash in raw text: `Hello\\` — should not panic.
        let text = text_with_range("Hello\\\\", 0, 0);
        let refs = extract_references(&text);
        assert!(refs.is_empty());

        // Single trailing backslash in raw text: `Hello\` — the critical edge case.
        let text2 = text_with_range("Hello\\", 0, 0);
        let refs2 = extract_references(&text2);
        assert!(refs2.is_empty());
    }
}
