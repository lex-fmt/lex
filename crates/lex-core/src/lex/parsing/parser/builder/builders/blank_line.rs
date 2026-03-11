use crate::lex::parsing::ir::{NodeType, ParseNode};
use crate::lex::token::{LineContainer, LineType};
use std::ops::Range;

/// Build a BlankLineGroup parse node from a matched blank line range.
pub(in crate::lex::parsing::parser::builder) fn build_blank_line_group(
    tokens: &[LineContainer],
    token_range: Range<usize>,
) -> Result<ParseNode, String> {
    let mut flat_tokens = Vec::new();

    for idx in token_range {
        match &tokens[idx] {
            LineContainer::Token(line) if line.line_type == LineType::BlankLine => {
                flat_tokens.extend(
                    line.source_tokens
                        .iter()
                        .cloned()
                        .zip(line.token_spans.iter().cloned()),
                );
            }
            LineContainer::Token(line) => {
                return Err(format!(
                    "Expected BlankLine token but found {:?}",
                    line.line_type
                ));
            }
            LineContainer::Container { .. } => {
                return Err("BlankLineGroup cannot include nested containers".to_string());
            }
        }
    }

    Ok(ParseNode::new(
        NodeType::BlankLineGroup,
        flat_tokens,
        vec![],
    ))
}
