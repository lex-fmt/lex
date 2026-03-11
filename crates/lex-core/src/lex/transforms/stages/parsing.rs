//! Parsing stage - produces IR (ParseNode tree)

use crate::lex::parsing::ir::{NodeType, ParseNode};
use crate::lex::token::Token;
use crate::lex::transforms::{Runnable, TransformError};
use std::ops::Range;

/// Parsing stage: tokenizes, lexes, and parses to produce IR (ParseNode tree)
pub struct Parsing;

impl Parsing {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Parsing {
    fn default() -> Self {
        Self::new()
    }
}

impl Runnable<String, ParseNode> for Parsing {
    fn run(&self, input: String) -> Result<ParseNode, TransformError> {
        // Ensure source ends with newline (required for parsing)
        let source = if !input.is_empty() && !input.ends_with('\n') {
            format!("{input}\n")
        } else {
            input
        };

        // Run full lexing pipeline
        let tokens = crate::lex::transforms::standard::LEXING
            .run(source.clone())
            .map_err(|e| TransformError::StageFailed {
                stage: "Lexing".to_string(),
                message: e.to_string(),
            })?;

        // Convert to ParseNode using internal parsing logic
        parse_to_ir(tokens, &source)
    }
}

/// Internal helper to parse tokens into IR
fn parse_to_ir(
    tokens: Vec<(Token, Range<usize>)>,
    source: &str,
) -> Result<ParseNode, TransformError> {
    use crate::lex::lexing::transformations::line_token_grouping::GroupedTokens;
    use crate::lex::lexing::transformations::{DocumentStartMarker, LineTokenGroupingMapper};
    use crate::lex::parsing::parser;
    use crate::lex::token::line::LineToken;
    use crate::lex::token::to_line_container;

    // Apply line token grouping
    let mut mapper = LineTokenGroupingMapper::new();
    let grouped_tokens = mapper.map(tokens);

    // Convert grouped tokens to line tokens
    let line_tokens: Vec<LineToken> = grouped_tokens
        .into_iter()
        .map(GroupedTokens::into_line_token)
        .collect();

    // Inject DocumentStart marker to mark metadata/content boundary
    let line_tokens = DocumentStartMarker::mark(line_tokens);

    // Build LineContainer tree
    let tree = to_line_container::build_line_container(line_tokens);

    // Extract children from root container
    let children = match tree {
        crate::lex::token::line::LineContainer::Container { children, .. } => children,
        crate::lex::token::line::LineContainer::Token(_) => {
            return Err(TransformError::StageFailed {
                stage: "Parsing".to_string(),
                message: "Expected root container, found single token".to_string(),
            })
        }
    };

    // Parse to IR using declarative grammar
    let content = parser::parse_with_declarative_grammar(children, source).map_err(|e| {
        TransformError::StageFailed {
            stage: "Parsing".to_string(),
            message: e.to_string(),
        }
    })?;

    // Create root ParseNode
    Ok(ParseNode::new(NodeType::Document, vec![], content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsing_simple() {
        let source = "Hello world\n".to_string();
        let parsing = Parsing::new();
        let result = parsing.run(source);
        assert!(result.is_ok());
        let node = result.unwrap();
        assert_eq!(node.node_type, NodeType::Document);
    }

    #[test]
    fn test_parsing_with_session() {
        let source = "Session:\n    Content\n".to_string();
        let parsing = Parsing::new();
        let result = parsing.run(source);
        assert!(result.is_ok());
        let node = result.unwrap();
        assert_eq!(node.node_type, NodeType::Document);
        assert!(!node.children.is_empty());
    }
}
