//! Paragraph builder
//!
//! Handles construction of paragraph nodes.

use super::helpers::extract_line_token;
use crate::lex::parsing::ir::{NodeType, ParseNode};
use crate::lex::token::LineContainer;

/// Build a paragraph node
pub(in crate::lex::parsing::parser::builder) fn build_paragraph(
    tokens: &[LineContainer],
    start_idx: usize,
    end_idx: usize,
) -> Result<ParseNode, String> {
    let paragraph_tokens: Vec<_> = (start_idx..=end_idx)
        .filter_map(|idx| extract_line_token(&tokens[idx]).ok().cloned())
        .collect();

    let mut all_tokens = Vec::new();
    for line in paragraph_tokens {
        all_tokens.extend(
            line.source_tokens
                .into_iter()
                .zip(line.token_spans.into_iter()),
        );
    }

    Ok(ParseNode::new(NodeType::Paragraph, all_tokens, vec![]))
}
