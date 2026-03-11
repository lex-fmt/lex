//! Standard transform definitions
//!
//! This module provides pre-built transforms for common use cases.
//! All transforms are defined as static references using `once_cell::sync::Lazy`.

use crate::lex::assembling::{AttachAnnotations, AttachRoot};
use crate::lex::parsing::ir::ParseNode;
use crate::lex::parsing::Document;
use crate::lex::token::Token;
use crate::lex::transforms::stages::{
    CoreTokenization, ParseInlines, Parsing, SemanticIndentation,
};
use crate::lex::transforms::{Runnable, Transform};
use once_cell::sync::Lazy;
use std::ops::Range;

/// Type alias for token stream (to satisfy clippy::type_complexity)
pub type TokenStream = Vec<(Token, Range<usize>)>;

/// Type alias for lexing transform
pub type LexingTransform = Transform<String, TokenStream>;

/// Type alias for AST transform
pub type AstTransform = Transform<String, Document>;

/// Core tokenization transform: String → Vec<(Token, Range<usize>)>
///
/// Converts source text into a flat token stream using the logos lexer.
/// This is the first stage of any lex pipeline.
///
/// # Example
///
/// ```rust
/// use lex_parser::lex::transforms::standard::CORE_TOKENIZATION;
///
/// let tokens = CORE_TOKENIZATION.run("Hello world\n".to_string()).unwrap();
/// assert!(!tokens.is_empty());
/// ```
pub static CORE_TOKENIZATION: Lazy<LexingTransform> =
    Lazy::new(|| Transform::from_fn(Ok).then(CoreTokenization::new()));

/// Lexing transform: String → Vec<(Token, Range<usize>)>
///
/// Complete lexical analysis including:
/// 1. Core tokenization (logos)
/// 2. Semantic indentation (Indent/Dedent)
///
/// This produces a fully-processed token stream ready for parsing.
///
/// # Example
///
/// ```rust
/// use lex_parser::lex::transforms::standard::LEXING;
///
/// let tokens = LEXING.run("Session:\n    Content\n".to_string()).unwrap();
/// // tokens now include Indent/Dedent
/// ```
pub static LEXING: Lazy<LexingTransform> = Lazy::new(|| {
    Transform::from_fn(Ok)
        .then(CoreTokenization::new())
        .then(SemanticIndentation::new())
});

/// Type alias for IR transform
pub type IrTransform = Transform<String, ParseNode>;

/// String to IR transform: String → ParseNode
///
/// Pipeline from source text to intermediate representation (IR):
/// 1. Core tokenization
/// 2. Semantic indentation
/// 3. Line token grouping
/// 4. Parsing to IR
///
/// # Example
///
/// ```rust
/// use lex_parser::lex::transforms::standard::TO_IR;
///
/// let ir = TO_IR.run("Hello world\n".to_string()).unwrap();
/// ```
pub static TO_IR: Lazy<IrTransform> = Lazy::new(|| Transform::from_fn(Ok).then(Parsing::new()));

/// String to AST transform: String → Document
///
/// Complete pipeline from source text to parsed AST:
/// 1. Core tokenization
/// 2. Semantic indentation
/// 3. Line token grouping
/// 4. Parsing to IR
/// 5. Building AST root session
/// 6. Attaching root session to Document
/// 7. Attaching annotations as metadata
///
/// This is the standard transform for most use cases.
///
/// # Example
///
/// ```rust
/// use lex_parser::lex::transforms::standard::STRING_TO_AST;
///
/// let doc = STRING_TO_AST.run("Hello world\n".to_string()).unwrap();
/// assert!(!doc.root.children.is_empty());
/// ```
pub static STRING_TO_AST: Lazy<AstTransform> =
    Lazy::new(|| {
        Transform::from_fn(|s: String| {
            // Ensure source ends with newline (required for parsing)
            let source = if !s.is_empty() && !s.ends_with('\n') {
                format!("{s}\n")
            } else {
                s
            };

            // Run lexing
            let tokens = LEXING.run(source.clone())?;

            // Parse to AST
            let root = crate::lex::parsing::engine::parse_from_flat_tokens(tokens, &source)
                .map_err(|e| crate::lex::transforms::TransformError::StageFailed {
                    stage: "Parser".to_string(),
                    message: e.to_string(),
                })?;

            // Parse inline elements before assembly
            let root = ParseInlines::new().run(root)?;

            // Attach root session to a document
            let mut doc = AttachRoot::new().run(root)?;

            // Attach annotations as metadata
            doc = AttachAnnotations::new().run(doc)?;

            Ok(doc)
        })
    });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::token::Token;

    #[test]
    fn test_core_tokenization() {
        let result = CORE_TOKENIZATION.run("Hello world\n".to_string()).unwrap();
        assert!(!result.is_empty());

        // Should have Text tokens
        assert!(result.iter().any(|(t, _)| matches!(t, Token::Text(_))));
    }

    #[test]
    fn test_lexing_adds_indentation() {
        let source = "Session:\n    Content\n".to_string();
        let result = LEXING.run(source).unwrap();

        // Should have Indent and Dedent tokens
        assert!(result.iter().any(|(t, _)| matches!(t, Token::Indent(_))));
        assert!(result.iter().any(|(t, _)| matches!(t, Token::Dedent(_))));
    }

    #[test]
    fn test_lexing_no_indentation() {
        let source = "Hello\n".to_string();
        let result = LEXING.run(source).unwrap();

        // Should not have Indent/Dedent
        assert!(!result.iter().any(|(t, _)| matches!(t, Token::Indent(_))));
        assert!(!result.iter().any(|(t, _)| matches!(t, Token::Dedent(_))));
    }

    #[test]
    fn test_string_to_ast_simple() {
        let result = STRING_TO_AST.run("Hello world\n".to_string()).unwrap();
        assert!(!result.root.children.is_empty());
    }

    #[test]
    fn test_string_to_ast_with_session() {
        let source = "Session:\n    Content here\n".to_string();
        let result = STRING_TO_AST.run(source).unwrap();

        assert!(!result.root.children.is_empty());
    }

    #[test]
    fn test_string_to_ast_adds_newline() {
        // Test that source without trailing newline works
        let result = STRING_TO_AST.run("Hello world".to_string()).unwrap();
        assert!(!result.root.children.is_empty());
    }

    #[test]
    fn test_transforms_are_reusable() {
        // Test that we can use the same transform multiple times
        let result1 = LEXING.run("Hello\n".to_string()).unwrap();
        let result2 = LEXING.run("World\n".to_string()).unwrap();

        assert!(!result1.is_empty());
        assert!(!result2.is_empty());
    }
}
