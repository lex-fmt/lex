//! Annotation-specific helpers shared across lexer, parser, and builders.
//!
//! Currently this module focuses on analyzing annotation headers (the token
//! sequence between `::` markers). The helpers keep the "label vs parameters"
//! rules in one place so every stage enforces the same constraints.

use crate::lex::token::Token;
use std::ops::Range;

/// Result of analyzing the tokens inside an annotation header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotationHeaderAnalysis {
    /// Number of tokens (including leading whitespace) that belong to the label.
    pub label_token_count: usize,
    /// Whether a syntactic label (non-whitespace, non-parameter) was found.
    pub has_label: bool,
}

/// Analyze a raw slice of tokens located between `::` markers.
pub fn analyze_annotation_header_tokens(tokens: &[Token]) -> AnnotationHeaderAnalysis {
    analyze_slice(tokens, |token| token)
}

/// Analyze a slice of `(Token, Range)` pairs located between `::` markers.
pub fn analyze_annotation_header_token_pairs(
    tokens: &[(Token, Range<usize>)],
) -> AnnotationHeaderAnalysis {
    analyze_slice(tokens, |pair| &pair.0)
}

/// Slice helper used by the public analyzers.
fn analyze_slice<T>(tokens: &[T], mut get: impl FnMut(&T) -> &Token) -> AnnotationHeaderAnalysis {
    let len = tokens.len();
    let mut idx = 0;

    // Consume leading whitespace
    while idx < len && is_header_whitespace(get(&tokens[idx])) {
        idx += 1;
    }

    let mut consumed = idx;
    let mut has_label = false;

    while idx < len {
        let token = get(&tokens[idx]);
        if is_label_component(token) {
            // Check if this sequence is actually the start of a parameter key
            let mut check_idx = idx + 1;
            while check_idx < len && is_label_component(get(&tokens[check_idx])) {
                check_idx += 1;
            }
            while check_idx < len && is_header_whitespace(get(&tokens[check_idx])) {
                check_idx += 1;
            }
            if check_idx < len && matches!(get(&tokens[check_idx]), Token::Equals) {
                break;
            }

            idx += 1;
            consumed = idx;
            has_label = true;
        } else if is_header_whitespace(token) {
            idx += 1;
            consumed = idx;
        } else {
            break;
        }
    }

    AnnotationHeaderAnalysis {
        label_token_count: consumed,
        has_label,
    }
}

fn is_label_component(token: &Token) -> bool {
    matches!(
        token,
        Token::Text(_) | Token::Dash | Token::Number(_) | Token::Period
    )
}

fn is_header_whitespace(token: &Token) -> bool {
    matches!(token, Token::Whitespace(_) | Token::Indentation)
}

/// Collect the tokens that compose the label segment (including leading
/// whitespace) and return the index of the next token after the label.
pub fn split_label_tokens_with_ranges(
    tokens: &[(Token, Range<usize>)],
) -> (Vec<(Token, Range<usize>)>, usize, bool) {
    let analysis = analyze_annotation_header_token_pairs(tokens);
    let label_tokens = tokens[..analysis.label_token_count].to_vec();
    (label_tokens, analysis.label_token_count, analysis.has_label)
}
