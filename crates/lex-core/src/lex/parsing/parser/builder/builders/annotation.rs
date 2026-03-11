//! Annotation builder
//!
//! Handles construction of annotation nodes in both block and single-line forms.

use super::helpers::{
    extract_annotation_header_tokens, extract_annotation_single_content, extract_line_token,
    AnnotationHeaderAndContent,
};
use crate::lex::parsing::ir::{NodeType, ParseNode};
use crate::lex::token::LineContainer;

/// Type alias for the recursive parser function callback
type ParserFn = dyn Fn(Vec<LineContainer>, &str) -> Result<Vec<ParseNode>, String>;

/// Build an annotation block node
pub(in crate::lex::parsing::parser::builder) fn build_annotation_block(
    tokens: &[LineContainer],
    start_idx: usize,
    content_idx: usize,
    source: &str,
    parse_children: &ParserFn,
) -> Result<ParseNode, String> {
    let start_token = extract_line_token(&tokens[start_idx])?;
    let header_tokens = extract_annotation_header_tokens(start_token)?;

    let children = if let Some(LineContainer::Container { children, .. }) = tokens.get(content_idx)
    {
        parse_children(children.clone(), source)?
    } else {
        vec![]
    };

    Ok(ParseNode::new(
        NodeType::Annotation,
        header_tokens,
        children,
    ))
}

/// Build an annotation single-line node
pub(in crate::lex::parsing::parser::builder) fn build_annotation_single(
    tokens: &[LineContainer],
    start_idx: usize,
) -> Result<ParseNode, String> {
    let start_token = extract_line_token(&tokens[start_idx])?;
    let AnnotationHeaderAndContent {
        header_tokens,
        children,
    } = extract_annotation_single_content(start_token)?;

    Ok(ParseNode::new(
        NodeType::Annotation,
        header_tokens,
        children,
    ))
}
