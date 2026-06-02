//! Transform integration for lex-babel formats
//!
//! This module provides transform-style interfaces for format conversions.
//! While lex-parser provides the core transform infrastructure, lex-babel
//! adds serialization transforms that operate on AST nodes.

use crate::format::Format;
use crate::formats::lex::formatting_rules::FormattingRules;
use crate::formats::lex::LexFormat;
use lex_core::lex::ast::Document;

/// Serialize a Document to Lex format with default formatting rules
///
/// This provides a simple functional interface that can be used
/// in transform-style pipelines outside the standard lex-parser transforms.
///
/// # Example
///
/// ```
/// use lex_babel::transforms::serialize_to_lex;
/// use lex_core::lex::transforms::standard::STRING_TO_AST;
///
/// let source = "Hello world\n";
/// let doc = STRING_TO_AST.run(source.to_string()).unwrap();
/// let formatted = serialize_to_lex(&doc).unwrap();
/// assert_eq!(formatted, "Hello world\n");
/// ```
pub fn serialize_to_lex(doc: &Document) -> Result<String, String> {
    let format = LexFormat::default();
    format.serialize(doc).map_err(|e| e.to_string())
}

/// Serialize a Document to Lex format with custom formatting rules
///
/// # Example
///
/// ```
/// use lex_babel::transforms::serialize_to_lex_with_rules;
/// use lex_babel::formats::lex::formatting_rules::FormattingRules;
/// use lex_core::lex::transforms::standard::STRING_TO_AST;
///
/// let source = "Hello world\n";
/// let doc = STRING_TO_AST.run(source.to_string()).unwrap();
///
/// let mut rules = FormattingRules::default();
/// rules.indent_string = "  ".to_string(); // 2-space indent
///
/// let formatted = serialize_to_lex_with_rules(&doc, rules).unwrap();
/// ```
pub fn serialize_to_lex_with_rules(
    doc: &Document,
    rules: FormattingRules,
) -> Result<String, String> {
    let format = LexFormat::new(rules);
    format.serialize(doc).map_err(|e| e.to_string())
}

/// Round-trip transformation: parse and re-serialize
///
/// Useful for formatting operations and testing.
///
/// # Example
///
/// ```
/// use lex_babel::transforms::format_lex_source;
///
/// let source = "Hello world\n";
/// let formatted = format_lex_source(source).unwrap();
/// assert_eq!(formatted, "Hello world\n");
/// ```
pub fn format_lex_source(source: &str) -> Result<String, String> {
    use lex_core::lex::transforms::standard::STRING_TO_AST;

    let doc = STRING_TO_AST
        .run(source.to_string())
        .map_err(|e| e.to_string())?;

    serialize_to_lex(&doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::ast::{ContentItem, Paragraph};

    #[test]
    fn test_serialize_to_lex() {
        let doc = Document::with_content(vec![ContentItem::Paragraph(Paragraph::from_line(
            "Test".to_string(),
        ))]);

        let result = serialize_to_lex(&doc);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Test\n");
    }

    #[test]
    fn test_serialize_with_custom_rules() {
        let doc = Document::with_content(vec![ContentItem::Paragraph(Paragraph::from_line(
            "Test".to_string(),
        ))]);

        let rules = FormattingRules {
            indent_string: "  ".to_string(),
            ..Default::default()
        };

        let result = serialize_to_lex_with_rules(&doc, rules);
        assert!(result.is_ok());
    }

    #[test]
    fn test_format_lex_source() {
        let source = "Hello world\n";
        let formatted = format_lex_source(source);
        assert!(formatted.is_ok());
        assert_eq!(formatted.unwrap(), "Hello world\n");
    }

    #[test]
    fn test_round_trip_simple() {
        let original = "Introduction\n\n    This is a session.\n";
        let formatted = format_lex_source(original).unwrap();

        // Parse both and compare (structural equivalence)
        use lex_core::lex::transforms::standard::STRING_TO_AST;

        let doc1 = STRING_TO_AST.run(original.to_string()).unwrap();
        let doc2 = STRING_TO_AST.run(formatted.clone()).unwrap();

        // Both should parse successfully
        assert_eq!(doc1.root.children.len(), doc2.root.children.len());
    }
}
