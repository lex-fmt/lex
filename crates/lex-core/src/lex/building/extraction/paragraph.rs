//! Paragraph Data Extraction
//!
//! Extracts primitive data (text, byte ranges) from normalized token vectors
//! for building Paragraph AST nodes.

use crate::lex::token::normalization::utilities::{compute_bounding_box, extract_text};
use crate::lex::token::Token;
use std::ops::Range as ByteRange;

/// Extracted data for building a Paragraph AST node.
///
/// Contains the text and byte ranges for each line, plus the overall byte range.
/// All ranges are byte offsets (Range<usize>), not ast::Range.
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct ParagraphData {
    /// Text and byte range for each line in the paragraph
    pub text_lines: Vec<(String, ByteRange<usize>)>,
    /// Overall byte range spanning all lines
    pub overall_byte_range: ByteRange<usize>,
}

/// Extract paragraph data from token lines.
///
/// Each inner vector represents one line of the paragraph.
/// Extracts text and computes byte ranges for each line and the overall paragraph.
///
/// # Arguments
///
/// * `token_lines` - Normalized token vectors, one per line
/// * `source` - The original source string
///
/// # Returns
///
/// ParagraphData containing text and byte ranges for the paragraph
///
/// # Example
///
/// ```rust,ignore
/// let token_lines = vec![
///     vec![(Token::Text("line1".into()), 0..5)],
///     vec![(Token::Text("line2".into()), 6..11)],
/// ];
/// let data = extract_paragraph_data(token_lines, source);
/// assert_eq!(data.text_lines.len(), 2);
/// ```
pub(in crate::lex::building) fn extract_paragraph_data(
    token_lines: Vec<Vec<(Token, ByteRange<usize>)>>,
    source: &str,
) -> ParagraphData {
    let text_lines: Vec<(String, ByteRange<usize>)> = token_lines
        .iter()
        .map(|tokens| {
            let byte_range = compute_bounding_box(tokens);
            let text = extract_text(byte_range.clone(), source);
            (text, byte_range)
        })
        .collect();

    // Compute overall byte range from all tokens
    let all_tokens: Vec<(Token, ByteRange<usize>)> = token_lines.into_iter().flatten().collect();
    let overall_byte_range = if all_tokens.is_empty() {
        0..0
    } else {
        compute_bounding_box(&all_tokens)
    };

    ParagraphData {
        text_lines,
        overall_byte_range,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_paragraph_data() {
        let source = "hello world";
        let token_lines = vec![vec![(Token::Text("hello".to_string()), 0..5)]];

        let data = extract_paragraph_data(token_lines, source);

        assert_eq!(data.text_lines.len(), 1);
        assert_eq!(data.text_lines[0].0, "hello");
        assert_eq!(data.text_lines[0].1, 0..5);
        assert_eq!(data.overall_byte_range, 0..5);
    }
}
