//! Definition builder
//!
//! Handles construction of definition nodes.

use super::helpers::extract_line_token;
use crate::lex::parsing::ir::{NodeType, ParseNode};
use crate::lex::token::LineContainer;

/// Type alias for the recursive parser function callback
type ParserFn = dyn Fn(Vec<LineContainer>, &str) -> Result<Vec<ParseNode>, String>;

/// Build a definition node
pub(in crate::lex::parsing::parser::builder) fn build_definition(
    tokens: &[LineContainer],
    subject_idx: usize,
    content_idx: usize,
    source: &str,
    parse_children: &ParserFn,
) -> Result<ParseNode, String> {
    let subject_token = extract_line_token(&tokens[subject_idx])?;

    let children = if let Some(LineContainer::Container { children, .. }) = tokens.get(content_idx)
    {
        parse_children(children.clone(), source)?
    } else {
        Vec::new()
    };

    // Filter out Colon, Whitespace, and BlankLine tokens from definition subject
    let subject_tokens: Vec<_> = subject_token
        .source_tokens
        .clone()
        .into_iter()
        .zip(subject_token.token_spans.clone())
        .filter(|(token, _)| {
            !matches!(
                token,
                crate::lex::lexing::Token::Colon
                    | crate::lex::lexing::Token::Whitespace(_)
                    | crate::lex::lexing::Token::BlankLine(_)
            )
        })
        .collect();

    Ok(ParseNode::new(
        NodeType::Definition,
        subject_tokens,
        children,
    ))
}
