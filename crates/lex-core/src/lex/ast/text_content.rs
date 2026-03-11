//! TextContent facade for representing user content text
//!
//! This module provides the `TextContent` type, which serves as a stable
//! interface for user-provided text throughout the AST. The facade is designed
//! to evolve over time:
//!
//! - Phase 1 (current): Plain text strings with source position tracking
//! - Phase 2 (future): Parsed inline elements (bold, italic, links, etc.)
//!
//! By using a facade, we can evolve from Phase 1 to Phase 2 without changing
//! the AST node types. External code accesses text via stable API methods
//! (.as_string(), future: .as_inlines()), which work regardless of the
//! internal representation.

use super::range::Range;
use crate::lex::inlines::{InlineContent, InlineNode};

/// Represents user-provided text content with source position tracking.
///
/// TextContent acts as a facade over different internal representations,
/// allowing the text layer to evolve without breaking the AST structure.
/// Currently stores plain text; future versions will support parsed inline nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct TextContent {
    /// Location in the source covering this text
    pub location: Option<Range>,
    /// Internal representation (evolves over time)
    inner: TextRepresentation,
}

/// Internal representation of text content.
///
/// This enum encapsulates the actual text storage format. It can evolve
/// without changing the public TextContent API.
#[derive(Debug, Clone, PartialEq)]
enum TextRepresentation {
    /// Plain text as a String.
    /// May contain formatting markers like "bold" or "_italic_"
    /// that will be parsed in Phase 2.
    Text(String),
    /// Parsed inline nodes along with the original raw string.
    Inlines { raw: String, nodes: InlineContent },
}

impl TextContent {
    /// Create TextContent from a string and optional source location.
    ///
    /// # Arguments
    /// * `text` - The raw text content
    /// * `location` - Optional source location of this text
    ///
    ///
    pub fn from_string(text: String, location: Option<Range>) -> Self {
        Self {
            location,
            inner: TextRepresentation::Text(text),
        }
    }

    /// Create empty TextContent.
    pub fn empty() -> Self {
        Self {
            location: None,
            inner: TextRepresentation::Text(String::new()),
        }
    }

    /// Get the text content as a string slice.
    ///
    /// Works regardless of internal representation. In Phase 1, returns the
    /// stored string directly. In Phase 2, would flatten inline nodes to text.
    ///
    ///
    pub fn as_string(&self) -> &str {
        match &self.inner {
            TextRepresentation::Text(s) => s,
            TextRepresentation::Inlines { raw, .. } => raw,
        }
    }

    /// Get mutable access to the text content.
    ///
    /// Note: Only available in Phase 1. Once inlines are parsed,
    /// you would need to reconstruct inlines after mutations.
    ///
    /// # Panics
    /// In Phase 2, this may panic or return an error if inlines have been parsed.
    pub fn as_string_mut(&mut self) -> &mut String {
        match &mut self.inner {
            TextRepresentation::Text(s) => s,
            TextRepresentation::Inlines { .. } => {
                panic!(
                    "TextContent::as_string_mut cannot be used after inline parsing has occurred"
                )
            }
        }
    }

    /// Check if content is empty.
    pub fn is_empty(&self) -> bool {
        self.as_string().is_empty()
    }

    /// Get the length of the content in characters.
    pub fn len(&self) -> usize {
        self.as_string().len()
    }

    /// Parse inline items contained in this text.
    pub fn inline_items(&self) -> InlineContent {
        match &self.inner {
            TextRepresentation::Text(s) => crate::lex::inlines::parse_inlines(s),
            TextRepresentation::Inlines { nodes, .. } => nodes.clone(),
        }
    }

    /// Returns a reference to parsed inline nodes when available.
    pub fn inline_nodes(&self) -> Option<&[InlineNode]> {
        match &self.inner {
            TextRepresentation::Inlines { nodes, .. } => Some(nodes),
            _ => None,
        }
    }

    /// Parse inline nodes (if not already parsed) and store them in this TextContent.
    pub fn ensure_inline_parsed(&mut self) {
        if matches!(self.inner, TextRepresentation::Inlines { .. }) {
            return;
        }

        let raw = match std::mem::replace(&mut self.inner, TextRepresentation::Text(String::new()))
        {
            TextRepresentation::Text(raw) => raw,
            TextRepresentation::Inlines { raw, nodes } => {
                self.inner = TextRepresentation::Inlines { raw, nodes };
                return;
            }
        };
        let nodes = crate::lex::inlines::parse_inlines(&raw);
        self.inner = TextRepresentation::Inlines { raw, nodes };
    }

    // ============================================================================
    // LSP-FRIENDLY APIS (Issue #290)
    // ============================================================================

    /// Get parsed inline nodes if available (LSP API).
    ///
    /// Returns `Some` if inlines have been parsed via `parse_inlines()` or `inlines_or_parse()`.
    /// Returns `None` if content is still in plain text form.
    ///
    /// This is a convenience alias for `inline_nodes()`.
    #[inline]
    pub fn inlines(&self) -> Option<&[InlineNode]> {
        self.inline_nodes()
    }

    /// Parse text into inline nodes and store the result (LSP API).
    ///
    /// This method is idempotent - calling it multiple times has no additional effect.
    /// After calling this method, `inlines()` will return `Some`.
    ///
    /// This is a convenience alias for `ensure_inline_parsed()`.
    #[inline]
    pub fn parse_inlines(&mut self) {
        self.ensure_inline_parsed();
    }

    /// Get or parse inline nodes (LSP API).
    ///
    /// If inlines are already parsed, returns a reference to them.
    /// Otherwise, parses the text into inlines, stores the result, and returns a reference.
    ///
    /// This is the recommended method for LSP features that need access to inline elements.
    pub fn inlines_or_parse(&mut self) -> &[InlineNode] {
        self.ensure_inline_parsed();
        self.inline_nodes()
            .expect("inline_nodes should be available after ensure_inline_parsed")
    }
}

impl Default for TextContent {
    fn default() -> Self {
        Self::empty()
    }
}

impl From<String> for TextContent {
    fn from(text: String) -> Self {
        Self::from_string(text, None)
    }
}

impl From<&str> for TextContent {
    fn from(text: &str) -> Self {
        Self::from_string(text.to_string(), None)
    }
}

impl AsRef<str> for TextContent {
    fn as_ref(&self) -> &str {
        self.as_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_from_string() {
        let content = TextContent::from_string("Hello".to_string(), None);
        assert_eq!(content.as_string(), "Hello");
    }

    #[test]
    fn test_empty() {
        let content = TextContent::empty();
        assert!(content.is_empty());
        assert_eq!(content.as_string().len(), 0);
    }

    #[test]
    fn test_from_string_trait() {
        let content = TextContent::from("Hello".to_string());
        assert_eq!(content.as_string(), "Hello");
    }

    #[test]
    fn test_from_str_trait() {
        let content = TextContent::from("Hello");
        assert_eq!(content.as_string(), "Hello");
    }

    #[test]
    fn test_as_ref() {
        let content = TextContent::from("Hello");
        let text: &str = content.as_ref();
        assert_eq!(text, "Hello");
    }

    #[test]
    fn test() {
        let location = Range::new(0..0, Position::new(0, 0), Position::new(0, 5));
        let content = TextContent::from_string("Hello".to_string(), Some(location.clone()));
        assert_eq!(content.location, Some(location));
    }

    #[test]
    fn test_mutate() {
        let mut content = TextContent::from_string("Hello".to_string(), None);
        *content.as_string_mut() = "World".to_string();
        assert_eq!(content.as_string(), "World");
    }

    #[test]
    fn parses_inline_items() {
        use crate::lex::inlines::InlineNode;

        let content = TextContent::from_string("Hello *world*".to_string(), None);
        let nodes = content.inline_items();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0], InlineNode::plain("Hello ".into()));
        match &nodes[1] {
            InlineNode::Strong { content, .. } => {
                assert_eq!(content, &vec![InlineNode::plain("world".into())]);
            }
            other => panic!("Unexpected inline node: {other:?}"),
        }
    }

    #[test]
    fn persists_inline_nodes_after_parsing() {
        use crate::lex::inlines::InlineNode;

        let mut content = TextContent::from_string("Hello *world*".to_string(), None);
        assert!(content.inline_nodes().is_none());

        content.ensure_inline_parsed();
        let nodes = content.inline_nodes().expect("expected inline nodes");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0], InlineNode::plain("Hello ".into()));
        match &nodes[1] {
            InlineNode::Strong { content, .. } => {
                assert_eq!(content, &vec![InlineNode::plain("world".into())]);
            }
            other => panic!("Unexpected inline node: {other:?}"),
        }

        // inline_items should reuse the stored nodes rather than re-parse
        assert_eq!(content.inline_items(), nodes.to_vec());
        assert_eq!(content.as_string(), "Hello *world*");
    }

    use super::super::range::Position;

    // ============================================================================
    // LSP API TESTS (Issue #290)
    // ============================================================================

    #[test]
    fn test_inlines_alias() {
        let mut content = TextContent::from_string("Hello *world*".to_string(), None);

        // Before parsing
        assert!(content.inlines().is_none());

        // After parsing
        content.parse_inlines();
        let nodes = content.inlines().expect("expected inline nodes");
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_parse_inlines_alias() {
        let mut content = TextContent::from_string("Hello *world*".to_string(), None);

        content.parse_inlines();
        assert!(content.inlines().is_some());

        // Idempotent - calling again should not panic
        content.parse_inlines();
        assert!(content.inlines().is_some());
    }

    #[test]
    fn test_inlines_or_parse() {
        let mut content = TextContent::from_string("Hello *world*".to_string(), None);

        // First call parses
        {
            let nodes1 = content.inlines_or_parse();
            assert_eq!(nodes1.len(), 2);
        }

        // Second call returns cached result
        {
            let nodes2 = content.inlines_or_parse();
            assert_eq!(nodes2.len(), 2);
        }
    }

    #[test]
    fn test_inlines_or_parse_with_references() {
        use crate::lex::inlines::InlineNode;

        let mut content =
            TextContent::from_string("See [42] and [https://example.com]".to_string(), None);
        let nodes = content.inlines_or_parse();

        // Should have: Plain, Reference, Plain, Reference
        assert_eq!(nodes.len(), 4);
        assert!(matches!(nodes[0], InlineNode::Plain { .. }));
        assert!(matches!(nodes[1], InlineNode::Reference { .. }));
        assert!(matches!(nodes[2], InlineNode::Plain { .. }));
        assert!(matches!(nodes[3], InlineNode::Reference { .. }));
    }
}
