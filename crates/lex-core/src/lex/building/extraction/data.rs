//! Data Node Extraction
//!
//! Extracts primitive data (text, byte ranges) from normalized token vectors
//! for building Data AST nodes (label + optional parameters).

use super::parameter::{parse_parameter, ParameterData};
use crate::lex::annotation::split_label_tokens_with_ranges;
use crate::lex::token::normalization::utilities::{compute_bounding_box, extract_text};
use crate::lex::token::Token;
use std::ops::Range as ByteRange;

/// Extracted data for building a `Data` AST node.
///
/// Contains the label text, parameters, and their byte ranges.
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct DataExtraction {
    /// The annotation label text
    pub label_text: String,
    /// Byte range of the label
    pub label_byte_range: ByteRange<usize>,
    /// Extracted parameter data
    pub parameters: Vec<ParameterData>,
}

/// Extract data node contents from tokens (between :: markers).
///
/// This function implements the full annotation header parsing logic:
/// 1. Identify label tokens (before any '=' sign)
/// 2. Parse parameters (key=value pairs)
/// 3. Extract text for all components
///
/// # Arguments
///
/// * `tokens` - The tokens between :: markers
/// * `source` - The original source string
///
/// # Returns
///
/// `DataExtraction` containing label text, parameters, and byte ranges
///
/// # Example
///
/// ```ignore
/// Input tokens: "warning severity=high, category=security"
/// Output: DataExtraction {
///   label_text: "warning",
///   parameters: [
///     { key: "severity", value: Some("high") },
///     { key: "category", value: Some("security") }
///   ]
/// }
/// ```
pub(in crate::lex::building) fn extract_data(
    tokens: Vec<(Token, ByteRange<usize>)>,
    source: &str,
) -> DataExtraction {
    if tokens.is_empty() {
        panic!("Annotation header tokens cannot be empty; parser must ensure labels are present");
    }

    // 1. Parse label
    let (label_tokens, mut i, has_label) = split_label_tokens_with_ranges(&tokens);
    if !has_label {
        panic!("Annotation header must include a label before parameters");
    }

    let (label_text, label_byte_range) = if !label_tokens.is_empty() {
        // Skip leading whitespace to avoid including the :: marker in the bounding box
        // (since the marker might be between the indentation and the label)
        let meaningful_tokens: Vec<_> = label_tokens
            .iter()
            .skip_while(|(t, _)| matches!(t, Token::Whitespace(_) | Token::Indentation))
            .cloned()
            .collect();

        if !meaningful_tokens.is_empty() {
            let range = compute_bounding_box(&meaningful_tokens);
            let text = extract_text(range.clone(), source).trim().to_string();
            (text, range)
        } else {
            (String::new(), 0..0)
        }
    } else {
        (String::new(), 0..0)
    };

    // 2. Parse parameters
    let mut parameters = Vec::new();
    while i < tokens.len() {
        if let Some((param_data, next_i)) = parse_parameter(&tokens, i, source) {
            parameters.push(param_data);
            i = next_i;
        } else {
            break;
        }
    }

    DataExtraction {
        label_text,
        label_byte_range,
        parameters,
    }
}
