//! Line Token Grouping Transformation
//!
//! Groups flat tokens into line-based groups with classification.
//! This transformation:
//! - Groups consecutive tokens into lines (delimited by Newline)
//! - Classifies each line by type (SubjectLine, ListLine, etc.)
//! - Handles structural tokens (Indent, Dedent, BlankLine) specially
//! - Applies dialog line detection
//!
//! Converts: TokenStream::Flat → TokenStream::Grouped

use crate::lex::lexing::line_grouping::group_into_lines;
use crate::lex::token::{LineToken, LineType, Token};
use std::ops::Range as ByteRange;

/// Transformation that groups flat tokens into line-based groups.
pub struct LineTokenGroupingMapper;

impl LineTokenGroupingMapper {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LineTokenGroupingMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupedTokens {
    pub source_tokens: Vec<(Token, ByteRange<usize>)>,
    pub group_type: GroupType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GroupType {
    Line(LineType),
}

impl LineTokenGroupingMapper {
    pub fn map(&mut self, tokens: Vec<(Token, ByteRange<usize>)>) -> Vec<GroupedTokens> {
        // Group tokens into LineTokens
        let line_tokens = group_into_lines(tokens);

        // Convert LineTokens to GroupedTokens
        let grouped_tokens: Vec<GroupedTokens> = line_tokens
            .into_iter()
            .map(|line_token| GroupedTokens {
                source_tokens: line_token.source_token_pairs(),
                group_type: GroupType::Line(line_token.line_type),
            })
            .collect();

        grouped_tokens
    }
}

impl GroupedTokens {
    /// Convert grouped tokens into the canonical [`LineToken`] representation.
    pub fn into_line_token(self) -> LineToken {
        let (source_tokens, token_spans): (Vec<_>, Vec<_>) = self.source_tokens.into_iter().unzip();
        let GroupType::Line(line_type) = self.group_type;

        LineToken {
            source_tokens,
            token_spans,
            line_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mapper_integration() {
        let tokens = vec![
            (Token::Text("Title".to_string()), 0..5),
            (Token::Colon, 5..6),
            (Token::BlankLine(Some("\n".to_string())), 6..7),
        ];

        let mut mapper = LineTokenGroupingMapper::new();
        let groups = mapper.map(tokens);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].source_tokens.len(), 3);
        match groups[0].group_type {
            GroupType::Line(LineType::SubjectLine) => {}
            _ => panic!("Expected SubjectLine"),
        }
    }
}
