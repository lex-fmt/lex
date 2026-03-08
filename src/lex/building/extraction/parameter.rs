//! Parameter Data Extraction
//!
//! Extracts primitive data (text, byte ranges) for annotation parameters.
//! Handles key=value parsing with support for quoted and unquoted values.

use crate::lex::escape::is_quote_escaped;
use crate::lex::token::normalization::utilities::{compute_bounding_box, extract_text};
use crate::lex::token::Token;
use std::ops::Range as ByteRange;

/// Extracted data for a parameter (key=value pair).
///
/// Contains primitive data (text and byte ranges) for constructing a Parameter AST node.
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct ParameterData {
    /// The parameter key text
    pub key_text: String,
    /// The parameter value text (optional)
    pub value_text: Option<String>,
    /// Byte range of the key
    #[allow(dead_code)]
    pub key_byte_range: ByteRange<usize>,
    /// Byte range of the value (if present)
    #[allow(dead_code)]
    pub value_byte_range: Option<ByteRange<usize>>,
    /// Overall byte range spanning the entire parameter
    pub overall_byte_range: ByteRange<usize>,
}

/// Parse a single parameter (key=value or just key).
///
/// Returns the parameter data and the index after this parameter.
pub(super) fn parse_parameter(
    tokens: &[(Token, ByteRange<usize>)],
    start_idx: usize,
    source: &str,
) -> Option<(ParameterData, usize)> {
    let mut i = start_idx;

    // Skip leading whitespace and commas
    while i < tokens.len() && matches!(tokens[i].0, Token::Whitespace(_) | Token::Comma) {
        i += 1;
    }

    if i >= tokens.len() {
        return None;
    }

    // Collect key tokens
    let mut key_tokens = Vec::new();
    while i < tokens.len() {
        match &tokens[i].0 {
            Token::Text(_) | Token::Dash | Token::Number(_) | Token::Period => {
                key_tokens.push(tokens[i].clone());
                i += 1;
            }
            _ => break,
        }
    }

    if key_tokens.is_empty() {
        return None;
    }

    // Skip whitespace after key
    while i < tokens.len() && matches!(tokens[i].0, Token::Whitespace(_)) {
        i += 1;
    }

    // Check for '='
    let (value_tokens, value_range) = if i < tokens.len() && matches!(tokens[i].0, Token::Equals) {
        i += 1; // Skip '='

        // Skip whitespace after '='
        while i < tokens.len() && matches!(tokens[i].0, Token::Whitespace(_)) {
            i += 1;
        }

        // Collect value tokens
        let mut val_tokens = Vec::new();
        let is_quoted;

        // Check if value is quoted
        if i < tokens.len() && matches!(tokens[i].0, Token::Quote) {
            is_quoted = true;
            val_tokens.push(tokens[i].clone()); // Include opening quote
            i += 1;
            while i < tokens.len() {
                if matches!(tokens[i].0, Token::Quote) {
                    // Check if this quote is escaped by a preceding backslash
                    if is_quote_escaped(source.as_bytes(), tokens[i].1.start) {
                        // Escaped quote — include as content, not a delimiter
                        val_tokens.push(tokens[i].clone());
                        i += 1;
                        continue;
                    }
                    break; // Unescaped quote = closing delimiter
                }
                val_tokens.push(tokens[i].clone());
                i += 1;
            }
            if i < tokens.len() && matches!(tokens[i].0, Token::Quote) {
                val_tokens.push(tokens[i].clone()); // Include closing quote
                i += 1;
            }
        } else {
            is_quoted = false;
            // Unquoted value - collect until comma, LexMarker, BlankLine, or end
            while i < tokens.len() {
                match &tokens[i].0 {
                    Token::Comma | Token::LexMarker | Token::BlankLine(_) => break,
                    Token::Whitespace(_) => {
                        // Check if there's a comma, LexMarker, or BlankLine after whitespace
                        let mut peek = i + 1;
                        while peek < tokens.len() && matches!(tokens[peek].0, Token::Whitespace(_))
                        {
                            peek += 1;
                        }
                        if peek < tokens.len()
                            && matches!(
                                tokens[peek].0,
                                Token::Comma | Token::LexMarker | Token::BlankLine(_)
                            )
                        {
                            break;
                        }
                        val_tokens.push(tokens[i].clone());
                        i += 1;
                    }
                    _ => {
                        val_tokens.push(tokens[i].clone());
                        i += 1;
                    }
                }
            }
        }

        if !val_tokens.is_empty() {
            let val_range = compute_bounding_box(&val_tokens);
            let val_text = extract_text(val_range.clone(), source);
            // Only trim unquoted values - quoted values should preserve spaces
            let val_text = if is_quoted {
                val_text
            } else {
                val_text.trim().to_string()
            };
            (Some(val_text), Some(val_range))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let key_byte_range = compute_bounding_box(&key_tokens);
    let key_text = extract_text(key_byte_range.clone(), source)
        .trim()
        .to_string();

    // Compute overall range
    let overall_start = key_tokens.first().unwrap().1.start;
    let overall_end = if let Some(ref vr) = value_range {
        vr.end
    } else {
        key_tokens.last().unwrap().1.end
    };
    let overall_byte_range = overall_start..overall_end;

    Some((
        ParameterData {
            key_text,
            value_text: value_tokens,
            key_byte_range,
            value_byte_range: value_range,
            overall_byte_range,
        },
        i,
    ))
}
