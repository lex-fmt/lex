//! Tree Builder - Builds hierarchical LineContainer tree from LineTokens
//!
//!     This module builds a hierarchical tree structure from a flat list of classified LineTokens.
//!     It uses a recursive descent approach to handle indentation.
//!
//!     This is a key step in the parser design. After lexing has produced line tokens with indent
//!     and dedent markers, we group line tokens into a tree of LineContainers. What this gives us
//!     is the ability to parse each level in isolation. Because we don't need to know what a
//!     LineContainer has, but only that it is a line container, we can parse each level with a
//!     regular regex. We simply print token names and match the grammar patterns against them.
//!
//!     This transformation is part of the lexing pipeline (steps 2-4 in the parser design), where
//!     we group tokens into lines, build a tree of line groups reflecting the nesting structure,
//!     and inject context information into each group allowing parsing to only read each level's
//!     lines.
//!
//!     The key insight is that parsing only needs to read each level's lines, which can include a
//!     LineContainer (that is, there is child content there), with no tree traversal needed.
//!     Parsing is done declaratively by processing the grammar patterns (regular strings) through
//!     rust's regex engine. Put another way, once tokens are grouped into a tree of lines, parsing
//!     can be done in a regular single pass.
//!
//!     See the [parser design](crate::lex) module for the complete architecture overview.

use crate::lex::token::{LineContainer, LineToken, LineType};
use std::iter::Peekable;

/// Build a LineContainer tree from already-grouped LineTokens.
///
/// This is the main entry point that builds a hierarchical structure from
/// line tokens that have already been grouped and classified by the
/// lexing pipeline.
///
/// # Arguments
///
/// * `line_tokens` - Vector of LineTokens from the lexing pipeline
///
/// # Returns
///
/// A LineContainer tree ready for the line-based parser
pub fn build_line_container(line_tokens: Vec<LineToken>) -> LineContainer {
    let mut tokens_iter = line_tokens.into_iter().peekable();
    let children = build_recursive(&mut tokens_iter);
    LineContainer::Container { children }
}

/// Recursively build a hierarchy of LineContainers from a stream of LineTokens.
///
/// This function processes tokens at the current indentation level. When it encounters
/// an `Indent`, it recursively calls itself to build a nested `Container`. It stops
/// processing the current level when it sees a `Dedent` (which belongs to the parent
/// level) or when the token stream is exhausted.
fn build_recursive<I>(tokens: &mut Peekable<I>) -> Vec<LineContainer>
where
    I: Iterator<Item = LineToken>,
{
    let mut children = Vec::new();

    while let Some(token) = tokens.peek() {
        match token.line_type {
            LineType::Indent => {
                tokens.next(); // Consume Indent token
                let indented_children = build_recursive(tokens);
                children.push(LineContainer::Container {
                    children: indented_children,
                });
            }
            LineType::Dedent => {
                // This Dedent signifies the end of the current level.
                // Consume it and return to the parent level.
                tokens.next();
                return children;
            }
            _ => {
                // Regular token, consume and add to the current level's children.
                if let Some(t) = tokens.next() {
                    children.push(LineContainer::Token(t));
                }
            }
        }
    }

    children
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::token::Token;

    #[allow(clippy::single_range_in_vec_init)]
    #[test]
    fn test_build_hierarchy_simple() {
        // Test with already-grouped LineTokens
        let line_tokens = vec![LineToken {
            source_tokens: vec![
                Token::Text("Hello".to_string()),
                Token::Whitespace(1),
                Token::Text("world".to_string()),
                Token::BlankLine(Some("\n".to_string())),
            ],
            token_spans: vec![0..5, 5..6, 6..11, 11..12],
            line_type: LineType::ParagraphLine,
        }];

        let container = build_line_container(line_tokens);

        match container {
            LineContainer::Container { children } => {
                assert_eq!(children.len(), 1);
                match &children[0] {
                    LineContainer::Token(line_token) => {
                        assert_eq!(line_token.line_type, LineType::ParagraphLine);
                        assert_eq!(line_token.source_tokens.len(), 4);
                    }
                    _ => panic!("Expected Token"),
                }
            }
            _ => panic!("Expected Container at root"),
        }
    }

    #[allow(clippy::single_range_in_vec_init)]
    #[test]
    fn test_build_hierarchy_with_indentation() {
        // Test hierarchy building with Indent/Dedent markers
        let line_tokens = vec![
            LineToken {
                source_tokens: vec![
                    Token::Text("Title".to_string()),
                    Token::Colon,
                    Token::BlankLine(Some("\n".to_string())),
                ],
                token_spans: vec![0..5, 5..6, 6..7],
                line_type: LineType::SubjectLine,
            },
            LineToken {
                source_tokens: vec![Token::Indentation],
                token_spans: vec![7..11],
                line_type: LineType::Indent,
            },
            LineToken {
                source_tokens: vec![
                    Token::Text("Content".to_string()),
                    Token::BlankLine(Some("\n".to_string())),
                ],
                token_spans: vec![11..18, 18..19],
                line_type: LineType::ParagraphLine,
            },
            LineToken {
                source_tokens: vec![Token::Dedent(vec![])],
                token_spans: vec![0..0],
                line_type: LineType::Dedent,
            },
        ];

        let container = build_line_container(line_tokens);

        // Expected structure: [Token(Title), Container([Token(Content)])]
        match container {
            LineContainer::Container { children } => {
                assert_eq!(
                    children.len(),
                    2,
                    "Should have two items at the root: the title token and the content container"
                );

                // First child should be the title token
                match &children[0] {
                    LineContainer::Token(line_token) => {
                        assert_eq!(line_token.line_type, LineType::SubjectLine);
                    }
                    _ => panic!("Expected Token for title"),
                }

                // Second child should be the container for indented content
                match &children[1] {
                    LineContainer::Container {
                        children: nested_children,
                    } => {
                        assert_eq!(
                            nested_children.len(),
                            1,
                            "Nested container should have one item"
                        );
                        match &nested_children[0] {
                            LineContainer::Token(line_token) => {
                                assert_eq!(line_token.line_type, LineType::ParagraphLine);
                            }
                            _ => panic!("Expected Token for content"),
                        }
                    }
                    _ => panic!("Expected Container for indented content"),
                }
            }
            _ => panic!("Expected Container at root"),
        }
    }
}
