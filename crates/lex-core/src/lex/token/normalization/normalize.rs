//! Token Normalization
//!
//! This module provides utilities to normalize various token formats into standard vectors
//! of `(Token, Range<usize>)` pairs. This is the first step in the AST construction pipeline.
//!
//! # Architecture
//!
//! ```text
//! Various Token Formats → Normalization → Vec<(Token, Range<usize>)>
//! (LineToken, etc.)                       ↓
//!                                         Standard format for data extraction
//! ```
//!
//! # Responsibilities
//!
//! - Convert LineToken → Vec<(Token, Range<usize>)>
//! - Flatten nested token structures
//! - Provide consistent interface regardless of input token format
//!
//! # Usage
//!
//! ```rust,ignore
//! use crate::lex::token::normalization;
//!
//! // Normalize a single line token
//! let tokens = normalization::normalize_line_token(&line_token);
//!
//! // Normalize multiple line tokens
//! let token_lines = normalization::normalize_line_tokens(&line_tokens);
//!
//! // Flatten to single vector
//! let flat_tokens = token_normalization::flatten(&token_lines);
//! ```

use crate::lex::token::{LineToken, Token};
use std::ops::Range as ByteRange;

use super::utilities::flatten_token_vecs;

// ============================================================================
// SINGLE LINE TOKEN NORMALIZATION
// ============================================================================

/// Normalize a single LineToken to a vector of (Token, Range) pairs.
///
/// This is the primary normalization function for parser tokens.
/// It extracts the source tokens and their byte ranges from the LineToken
/// structure.
///
/// # Arguments
///
/// * `token` - The LineToken to normalize
///
/// # Returns
///
/// A vector of (Token, Range<usize>) pairs representing the line's tokens
///
/// # Example
///
/// ```rust,ignore
/// let line_token: LineToken = /* ... from parser ... */;
/// let tokens = normalize_line_token(&line_token);
/// // tokens is now Vec<(Token, Range<usize>)>
/// ```
pub(crate) fn normalize_line_token(token: &LineToken) -> Vec<(Token, ByteRange<usize>)> {
    token.source_token_pairs()
}

// ============================================================================
// MULTIPLE LINE TOKENS NORMALIZATION
// ============================================================================

/// Normalize multiple LineTokens to vectors of (Token, Range) pairs.
///
/// This preserves line boundaries - each LineToken becomes a separate vector.
/// Useful when you need to process tokens per-line (e.g., for indentation wall
/// calculation in verbatim blocks).
///
/// # Arguments
///
/// * `tokens` - The LineTokens to normalize
///
/// # Returns
///
/// A vector of token vectors, where each inner vector represents one line
///
/// # Example
///
/// ```rust,ignore
/// let line_tokens: Vec<LineToken> = /* ... from parser ... */;
/// let token_lines = normalize_line_tokens(&line_tokens);
/// // token_lines[0] is the tokens from the first line
/// // token_lines[1] is the tokens from the second line, etc.
/// ```
pub(crate) fn normalize_line_tokens(tokens: &[LineToken]) -> Vec<Vec<(Token, ByteRange<usize>)>> {
    tokens.iter().map(normalize_line_token).collect()
}

// ============================================================================
// FLATTENING OPERATIONS
// ============================================================================

/// Flatten normalized token lines into a single vector.
///
/// This is useful when you need all tokens together (e.g., to compute an
/// overall bounding box for location tracking).
///
/// # Arguments
///
/// * `token_lines` - The normalized token lines to flatten
///
/// # Returns
///
/// A single flat vector containing all tokens from all lines
///
/// # Example
///
/// ```rust,ignore
/// let token_lines = normalize_line_tokens(&line_tokens);
/// let all_tokens = flatten(&token_lines);
/// // all_tokens contains every token from every line
/// ```
#[allow(dead_code)]
pub(crate) fn flatten(
    token_lines: &[Vec<(Token, ByteRange<usize>)>],
) -> Vec<(Token, ByteRange<usize>)> {
    flatten_token_vecs(token_lines)
}

// ============================================================================
// NORMALIZED TOKEN SUPPORT
// ============================================================================

/// Normalize tokens that are already in (Token, Range) format.
///
/// Pass-through for callers that already have tokens in normalized format.
/// Provided for API consistency.
///
/// # Arguments
///
/// * `tokens` - Tokens already in standard format
///
/// # Returns
///
/// The same tokens (cloned for ownership)
///
/// # Example
///
/// ```rust,ignore
/// let tokens: Vec<(Token, Range<usize>)> = /* ... already normalized ... */;
/// let normalized = normalize_token_pairs(&tokens);
/// // Same as input, but owned
/// ```
#[allow(dead_code)]
pub(crate) fn normalize_token_pairs(
    tokens: &[(Token, ByteRange<usize>)],
) -> Vec<(Token, ByteRange<usize>)> {
    tokens.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::token::{LineType, Token};

    fn make_line_token(tokens: Vec<Token>, spans: Vec<ByteRange<usize>>) -> LineToken {
        LineToken {
            source_tokens: tokens,
            token_spans: spans,
            line_type: LineType::ParagraphLine,
        }
    }

    #[test]
    fn test_normalize_line_token() {
        let token = make_line_token(
            vec![Token::Text("hello".to_string()), Token::Whitespace(1)],
            vec![0..5, 5..6],
        );

        let normalized = normalize_line_token(&token);

        assert_eq!(normalized.len(), 2);
        assert!(matches!(normalized[0].0, Token::Text(_)));
        assert_eq!(normalized[0].1, 0..5);
        assert!(matches!(normalized[1].0, Token::Whitespace(_)));
        assert_eq!(normalized[1].1, 5..6);
    }

    #[test]
    fn test_normalize_line_tokens() {
        #[allow(clippy::single_range_in_vec_init)]
        let tokens = vec![
            make_line_token(vec![Token::Text("line1".to_string())], vec![0..5]),
            make_line_token(vec![Token::Text("line2".to_string())], vec![6..11]),
        ];

        let normalized = normalize_line_tokens(&tokens);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].len(), 1);
        assert_eq!(normalized[1].len(), 1);
        assert_eq!(normalized[0][0].1, 0..5);
        assert_eq!(normalized[1][0].1, 6..11);
    }

    #[test]
    fn test_flatten() {
        let token_lines = vec![
            vec![
                (Token::Text("hello".to_string()), 0..5),
                (Token::Whitespace(1), 5..6),
            ],
            vec![
                (Token::Text("world".to_string()), 6..11),
                (Token::BlankLine(Some("\n".to_string())), 11..12),
            ],
        ];

        let flat = flatten(&token_lines);

        assert_eq!(flat.len(), 4);
        assert_eq!(flat[0].1, 0..5);
        assert_eq!(flat[1].1, 5..6);
        assert_eq!(flat[2].1, 6..11);
        assert_eq!(flat[3].1, 11..12);
    }

    #[test]
    fn test_normalize_token_pairs() {
        let tokens = vec![
            (Token::Text("test".to_string()), 0..4),
            (Token::Whitespace(1), 4..5),
        ];

        let normalized = normalize_token_pairs(&tokens);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].1, 0..4);
        assert_eq!(normalized[1].1, 4..5);
    }
}
