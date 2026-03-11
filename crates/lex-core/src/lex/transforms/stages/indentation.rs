//! Semantic indentation transformation stage
//!
//! Converts raw Indentation tokens into semantic Indent/Dedent pairs.

use crate::lex::lexing::transformations::semantic_indentation::SemanticIndentationMapper;
use crate::lex::token::Token;
use crate::lex::transforms::{Runnable, TransformError};
use std::ops::Range;

/// Semantic indentation stage
///
/// Transforms raw Indentation tokens into semantic Indent/Dedent pairs
/// based on indentation level changes between lines.
///
/// # Input
/// - `Vec<(Token, Range<usize>)>` - flat token stream with Indentation tokens
///
/// # Output
/// - `Vec<(Token, Range<usize>)>` - token stream with Indent/Dedent tokens
pub struct SemanticIndentation;

impl SemanticIndentation {
    pub fn new() -> Self {
        SemanticIndentation
    }
}

impl Default for SemanticIndentation {
    fn default() -> Self {
        Self::new()
    }
}

impl Runnable<Vec<(Token, Range<usize>)>, Vec<(Token, Range<usize>)>> for SemanticIndentation {
    fn run(
        &self,
        input: Vec<(Token, Range<usize>)>,
    ) -> Result<Vec<(Token, Range<usize>)>, TransformError> {
        let mut mapper = SemanticIndentationMapper::new();
        mapper.map(input).map_err(|e| TransformError::StageFailed {
            stage: "SemanticIndentation".to_string(),
            message: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::lexing::base_tokenization;

    #[test]
    fn test_semantic_indentation_no_indentation() {
        let stage = SemanticIndentation::new();
        let tokens = base_tokenization::tokenize("Hello\n");
        let result = stage.run(tokens).unwrap();

        // Should not have Indent or Dedent tokens
        assert!(!result.iter().any(|(t, _)| matches!(t, Token::Indent(_))));
        assert!(!result.iter().any(|(t, _)| matches!(t, Token::Dedent(_))));
    }

    #[test]
    fn test_semantic_indentation_simple() {
        let stage = SemanticIndentation::new();
        let source = "Hello:\n    World\n";
        let tokens = base_tokenization::tokenize(source);
        let result = stage.run(tokens).unwrap();

        // Should have Indent and Dedent tokens
        assert!(result.iter().any(|(t, _)| matches!(t, Token::Indent(_))));
        assert!(result.iter().any(|(t, _)| matches!(t, Token::Dedent(_))));
    }

    #[test]
    fn test_semantic_indentation_preserves_other_tokens() {
        let stage = SemanticIndentation::new();
        let source = "Hello\n";
        let tokens = base_tokenization::tokenize(source);

        let result = stage.run(tokens).unwrap();

        // Should preserve all non-indentation tokens
        let text_tokens: Vec<_> = result
            .iter()
            .filter(|(t, _)| matches!(t, Token::Text(_)))
            .collect();
        assert_eq!(text_tokens.len(), 1);
    }

    #[test]
    fn test_semantic_indentation_multiple_levels() {
        let stage = SemanticIndentation::new();
        let source = "A:\n    B:\n        C\n";
        let tokens = base_tokenization::tokenize(source);
        let result = stage.run(tokens).unwrap();

        // Should have 2 Indents and 2 Dedents
        let indent_count = result
            .iter()
            .filter(|(t, _)| matches!(t, Token::Indent(_)))
            .count();
        let dedent_count = result
            .iter()
            .filter(|(t, _)| matches!(t, Token::Dedent(_)))
            .count();

        assert_eq!(indent_count, 2);
        assert_eq!(dedent_count, 2);
    }
}
