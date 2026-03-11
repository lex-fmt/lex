//! Core tokenization stage
//!
//! Converts source text into a flat stream of tokens using the logos lexer.

use crate::lex::lexing::base_tokenization;
use crate::lex::token::Token;
use crate::lex::transforms::{Runnable, TransformError};
use std::ops::Range;

/// Core tokenization stage
///
/// Converts source text (String) into a flat token stream.
/// This is the first stage of the lexing pipeline.
///
/// # Input
/// - `String` - source code text
///
/// # Output
/// - `Vec<(Token, Range<usize>)>` - flat token stream with byte ranges
pub struct CoreTokenization;

impl CoreTokenization {
    pub fn new() -> Self {
        CoreTokenization
    }
}

impl Default for CoreTokenization {
    fn default() -> Self {
        Self::new()
    }
}

impl Runnable<String, Vec<(Token, Range<usize>)>> for CoreTokenization {
    fn run(&self, input: String) -> Result<Vec<(Token, Range<usize>)>, TransformError> {
        Ok(base_tokenization::tokenize(&input))
    }
}

// Also implement for &str for convenience
impl Runnable<&str, Vec<(Token, Range<usize>)>> for CoreTokenization {
    fn run(&self, input: &str) -> Result<Vec<(Token, Range<usize>)>, TransformError> {
        Ok(base_tokenization::tokenize(input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_tokenization_simple() {
        let stage = CoreTokenization::new();
        let result = stage.run("Hello world\n".to_string()).unwrap();

        assert!(!result.is_empty());
        // Should have Text("Hello"), Whitespace, Text("world"), BlankLine tokens
        assert!(result.len() >= 4);
    }

    #[test]
    fn test_core_tokenization_with_str() {
        let stage = CoreTokenization::new();
        let result = stage.run("Hello\n").unwrap();

        assert!(!result.is_empty());
    }

    #[test]
    fn test_core_tokenization_empty() {
        let stage = CoreTokenization::new();
        let result = stage.run("".to_string()).unwrap();

        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_core_tokenization_preserves_ranges() {
        let stage = CoreTokenization::new();
        let source = "Hello world\n";
        let result = stage.run(source.to_string()).unwrap();

        // Verify ranges point to valid positions
        for (token, range) in &result {
            assert!(range.start <= range.end);
            assert!(range.end <= source.len());

            // Verify we can extract text from source using range
            if !matches!(token, Token::Indent(_) | Token::Dedent(_)) {
                let _text = &source[range.clone()];
                // Should not panic
            }
        }
    }
}
