//! Verbatim block builder
//!
//!     Handles construction of verbatim block nodes from matched patterns.
//!
//!     The verbatim parsing is the only stateful parsing in the pipeline. It matches a
//!     subject line, then either an indented container (in-flow) or flat lines
//!     (full-width/groups), and requires the closing annotation at the same indentation as
//!     the subject.
//!
//!     Since verbatim content can hold non Lex content, its content can't be parsed. It can
//!     be lexed without prejudice, but not parsed. Not only would it be gibberish, but worse,
//!     in case it would trigger indent and dedent events, it would throw off the parsing and
//!     break the document. This is why verbatim parsing must come first in the grammar pattern
//!     matching order.
//!
//!     The end marker identification has to be very easy. That's the reason why it ends in a
//!     closing annotation (:: label ::), which is the only form that is not common on regular
//!     text and can be detected in a pre-pass without full parsing. The closing annotation
//!     must be at the same indentation level as the subject line.
//!
//!     This builder extracts the subject line, collects all content lines (which may be in a
//!     container for in-flow mode, or flat lines for full-width mode), and the closing data
//!     line to construct the verbatim block IR node.
//!
//!     See [grammar matcher](crate::lex::parsing::parser::GrammarMatcher::match_verbatim_block)
//!     for the imperative matching logic that identifies verbatim blocks before other patterns
//!     are tried.

use super::helpers::{collect_line_tokens, extract_annotation_header_tokens, extract_line_token};
use crate::lex::parsing::ir::{NodeType, ParseNode, ParseNodePayload};
use crate::lex::token::{LineContainer, LineType, Token};
use std::ops::Range;

/// Build a verbatim block node from a subject, arbitrary content lines, and a closing annotation.
pub(in crate::lex::parsing::parser::builder) fn build_verbatim_block(
    tokens: &[LineContainer],
    subject_idx: usize,
    content_range: Range<usize>,
    closing_idx: usize,
) -> Result<ParseNode, String> {
    let subject_token = extract_line_token(&tokens[subject_idx])?.clone();

    let mut content_lines = Vec::new();
    for idx in content_range {
        if let Some(container) = tokens.get(idx) {
            collect_line_tokens(container, &mut content_lines);
        }
    }

    let closing_token = extract_line_token(&tokens[closing_idx])?;
    if !matches!(closing_token.line_type, LineType::DataMarkerLine) {
        return Err("Verbatim blocks must end with a closing annotation (:: label ::)".to_string());
    }
    let header_tokens = extract_annotation_header_tokens(closing_token)?;

    // Detect whether this is a table or a verbatim block by checking the label.
    // The label is the first non-whitespace text token in the header.
    let is_table = header_tokens.iter().find_map(|(token, _)| match token {
        Token::Text(text) => Some(text.as_str()),
        _ => None,
    }) == Some("table");

    let (node_type, payload) = if is_table {
        (
            NodeType::Table,
            ParseNodePayload::Table {
                subject: subject_token,
                content_lines,
                closing_data_tokens: header_tokens,
            },
        )
    } else {
        (
            NodeType::VerbatimBlock,
            ParseNodePayload::VerbatimBlock {
                subject: subject_token,
                content_lines,
                closing_data_tokens: header_tokens,
            },
        )
    };

    Ok(ParseNode::new(node_type, vec![], vec![]).with_payload(payload))
}
