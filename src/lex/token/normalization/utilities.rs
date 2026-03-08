//! Token processing utilities for the Immutable Log Architecture
//!
//! This module provides the core utilities for token manipulation.
//! All functions here are pure and thoroughly unit tested.
//!
//! # Architecture
//!
//! The Logos lexer produces `(Token, Range<usize>)` pairs - this is the ground truth.
//! Transformations create aggregate tokens that store these original pairs in `source_tokens`.
//! This module provides utilities to:
//! 1. Unroll aggregate tokens back to flat lists
//! 2. Flatten token vectors
//! 3. Compute bounding boxes from token ranges
//! 4. Extract text from ranges
//!
//! Note: Conversion from byte ranges to AST Range is handled in the `location` module.

use crate::lex::token::core::Token;
use std::ops::Range as ByteRange;

/// Trait that any token structure can implement to provide access to source tokens.
///
/// This enables the unrolling system to work with any parser's token representation.
#[allow(dead_code)]
pub trait SourceTokenProvider {
    /// Get the original Logos tokens that comprise this token.
    ///
    /// For atomic tokens (direct from Logos), this returns a slice containing just that token.
    /// For aggregate tokens (from transformations), this returns all the original tokens.
    fn source_tokens(&self) -> &[(Token, ByteRange<usize>)];
}

/// Unroll a collection of tokens to a flat list of original Logos tokens.
///
/// This recursively extracts all `source_tokens` from aggregate structures,
/// returning a flat list of the original `(Token, Range<usize>)` pairs that
/// came directly from the Logos lexer.
///
/// # Example
///
/// ```rust,ignore
/// let line_tokens: Vec<LineToken> = /* ... parsed tokens ... */;
/// let flat_tokens = unroll(&line_tokens);
/// // flat_tokens now contains all original Logos tokens
/// ```
#[allow(dead_code)]
pub fn unroll<T: SourceTokenProvider>(tokens: &[T]) -> Vec<(Token, ByteRange<usize>)> {
    tokens
        .iter()
        .flat_map(|t| t.source_tokens().iter().cloned())
        .collect()
}

/// Flatten a collection of token vectors into a single flat list.
///
/// This is useful for token types (like LineToken) that provide source tokens
/// as owned Vec rather than borrowed slices. It simply concatenates all the vectors.
///
/// # Example
///
/// ```rust,ignore
/// let line_tokens: Vec<LineToken> = /* ... parsed tokens ... */;
/// let token_vecs: Vec<Vec<(Token, Range<usize>)>> = line_tokens.iter()
///     .map(|lt| lt.source_token_pairs())
///     .collect();
/// let flat_tokens = flatten_token_vecs(&token_vecs);
/// ```
#[allow(dead_code)]
pub fn flatten_token_vecs(
    token_vecs: &[Vec<(Token, ByteRange<usize>)>],
) -> Vec<(Token, ByteRange<usize>)> {
    token_vecs.iter().flat_map(|v| v.iter().cloned()).collect()
}

/// Compute the bounding box (minimum start, maximum end) from a list of tokens.
///
/// Returns the smallest `Range<usize>` that encompasses all token ranges.
/// Returns `0..0` if the token list is empty.
///
/// # Example
///
/// ```rust,ignore
/// let tokens = vec![
///     (Token::Text("hello".into()), 0..5),
///     (Token::Whitespace(1), 5..6),
///     (Token::Text("world".into()), 6..11),
/// ];
/// let bbox = compute_bounding_box(&tokens);
/// assert_eq!(bbox, 0..11);
/// ```
pub fn compute_bounding_box(tokens: &[(Token, ByteRange<usize>)]) -> ByteRange<usize> {
    if tokens.is_empty() {
        return 0..0;
    }

    let min_start = tokens
        .iter()
        .map(|(_, range)| range.start)
        .min()
        .unwrap_or(0);
    let max_end = tokens.iter().map(|(_, range)| range.end).max().unwrap_or(0);

    min_start..max_end
}

/// Extract text from the source string at the given range.
///
/// # Arguments
///
/// * `range` - The byte offset range to extract
/// * `source` - The original source string
///
/// # Example
///
/// ```rust,ignore
/// let text = extract_text(0..5, "hello world");
/// assert_eq!(text, "hello");
/// ```
pub fn extract_text(range: ByteRange<usize>, source: &str) -> String {
    source[range].to_string()
}

/// Compute the 0-indexed column number for a given byte offset in the source string.
///
/// # Arguments
///
/// * `offset` - The byte offset to get the column for
/// * `source` - The original source string
///
/// # Example
///
/// ```rust,ignore
/// let source = "hello\n world";
/// // 'w' is at byte offset 6
/// let col = compute_column(6, source);
/// assert_eq!(col, 1);
/// ```
pub fn compute_column(offset: usize, source: &str) -> usize {
    let mut last_newline = 0;
    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            last_newline = i + 1;
        }
    }
    offset - last_newline
}

/// High-level convenience: extract text directly from tokens.
///
/// This combines `compute_bounding_box` and `extract_text` for convenience.
///
/// # Panics
///
/// Panics if tokens is empty.
#[allow(dead_code)]
pub fn tokens_to_text(tokens: &[(Token, ByteRange<usize>)], source: &str) -> String {
    let range = compute_bounding_box(tokens);
    extract_text(range, source)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock token provider for testing
    struct MockToken {
        tokens: Vec<(Token, ByteRange<usize>)>,
    }

    impl SourceTokenProvider for MockToken {
        fn source_tokens(&self) -> &[(Token, ByteRange<usize>)] {
            &self.tokens
        }
    }

    #[test]
    fn test_compute_bounding_box_single_token() {
        let tokens = vec![(
            Token::Text("hello".to_string()),
            ByteRange { start: 0, end: 5 },
        )];
        let bbox = compute_bounding_box(&tokens);
        assert_eq!(bbox, 0..5);
    }

    #[test]
    fn test_compute_bounding_box_multiple_contiguous() {
        let tokens = vec![
            (
                Token::Text("hello".to_string()),
                ByteRange { start: 0, end: 5 },
            ),
            (Token::Whitespace(1), ByteRange { start: 5, end: 6 }),
            (
                Token::Text("world".to_string()),
                ByteRange { start: 6, end: 11 },
            ),
        ];
        let bbox = compute_bounding_box(&tokens);
        assert_eq!(bbox, 0..11);
    }

    #[test]
    fn test_compute_bounding_box_non_contiguous() {
        // In case tokens have gaps (shouldn't happen normally, but test it)
        let tokens = vec![
            (
                Token::Text("hello".to_string()),
                ByteRange { start: 0, end: 5 },
            ),
            (
                Token::Text("world".to_string()),
                ByteRange { start: 10, end: 15 },
            ),
        ];
        let bbox = compute_bounding_box(&tokens);
        assert_eq!(bbox, 0..15);
    }

    #[test]
    fn test_compute_bounding_box_empty_returns_zero_range() {
        let tokens: Vec<(Token, ByteRange<usize>)> = vec![];
        assert_eq!(compute_bounding_box(&tokens), 0..0);
    }

    #[test]
    fn test_extract_text_simple() {
        let source = "hello world";
        assert_eq!(
            extract_text(ByteRange { start: 0, end: 5 }, source),
            "hello"
        );
        assert_eq!(
            extract_text(ByteRange { start: 6, end: 11 }, source),
            "world"
        );
    }

    #[test]
    fn test_extract_text_multiline() {
        let source = "line one\nline two\nline three";
        assert_eq!(
            extract_text(ByteRange { start: 0, end: 8 }, source),
            "line one"
        );
        assert_eq!(
            extract_text(ByteRange { start: 9, end: 17 }, source),
            "line two"
        );
    }

    #[test]
    fn test_extract_text_unicode() {
        let source = "hello 世界";
        // "世界" is 6 bytes (3 bytes per character)
        let text = extract_text(ByteRange { start: 6, end: 12 }, source);
        assert_eq!(text, "世界");
    }

    #[test]
    fn test_unroll_single_token() {
        let mock = MockToken {
            tokens: vec![(
                Token::Text("hello".to_string()),
                ByteRange { start: 0, end: 5 },
            )],
        };
        let unrolled = unroll(&[mock]);
        assert_eq!(unrolled.len(), 1);
        assert_eq!(unrolled[0].1, 0..5);
    }

    #[test]
    fn test_unroll_multiple_tokens() {
        let mock1 = MockToken {
            tokens: vec![(
                Token::Text("hello".to_string()),
                ByteRange { start: 0, end: 5 },
            )],
        };
        let mock2 = MockToken {
            tokens: vec![
                (Token::Whitespace(1), ByteRange { start: 5, end: 6 }),
                (
                    Token::Text("world".to_string()),
                    ByteRange { start: 6, end: 11 },
                ),
            ],
        };
        let unrolled = unroll(&[mock1, mock2]);
        assert_eq!(unrolled.len(), 3);
        assert_eq!(unrolled[0].1, 0..5);
        assert_eq!(unrolled[1].1, 5..6);
        assert_eq!(unrolled[2].1, 6..11);
    }

    #[test]
    fn test_tokens_to_text_convenience() {
        let source = "hello world";
        let tokens = vec![
            (
                Token::Text("hello".to_string()),
                ByteRange { start: 0, end: 5 },
            ),
            (Token::Whitespace(1), ByteRange { start: 5, end: 6 }),
        ];
        let text = tokens_to_text(&tokens, source);
        assert_eq!(text, "hello ");
    }

    #[test]
    fn test_flatten_token_vecs_empty() {
        let vecs: Vec<Vec<(Token, ByteRange<usize>)>> = vec![];
        let flattened = flatten_token_vecs(&vecs);
        assert_eq!(flattened.len(), 0);
    }

    #[test]
    fn test_flatten_token_vecs_single() {
        let vecs = vec![vec![
            (
                Token::Text("hello".to_string()),
                ByteRange { start: 0, end: 5 },
            ),
            (Token::Whitespace(1), ByteRange { start: 5, end: 6 }),
        ]];
        let flattened = flatten_token_vecs(&vecs);
        assert_eq!(flattened.len(), 2);
        assert_eq!(flattened[0].1, 0..5);
        assert_eq!(flattened[1].1, 5..6);
    }

    #[test]
    fn test_flatten_token_vecs_multiple() {
        let vecs = vec![
            vec![(
                Token::Text("hello".to_string()),
                ByteRange { start: 0, end: 5 },
            )],
            vec![
                (Token::Whitespace(1), ByteRange { start: 5, end: 6 }),
                (
                    Token::Text("world".to_string()),
                    ByteRange { start: 6, end: 11 },
                ),
            ],
        ];
        let flattened = flatten_token_vecs(&vecs);
        assert_eq!(flattened.len(), 3);
        assert_eq!(flattened[0].1, 0..5);
        assert_eq!(flattened[1].1, 5..6);
        assert_eq!(flattened[2].1, 6..11);
    }
}
