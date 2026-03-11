//! Semantic indentation mapper for TokenStream pipeline
//!
//!     This mapper transforms raw Indentation tokens into semantic Indent and Dedent token
//!     pairs based on indentation level changes between lines.
//!
//!     The logos lexer will produce indentation tokens, that is grouping several spaces or
//!     tabs into a single token. However, indentation tokens per se are not useful. We don't
//!     want to know how many spaces per line there are, but we want to know about indentation
//!     levels and what's inside each one. For this, we want to track indent and dedent events,
//!     which lets us neatly tell levels and their content.
//!
//!     This transformation is a stateful machine that tracks changes in indentation levels and
//!     emits indent and dedent events. In itself, this is trivial, and how most indentation
//!     handling is done. At this point, indent/dedent could be replaced for open/close braces
//!     in more c-style languages with the same effect.
//!
//!     Indent tokens store the original indentation token, while dedent tokens are synthetic
//!     and have no source tokens of their own.
//!
//! Algorithm
//!
//!     1. Track the current indentation level (number of Indentation tokens)
//!     2. For each line, count the Indentation tokens at the beginning
//!     3. Compare with the previous line's indentation level:
//!        - If greater: emit Indent tokens for each additional level
//!        - If less: emit Dedent tokens for each reduced level
//!        - If equal: no indentation tokens needed
//!     4. Replace Indentation tokens with the appropriate semantic tokens
//!     5. Always add final Dedent tokens to close the document structure
use crate::lex::token::Token;
use std::ops::Range as ByteRange;
#[derive(Debug, Clone, PartialEq)]
pub enum TransformationError {
    Error(String),
}
impl std::fmt::Display for TransformationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransformationError::Error(msg) => write!(f, "Transformation error: {msg}"),
        }
    }
}

/// A mapper that converts raw Indentation tokens to semantic Indent/Dedent pairs.
///
/// This transformation only operates on flat token streams and preserves all
/// token ranges exactly as they appear in the source.
pub struct SemanticIndentationMapper;

impl SemanticIndentationMapper {
    /// Create a new SemanticIndentationMapper.
    pub fn new() -> Self {
        SemanticIndentationMapper
    }
}

impl Default for SemanticIndentationMapper {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the start of the current line, going backwards from the given position
fn find_line_start(tokens: &[Token], mut pos: usize) -> usize {
    // Go backwards to find the previous newline or start of document
    while pos > 0 {
        pos -= 1;
        if matches!(tokens[pos], Token::BlankLine(_)) {
            return pos + 1;
        }
    }
    0
}

/// Check if a line is blank (only contains indentation and newline)
fn is_line_blank(tokens: &[Token], line_start: usize) -> bool {
    let mut i = line_start;

    // Skip any indentation tokens at the beginning
    while i < tokens.len() && matches!(tokens[i], Token::Indentation) {
        i += 1;
    }

    // Check if the next token is a newline (or end of file)
    i >= tokens.len() || matches!(tokens[i], Token::BlankLine(_))
}

/// Count consecutive Indentation tokens at the beginning of a line
fn count_line_indent_steps(tokens: &[Token], start: usize) -> usize {
    let mut count = 0;
    let mut i = start;

    while i < tokens.len() && matches!(tokens[i], Token::Indentation) {
        count += 1;
        i += 1;
    }

    count
}

impl SemanticIndentationMapper {
    /// Transforms flat tokens by converting Indentation tokens into semantic Indent/Dedent pairs.
    ///
    /// # Algorithm Overview
    ///
    /// This transformation processes tokens line-by-line:
    /// 1. For each line, count leading Indentation tokens to determine its indent level
    /// 2. Compare with previous line's level to emit Indent/Dedent tokens
    /// 3. Skip the consumed Indentation tokens and emit remaining line content
    /// 4. Preserve all original token locations for AST building
    ///
    /// # Example
    ///
    /// Input:  `[Text("a"), BlankLine, Indentation, Text("b"), BlankLine]`
    /// Output: `[Text("a"), BlankLine, Indent([Indentation]), Text("b"), BlankLine, Dedent([])]`
    pub fn map(
        &mut self,
        tokens: Vec<(Token, ByteRange<usize>)>,
    ) -> Result<Vec<(Token, ByteRange<usize>)>, TransformationError> {
        // Extract just the tokens for processing
        let token_kinds: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();

        let mut result = Vec::new();
        let mut current_level = 0; // Track current indentation level
        let mut i = 0;

        // Main loop: Process tokens line-by-line
        while i < tokens.len() {
            // Find the start of the current line
            let line_start = find_line_start(&token_kinds, i);

            // Count Indentation tokens at the beginning of this line
            let line_indent_level = count_line_indent_steps(&token_kinds, line_start);

            // Check if this line is blank (only contains indentation and newline)
            let is_blank_line = is_line_blank(&token_kinds, line_start);

            // Skip blank lines - they don't affect indentation level
            if is_blank_line {
                let mut j = line_start;
                while j < token_kinds.len() && !matches!(token_kinds[j], Token::BlankLine(_)) {
                    j += 1;
                }
                if j < token_kinds.len() && matches!(token_kinds[j], Token::BlankLine(_)) {
                    // Preserve the newline location
                    result.push((token_kinds[j].clone(), tokens[j].1.clone()));
                    j += 1;
                }
                i = j;
                continue;
            }

            // Calculate the target indentation level for this line
            let target_level = line_indent_level;

            // Stage 1: Emit Indent or Dedent tokens based on level change
            // This is where we transform indentation changes into semantic structure
            match target_level.cmp(&current_level) {
                std::cmp::Ordering::Greater => {
                    // Indenting: emit one Indent token per level increase
                    // Each Indent stores the original Indentation token it replaces for source fidelity
                    let indent_start_idx = line_start;
                    for level_idx in 0..(target_level - current_level) {
                        let indent_token_idx = indent_start_idx + current_level + level_idx;
                        let source_tokens = if indent_token_idx < token_kinds.len()
                            && matches!(token_kinds[indent_token_idx], Token::Indentation)
                        {
                            // Store the original (Token::Indentation, Range<usize>) pair
                            vec![tokens[indent_token_idx].clone()]
                        } else {
                            // No corresponding Indentation token (shouldn't happen in well-formed input)
                            vec![]
                        };
                        // Placeholder span 0..0 - will never be used, AST construction unrolls source_tokens
                        result.push((Token::Indent(source_tokens), 0..0));
                    }
                }
                std::cmp::Ordering::Less => {
                    // Dedenting: emit one Dedent token per level decrease
                    // Dedent tokens are purely structural (don't replace any source tokens)
                    for _ in 0..(current_level - target_level) {
                        // Placeholder span 0..0 - will never be used
                        result.push((Token::Dedent(vec![]), 0..0));
                    }
                }
                std::cmp::Ordering::Equal => {
                    // Same level: no indentation tokens needed
                }
            }

            // Update current level to match this line
            current_level = target_level;

            // Stage 2: Skip the Indentation tokens we already processed
            // These have been transformed into Indent tokens above
            let mut j = line_start;
            for _ in 0..line_indent_level {
                if j < token_kinds.len() && matches!(token_kinds[j], Token::Indentation) {
                    j += 1;
                }
            }

            // Stage 3: Emit the rest of the line content (everything except BlankLine)
            // Preserve all tokens with their original source locations
            while j < token_kinds.len() && !matches!(token_kinds[j], Token::BlankLine(_)) {
                result.push((token_kinds[j].clone(), tokens[j].1.clone()));
                j += 1;
            }

            // Stage 4: Emit the BlankLine token (end of line marker)
            if j < token_kinds.len() && matches!(token_kinds[j], Token::BlankLine(_)) {
                result.push((token_kinds[j].clone(), tokens[j].1.clone()));
                j += 1;
            }

            // Move to next line
            i = j;
        }

        // Final cleanup: Add Dedent tokens to close all remaining indentation levels
        // This ensures the document structure is properly closed (like closing braces)
        for _ in 0..current_level {
            result.push((Token::Dedent(vec![]), 0..0));
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::testing::factories::{mk_token, Tokens};
    use crate::lex::token::Token;

    fn with_loc(tokens: Vec<Token>) -> Tokens {
        tokens
            .into_iter()
            .enumerate()
            .map(|(idx, token)| mk_token(token, idx, idx + 1))
            .collect()
    }

    fn strip_loc(pairs: Tokens) -> Vec<Token> {
        pairs
            .into_iter()
            .map(|(t, _)| {
                // Normalize source_tokens to empty for test comparison
                match t {
                    Token::Indent(_) => Token::Indent(vec![]),
                    Token::Dedent(_) => Token::Dedent(vec![]),
                    Token::BlankLine(_) => Token::BlankLine(Some("\n".to_string())),
                    other => other,
                }
            })
            .collect()
    }

    #[test]
    fn test_simple_indentation() {
        let input = vec![
            Token::Text("a".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Dash,
            Token::BlankLine(Some("\n".to_string())),
        ];

        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(with_loc(input)).unwrap();
        let stripped = strip_loc(tokens);
        assert_eq!(
            stripped,
            vec![
                Token::Text("a".to_string()),
                Token::BlankLine(Some("\n".to_string())),
                Token::Indent(vec![]),
                Token::Dash,
                Token::BlankLine(Some("\n".to_string())),
                Token::Dedent(vec![]),
            ]
        );
    }

    #[test]
    fn test_multiple_indent_levels() {
        let input = vec![
            Token::Text("a".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Indentation,
            Token::Dash,
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Text("b".to_string()),
            Token::BlankLine(Some("\n".to_string())),
        ];

        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(with_loc(input)).unwrap();
        let stripped = strip_loc(tokens);
        assert_eq!(
            stripped,
            vec![
                Token::Text("a".to_string()),
                Token::BlankLine(Some("\n".to_string())),
                Token::Indent(vec![]),
                Token::Indent(vec![]),
                Token::Dash,
                Token::BlankLine(Some("\n".to_string())),
                Token::Dedent(vec![]),
                Token::Text("b".to_string()),
                Token::BlankLine(Some("\n".to_string())),
                Token::Dedent(vec![]),
            ]
        );
    }

    #[test]
    fn test_no_indentation() {
        let input = vec![
            Token::Text("a".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Text("b".to_string()),
            Token::BlankLine(Some("\n".to_string())),
        ];

        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(with_loc(input.clone())).unwrap();
        let stripped = strip_loc(tokens);
        assert_eq!(stripped, input);
    }

    #[test]
    fn test_empty_input() {
        let input = vec![];
        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(with_loc(input)).unwrap();
        let stripped = strip_loc(tokens);
        assert_eq!(stripped, vec![]);
    }

    #[test]
    fn test_single_line() {
        let input = vec![Token::Text("a".to_string())];
        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(with_loc(input)).unwrap();
        let stripped = strip_loc(tokens);
        assert_eq!(stripped, vec![Token::Text("a".to_string())]);
    }

    #[test]
    fn test_blank_lines() {
        let input = vec![
            Token::Text("a".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Dash,
            Token::BlankLine(Some("\n".to_string())),
            Token::BlankLine(Some("\n".to_string())), // blank line
            Token::Dash,
            Token::BlankLine(Some("\n".to_string())),
        ];

        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(with_loc(input)).unwrap();
        let stripped = strip_loc(tokens);
        assert_eq!(
            stripped,
            vec![
                Token::Text("a".to_string()),
                Token::BlankLine(Some("\n".to_string())),
                Token::Indent(vec![]),
                Token::Dash,
                Token::BlankLine(Some("\n".to_string())),
                Token::BlankLine(Some("\n".to_string())),
                Token::Dedent(vec![]),
                Token::Dash,
                Token::BlankLine(Some("\n".to_string())),
            ]
        );
    }

    #[test]
    fn test_blank_lines_with_indentation() {
        let input = vec![
            Token::Text("a".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Dash,
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::BlankLine(Some("\n".to_string())), // blank line with indentation
            Token::Dash,
            Token::BlankLine(Some("\n".to_string())),
        ];

        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(with_loc(input)).unwrap();
        let stripped = strip_loc(tokens);
        assert_eq!(
            stripped,
            vec![
                Token::Text("a".to_string()),
                Token::BlankLine(Some("\n".to_string())),
                Token::Indent(vec![]),
                Token::Dash,
                Token::BlankLine(Some("\n".to_string())),
                Token::BlankLine(Some("\n".to_string())),
                Token::Dedent(vec![]),
                Token::Dash,
                Token::BlankLine(Some("\n".to_string())),
            ]
        );
    }

    #[test]
    fn test_file_ending_while_indented() {
        let input = vec![
            Token::Text("a".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Dash,
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Indentation,
            Token::Text("b".to_string()),
        ];

        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(with_loc(input)).unwrap();
        let stripped = strip_loc(tokens);
        assert_eq!(
            stripped,
            vec![
                Token::Text("a".to_string()),
                Token::BlankLine(Some("\n".to_string())),
                Token::Indent(vec![]),
                Token::Dash,
                Token::BlankLine(Some("\n".to_string())),
                Token::Indent(vec![]),
                Token::Text("b".to_string()),
                Token::Dedent(vec![]),
                Token::Dedent(vec![]),
            ]
        );
    }

    #[test]
    fn test_sharp_drop_in_indentation() {
        let input = vec![
            Token::Text("a".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Indentation,
            Token::Indentation,
            Token::Dash,
            Token::BlankLine(Some("\n".to_string())),
            Token::Text("b".to_string()),
            Token::BlankLine(Some("\n".to_string())),
        ];

        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(with_loc(input)).unwrap();
        let stripped = strip_loc(tokens);
        assert_eq!(
            stripped,
            vec![
                Token::Text("a".to_string()),
                Token::BlankLine(Some("\n".to_string())),
                Token::Indent(vec![]),
                Token::Indent(vec![]),
                Token::Indent(vec![]),
                Token::Dash,
                Token::BlankLine(Some("\n".to_string())),
                Token::Dedent(vec![]),
                Token::Dedent(vec![]),
                Token::Dedent(vec![]),
                Token::Text("b".to_string()),
                Token::BlankLine(Some("\n".to_string())),
            ]
        );
    }

    #[test]
    fn test_count_line_indent_steps() {
        let tokens = vec![
            Token::Indentation,
            Token::Indentation,
            Token::Dash,
            Token::Text("a".to_string()),
        ];

        assert_eq!(count_line_indent_steps(&tokens, 0), 2);
        assert_eq!(count_line_indent_steps(&tokens, 2), 0);
    }

    #[test]
    fn test_find_line_start() {
        let tokens = vec![
            Token::Text("a".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Dash,
        ];

        assert_eq!(find_line_start(&tokens, 0), 0);
        assert_eq!(find_line_start(&tokens, 2), 2);
        assert_eq!(find_line_start(&tokens, 3), 2);
    }

    #[test]
    fn test_source_tokens_captured_in_indent() {
        // Verify that Indent tokens capture their source Indentation tokens (Immutable Log principle)
        let input: Tokens = vec![
            mk_token(Token::Text("a".to_string()), 0, 1),
            mk_token(Token::BlankLine(Some("\n".to_string())), 1, 2),
            mk_token(Token::Indentation, 2, 6), // 4 spaces
            mk_token(Token::Text("b".to_string()), 6, 7),
        ];

        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(input).unwrap();
        // Find the Indent token
        let indent_pos = tokens
            .iter()
            .position(|(t, _)| matches!(t, Token::Indent(_)))
            .expect("Should have Indent token");

        // Verify source_tokens are captured correctly
        if let Token::Indent(source_tokens) = &tokens[indent_pos].0 {
            assert_eq!(
                source_tokens.len(),
                1,
                "Indent should capture 1 source Indentation token"
            );
            assert_eq!(source_tokens[0].0, Token::Indentation);
            assert_eq!(
                source_tokens[0].1,
                2..6,
                "Source token should have correct range"
            );
        } else {
            panic!("Expected Indent token");
        }

        // Verify placeholder span is used
        assert_eq!(tokens[indent_pos].1, 0..0, "Indent uses placeholder span");
    }

    #[test]
    fn test_source_tokens_captured_in_multiple_indents() {
        // Verify that multiple Indent tokens each capture their respective source tokens
        let input: Tokens = vec![
            mk_token(Token::Text("a".to_string()), 0, 1),
            mk_token(Token::BlankLine(Some("\n".to_string())), 1, 2),
            mk_token(Token::Indentation, 2, 6), // First indent level
            mk_token(Token::Indentation, 6, 10), // Second indent level
            mk_token(Token::Text("b".to_string()), 10, 11),
        ];

        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(input).unwrap();
        // Find Indent tokens
        let indent_positions: Vec<_> = tokens
            .iter()
            .enumerate()
            .filter_map(|(i, (t, _))| {
                if matches!(t, Token::Indent(_)) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(indent_positions.len(), 2, "Should have 2 Indent tokens");

        // Verify first Indent captures first Indentation token
        if let Token::Indent(source_tokens) = &tokens[indent_positions[0]].0 {
            assert_eq!(source_tokens.len(), 1);
            assert_eq!(source_tokens[0].1, 2..6, "First Indent source range");
        }

        // Verify second Indent captures second Indentation token
        if let Token::Indent(source_tokens) = &tokens[indent_positions[1]].0 {
            assert_eq!(source_tokens.len(), 1);
            assert_eq!(source_tokens[0].1, 6..10, "Second Indent source range");
        }
    }

    // Additional comprehensive tests for edge cases
    #[test]
    fn test_blank_line_with_spaces_does_not_dedent() {
        let input = vec![
            Token::Indentation,
            Token::Indentation,
            Token::Text("Foo".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Indentation,
            Token::Text("Foo2".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::BlankLine(Some("\n".to_string())),
            Token::Indentation,
            Token::Indentation,
            Token::Text("Bar".to_string()),
            Token::BlankLine(Some("\n".to_string())),
        ];

        let mut mapper = SemanticIndentationMapper::new();
        let tokens = mapper.map(with_loc(input)).unwrap();
        let stripped = strip_loc(tokens);
        assert_eq!(
            stripped,
            vec![
                Token::Indent(vec![]),
                Token::Indent(vec![]),
                Token::Text("Foo".to_string()),
                Token::BlankLine(Some("\n".to_string())),
                Token::Text("Foo2".to_string()),
                Token::BlankLine(Some("\n".to_string())),
                Token::BlankLine(Some("\n".to_string())), // blank line preserved
                Token::Text("Bar".to_string()),
                Token::BlankLine(Some("\n".to_string())),
                Token::Dedent(vec![]),
                Token::Dedent(vec![]),
            ],
            "Blank lines with only spaces should NOT produce dedent/indent tokens"
        );
    }
}
