//! Math inline processing with MathML conversion
//!
//! This module provides a post-processor for math inline nodes that parses
//! AsciiMath expressions and converts them to MathML, attaching the result
//! as an annotation with label `doc.data` and parameter `type=mathml`.
//!
//! # Example
//!
//! ```rust,ignore
//! use lex_parser::lex::inlines::{parse_inlines_with_parser, InlineParser};
//! use lex_parser::lex::inlines::math::math_to_mathml_processor;
//! use lex_parser::lex::token::InlineKind;
//!
//! let parser = InlineParser::new()
//!     .with_post_processor(InlineKind::Math, math_to_mathml_processor);
//!
//! let nodes = parser.parse("#x^2 + y#");
//! // The math node will have a doc.data annotation with MathML
//! ```

use crate::lex::ast::elements::inlines::InlineNode;
use crate::lex::ast::elements::{Annotation, Label, Parameter};

/// Post-processor that parses AsciiMath expressions and adds MathML annotations.
///
/// This function is designed to be used with [`InlineParser::with_post_processor`](crate::lex::inlines::InlineParser::with_post_processor).
/// It processes Math inline nodes by:
/// 1. Parsing the AsciiMath expression using the polymath-rs library
/// 2. Converting the result to MathML
/// 3. Attaching the MathML as an annotation with label `doc.data` and parameter `type=mathml`
///
/// # Arguments
///
/// * `node` - The inline node to process (only Math nodes are transformed)
///
/// # Returns
///
/// The same node with MathML annotation added if it's a Math node, otherwise unchanged.
pub fn math_to_mathml_processor(node: InlineNode) -> InlineNode {
    match node {
        InlineNode::Math {
            text,
            mut annotations,
        } => {
            // Parse AsciiMath to MathML using polymath-rs
            match parse_asciimath_to_mathml(&text) {
                Ok(mathml) => {
                    // Create annotation with MathML content
                    let params = vec![Parameter::new("type".to_string(), "mathml".to_string())];
                    let mut anno =
                        Annotation::with_parameters(Label::new("doc.data".to_string()), params);

                    // Store MathML in annotation children as a paragraph
                    // (This allows tooling to access the MathML easily)
                    anno.children
                        .push(crate::lex::ast::elements::ContentItem::TextLine(
                            crate::lex::ast::elements::TextLine::new(
                                crate::lex::ast::TextContent::from(mathml),
                            ),
                        ));

                    annotations.push(anno);
                    InlineNode::Math { text, annotations }
                }
                Err(err) => {
                    // On parse error, attach an error annotation instead
                    let params = vec![
                        Parameter::new("type".to_string(), "error".to_string()),
                        Parameter::new("message".to_string(), err),
                    ];
                    let anno =
                        Annotation::with_parameters(Label::new("doc.data".to_string()), params);
                    annotations.push(anno);
                    InlineNode::Math { text, annotations }
                }
            }
        }
        other => other,
    }
}

/// Parse AsciiMath expression to MathML using polymath-rs.
///
/// # Arguments
///
/// * `asciimath` - The AsciiMath expression to parse
///
/// # Returns
///
/// * `Ok(String)` - The MathML representation
/// * `Err(String)` - Error message if parsing fails
fn parse_asciimath_to_mathml(asciimath: &str) -> Result<String, String> {
    // Use the high-level API from polymath-rs
    let mathml = polymath_rs::to_math_ml(asciimath);
    Ok(mathml)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::inlines::InlineParser;
    use crate::lex::token::InlineKind;

    #[test]
    fn parses_simple_expression() {
        let result = parse_asciimath_to_mathml("x + y");
        assert!(result.is_ok());
        let mathml = result.unwrap();
        assert!(mathml.contains("<math"));
        assert!(mathml.contains("</math>"));
    }

    #[test]
    fn parses_superscript() {
        let result = parse_asciimath_to_mathml("x^2");
        assert!(result.is_ok());
        let mathml = result.unwrap();
        assert!(mathml.contains("<msup"));
    }

    #[test]
    fn parses_fraction() {
        let result = parse_asciimath_to_mathml("a/b");
        assert!(result.is_ok());
        let mathml = result.unwrap();
        assert!(mathml.contains("<mfrac"));
    }

    #[test]
    fn post_processor_adds_annotation() {
        let node = InlineNode::math("x + y".to_string());
        let processed = math_to_mathml_processor(node);

        match processed {
            InlineNode::Math { text, annotations } => {
                assert_eq!(text, "x + y");
                assert_eq!(annotations.len(), 1);
                assert_eq!(annotations[0].data.label.value, "doc.data");
                assert_eq!(annotations[0].data.parameters.len(), 1);
                assert_eq!(annotations[0].data.parameters[0].key, "type");
                assert_eq!(annotations[0].data.parameters[0].value, "mathml");

                // Check that MathML content was stored
                assert!(!annotations[0].children.is_empty());
            }
            _ => panic!("Expected Math node"),
        }
    }

    #[test]
    fn post_processor_preserves_non_math_nodes() {
        let node = InlineNode::plain("text".to_string());
        let processed = math_to_mathml_processor(node.clone());
        assert_eq!(processed, node);
    }

    #[test]
    fn integration_with_parser() {
        let parser =
            InlineParser::new().with_post_processor(InlineKind::Math, math_to_mathml_processor);

        let nodes = parser.parse("#x^2 + y#");

        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            InlineNode::Math { text, annotations } => {
                assert_eq!(text, "x^2 + y");
                assert_eq!(annotations.len(), 1);
                assert_eq!(annotations[0].data.label.value, "doc.data");
            }
            _ => panic!("Expected Math node"),
        }
    }

    #[test]
    fn integration_full_lex_document_to_mathml() {
        use crate::lex::loader::DocumentLoader;

        // Simple Lex document with a paragraph containing math inline
        let lex_source = "A formula #x^2 + y# in text.";

        // Parse the Lex document
        let loader = DocumentLoader::from_string(lex_source);
        let doc = loader.parse().expect("Failed to parse document");

        // Navigate to the paragraph
        let para = doc.root.first_paragraph().expect("Expected a paragraph");

        // Parse inlines with the MathML processor
        let parser =
            InlineParser::new().with_post_processor(InlineKind::Math, math_to_mathml_processor);
        let inlines = parser.parse(&para.text());

        // Verify we have 3 inline nodes: Plain, Math, Plain
        assert_eq!(inlines.len(), 3);

        // Check the math node has MathML annotation
        match &inlines[1] {
            InlineNode::Math { text, annotations } => {
                assert_eq!(text, "x^2 + y");
                assert_eq!(annotations.len(), 1);

                // Verify annotation structure
                let anno = &annotations[0];
                assert_eq!(anno.data.label.value, "doc.data");
                assert_eq!(anno.data.parameters.len(), 1);
                assert_eq!(anno.data.parameters[0].key, "type");
                assert_eq!(anno.data.parameters[0].value, "mathml");

                // Verify MathML content exists
                assert!(!anno.children.is_empty());

                // Extract and verify MathML contains expected elements
                if let crate::lex::ast::elements::ContentItem::TextLine(line) = &anno.children[0] {
                    let mathml = line.text();
                    assert!(mathml.contains("<math"));
                    assert!(mathml.contains("</math>"));
                    assert!(mathml.contains("<msup")); // superscript for x^2
                } else {
                    panic!("Expected TextLine in annotation children");
                }
            }
            _ => panic!("Expected Math node at position 1"),
        }
    }
}
