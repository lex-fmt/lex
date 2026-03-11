//! Inline token types and specifications
//!
//!     This module defines the token types for inline parsing. Inline elements are span-based
//!     elements that can start and end at arbitrary positions within text content. Unlike the
//!     line-based tokens (core and line), inline tokens operate at the character level and can
//!     be nested within each other.
//!
//!     For the complete inline grammar specification, see specs/v1/grammar-inline.lex.
//!
//!     Related token modules:
//!     - [core](super::core) - Character and word-level tokens from the logos lexer
//!     - [line](super::line) - Line-based tokens for the main parser
//!
//! Inline Token Types
//!
//!     These are the inline element types supported by lex:
//!
//!         - Strong: *text* (bold/emphasis)
//!         - Emphasis: _text_ (italic)
//!         - Code: `text` (monospace, literal)
//!         - Math: #formula# (mathematical notation, literal)
//!         - Reference: [target] (links, citations, footnotes)
//!
//!     References support multiple subtypes including:
//!         - Citations: [@key] or [@key1; @key2, pp. 42-45]
//!         - Footnotes: [^label] or [42]
//!         - Session references: [#2.1]
//!         - URLs: [https://example.com]
//!         - File paths: [./path/to/file]
//!         - TK placeholders: [TK] or [TK-identifier]
//!         - General references: [Section Title]
//!
//!     Inline elements have these properties:
//!     - Clear start and end markers (single character tokens)
//!     - Can be nested (except literal types)
//!     - Cannot break parent element boundaries
//!     - No space allowed between marker and content
//!
//! Token Specifications
//!
//!     Each inline type is defined by an InlineSpec that specifies:
//!     - The kind of inline element (from InlineKind enum)
//!     - Start and end tokens (characters)
//!     - Whether it's literal (no nested parsing inside)
//!     - Optional post-processing callback for complex logic
//!
//! Citation Parsing
//!
//!     Citations are a specialized form of reference that follow academic citation format.
//!     The reference token [target] is post-processed to detect citations starting with @.
//!     Citation parsing is handled by [crate::lex::inlines::citations] which extracts:
//!     - Multiple citation keys (separated by ; or ,)
//!     - Optional page locators (p. or pp. followed by page ranges)
//!     - Page ranges in various formats: single pages, ranges, or lists

/// The type of inline element
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum InlineKind {
    /// Strong/bold text: *text*
    Strong,
    /// Emphasized/italic text: _text_
    Emphasis,
    /// Inline code: `text` (literal, no nested inlines)
    Code,
    /// Mathematical notation: #formula# (literal, no nested inlines)
    Math,
    /// Reference (link, citation, footnote): \[target\] (literal, no nested inlines)
    Reference,
}

impl std::fmt::Display for InlineKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InlineKind::Strong => write!(f, "strong"),
            InlineKind::Emphasis => write!(f, "emphasis"),
            InlineKind::Code => write!(f, "code"),
            InlineKind::Math => write!(f, "math"),
            InlineKind::Reference => write!(f, "reference"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inline_kind_display() {
        assert_eq!(format!("{}", InlineKind::Strong), "strong");
        assert_eq!(format!("{}", InlineKind::Emphasis), "emphasis");
        assert_eq!(format!("{}", InlineKind::Code), "code");
        assert_eq!(format!("{}", InlineKind::Math), "math");
        assert_eq!(format!("{}", InlineKind::Reference), "reference");
    }

    #[test]
    fn test_inline_kind_equality() {
        assert_eq!(InlineKind::Strong, InlineKind::Strong);
        assert_ne!(InlineKind::Strong, InlineKind::Emphasis);
        assert_ne!(InlineKind::Code, InlineKind::Math);
    }
}
