//! Line Grouping
//!
//!     Groups flat tokens into classified LineTokens. This module contains the core grouping
//!     logic that calls classifiers and creates LineToken structures.
//!
//!     Here we split tokens by line breaks into groups of tokens. Each group is a Line token
//!     and which category is determined by the tokens inside. This is also a fairly simple
//!     transformation.
//!
//!     Each line group is fairly simple and only contains the source tokens it uses. It does
//!     not process their information, and hence we consider this a lexing step as well.
//!
//!     The classification of each line is done by [classify_line_tokens](crate::lex::lexing::line_classification::classify_line_tokens),
//!     which determines the LineType based on the tokens in the line.

use crate::lex::lexing::line_classification::classify_line_tokens;
use crate::lex::token::{LineToken, LineType, Token};
use std::ops::Range as ByteRange;

/// Group flat tokens into classified LineTokens.
///
/// This implements the logic from ToLineTokensMapper:
/// - Groups consecutive tokens into lines (terminated by BlankLine)
/// - Classifies each line by type
/// - Handles structural tokens (Indent, Dedent) specially
pub fn group_into_lines(tokens: Vec<(Token, ByteRange<usize>)>) -> Vec<LineToken> {
    let mut line_tokens = Vec::new();
    let mut current_line = Vec::new();

    for (token, span) in tokens {
        // Indent/Dedent are structural and get their own LineToken
        if matches!(token, Token::Indent(_) | Token::Dedent(_)) {
            // Flush any accumulated line first
            if !current_line.is_empty() {
                line_tokens.push(classify_and_create_line_token(current_line));
            }
            current_line = Vec::new();

            let line_type = if matches!(token, Token::Indent(_)) {
                LineType::Indent
            } else {
                LineType::Dedent
            };

            // Indent token carries its source tokens, Dedent is synthetic
            let (source_tokens, token_spans) = if let Token::Indent(ref sources) = token {
                sources.iter().cloned().unzip()
            } else {
                (vec![token], vec![span])
            };

            line_tokens.push(LineToken {
                source_tokens,
                token_spans,
                line_type,
            });
            continue;
        }

        // Add current token to the line
        current_line.push((token.clone(), span));

        // A BlankLine token signifies the end of a line.
        if matches!(token, Token::BlankLine(_)) {
            line_tokens.push(classify_and_create_line_token(current_line));
            current_line = Vec::new();
        }
    }

    // Handle any remaining tokens (if input doesn't end with newline)
    if !current_line.is_empty() {
        line_tokens.push(classify_and_create_line_token(current_line));
    }

    // Apply dialog line detection

    apply_dialog_detection(line_tokens)
}

/// Classify tokens and create a LineToken with the appropriate LineType.
fn classify_and_create_line_token(token_tuples: Vec<(Token, ByteRange<usize>)>) -> LineToken {
    let (source_tokens, token_spans): (Vec<_>, Vec<_>) = token_tuples.into_iter().unzip();
    let line_type = classify_line_tokens(&source_tokens);

    LineToken {
        source_tokens,
        token_spans,
        line_type,
    }
}

/// Apply dialog line detection logic.
///
/// In the parser, once a dialog line is detected, all subsequent lines
/// are also treated as dialog lines until the end of the block.
fn apply_dialog_detection(mut line_tokens: Vec<LineToken>) -> Vec<LineToken> {
    let mut in_dialog = false;

    for line_token in &mut line_tokens {
        if line_token.line_type == LineType::BlankLine {
            in_dialog = false;
            continue;
        }

        if line_token.line_type != LineType::ListLine
            && line_token.line_type != LineType::DialogLine
        {
            in_dialog = false;
        }

        if in_dialog {
            line_token.line_type = LineType::DialogLine;
        }

        if line_token.line_type == LineType::ListLine {
            let non_whitespace_tokens: Vec<_> = line_token
                .source_tokens
                .iter()
                .filter(|t| !t.is_whitespace())
                .collect();

            if non_whitespace_tokens.len() >= 2 {
                let last_token = non_whitespace_tokens.last().unwrap();
                let second_to_last_token = non_whitespace_tokens[non_whitespace_tokens.len() - 2];

                if last_token.is_end_punctuation() && second_to_last_token.is_end_punctuation() {
                    line_token.line_type = LineType::DialogLine;
                    in_dialog = true;
                }
            }
        }
    }

    line_tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_single_line() {
        let tokens = vec![
            (Token::Text("Hello".to_string()), 0..5),
            (Token::BlankLine(Some("\n".to_string())), 5..6),
        ];

        let line_tokens = group_into_lines(tokens);

        assert_eq!(line_tokens.len(), 1);
        assert_eq!(line_tokens[0].line_type, LineType::ParagraphLine);
        assert_eq!(line_tokens[0].source_tokens.len(), 2);
        assert_eq!(line_tokens[0].token_spans.len(), 2);
    }

    #[test]
    fn test_group_multiple_lines() {
        let tokens = vec![
            (Token::Text("Line1".to_string()), 0..5),
            (Token::BlankLine(Some("\n".to_string())), 5..6),
            (Token::Text("Line2".to_string()), 6..11),
            (Token::BlankLine(Some("\n".to_string())), 11..12),
        ];

        let line_tokens = group_into_lines(tokens);

        assert_eq!(line_tokens.len(), 2);
        assert_eq!(line_tokens[0].line_type, LineType::ParagraphLine);
        assert_eq!(line_tokens[1].line_type, LineType::ParagraphLine);
    }

    #[test]
    fn test_group_with_indent_dedent() {
        let tokens = vec![
            (Token::Text("Title".to_string()), 0..5),
            (Token::Colon, 5..6),
            (Token::BlankLine(Some("\n".to_string())), 6..7),
            (Token::Indent(vec![(Token::Indentation, 7..11)]), 0..0),
            (Token::Text("Content".to_string()), 11..18),
            (Token::BlankLine(Some("\n".to_string())), 18..19),
            (Token::Dedent(vec![]), 0..0),
        ];

        let line_tokens = group_into_lines(tokens);

        assert_eq!(line_tokens.len(), 4);
        assert_eq!(line_tokens[0].line_type, LineType::SubjectLine);
        assert_eq!(line_tokens[1].line_type, LineType::Indent);
        assert_eq!(line_tokens[2].line_type, LineType::ParagraphLine);
        assert_eq!(line_tokens[3].line_type, LineType::Dedent);
    }

    #[test]
    fn test_group_with_blank_line_token() {
        let tokens = vec![
            (Token::Text("Line1".to_string()), 0..5),
            (Token::BlankLine(Some("\n".to_string())), 5..6),
            (Token::BlankLine(Some(" \n".to_string())), 6..8),
            (Token::Text("Line2".to_string()), 8..13),
            (Token::BlankLine(Some("\n".to_string())), 13..14),
        ];

        let line_tokens = group_into_lines(tokens);

        assert_eq!(line_tokens.len(), 3);
        assert_eq!(line_tokens[0].line_type, LineType::ParagraphLine);
        assert_eq!(line_tokens[1].line_type, LineType::BlankLine);
        assert_eq!(line_tokens[2].line_type, LineType::ParagraphLine);
    }

    #[test]
    fn test_dialog_detection() {
        let tokens = vec![
            (Token::Dash, 0..1),
            (Token::Whitespace(1), 1..2),
            (Token::Text("Hello".to_string()), 2..7),
            (Token::Period, 7..8),
            (Token::Period, 8..9),
            (Token::BlankLine(Some("\n".to_string())), 9..10),
            (Token::Dash, 10..11),
            (Token::Whitespace(1), 11..12),
            (Token::Text("World".to_string()), 12..17),
            (Token::BlankLine(Some("\n".to_string())), 17..18),
        ];

        let line_tokens = group_into_lines(tokens);

        assert_eq!(line_tokens.len(), 2);
        assert_eq!(line_tokens[0].line_type, LineType::DialogLine);
        assert_eq!(line_tokens[1].line_type, LineType::DialogLine);
    }

    #[test]
    fn test_preserves_ranges() {
        let tokens = vec![
            (Token::Text("Hello".to_string()), 0..5),
            (Token::Whitespace(1), 5..6),
            (Token::Text("world".to_string()), 6..11),
            (Token::BlankLine(Some("\n".to_string())), 11..12),
        ];

        let line_tokens = group_into_lines(tokens);

        assert_eq!(line_tokens[0].token_spans[0], 0..5);
        assert_eq!(line_tokens[0].token_spans[1], 5..6);
        assert_eq!(line_tokens[0].token_spans[2], 6..11);
        assert_eq!(line_tokens[0].token_spans[3], 11..12);
    }
}
