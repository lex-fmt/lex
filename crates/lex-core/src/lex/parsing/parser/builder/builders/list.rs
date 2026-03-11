//! List builder
//!
//! Handles construction of list nodes with their list items.

use super::helpers::extract_line_token;
use crate::lex::parsing::ir::{NodeType, ParseNode};
use crate::lex::token::LineContainer;

/// Type alias for the recursive parser function callback
type ParserFn = dyn Fn(Vec<LineContainer>, &str) -> Result<Vec<ParseNode>, String>;

/// Build a list node with list items
pub(in crate::lex::parsing::parser::builder) fn build_list(
    tokens: &[LineContainer],
    items: &[(usize, Option<usize>)],
    pattern_offset: usize,
    source: &str,
    parse_children: &ParserFn,
) -> Result<ParseNode, String> {
    let mut list_items = Vec::new();

    for (item_idx, content_idx) in items {
        let item_token = extract_line_token(&tokens[pattern_offset + item_idx])?;

        let children = if let Some(content_idx_val) = content_idx {
            if let Some(LineContainer::Container { children, .. }) =
                tokens.get(pattern_offset + content_idx_val)
            {
                parse_children(children.clone(), source)?
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let list_item = ParseNode::new(
            NodeType::ListItem,
            item_token
                .source_tokens
                .clone()
                .into_iter()
                .zip(item_token.token_spans.clone())
                .collect(),
            children,
        );
        list_items.push(list_item);
    }

    Ok(ParseNode::new(NodeType::List, vec![], list_items))
}
