//! Definition Data Extraction
//!
//! Extracts primitive data (text, byte ranges) from normalized token vectors
//! for building Definition AST nodes.

use crate::lex::token::normalization::utilities::{compute_bounding_box, extract_text};
use crate::lex::token::Token;
use std::ops::Range as ByteRange;

/// Extracted data for building a Definition AST node.
///
/// Contains the subject text and its byte range.
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct DefinitionData {
    /// The definition subject text
    pub subject_text: String,
    /// Byte range of the subject
    pub subject_byte_range: ByteRange<usize>,
}

/// Extract definition data from subject tokens.
///
/// # Arguments
///
/// * `tokens` - Normalized token vector for the definition subject
/// * `source` - The original source string
///
/// # Returns
///
/// DefinitionData containing the subject text and byte range
pub(in crate::lex::building) fn extract_definition_data(
    tokens: Vec<(Token, ByteRange<usize>)>,
    source: &str,
) -> DefinitionData {
    let subject_byte_range = compute_bounding_box(&tokens);
    let subject_text = extract_text(subject_byte_range.clone(), source);

    DefinitionData {
        subject_text,
        subject_byte_range,
    }
}
