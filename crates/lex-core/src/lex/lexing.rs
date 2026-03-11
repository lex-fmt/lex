//! Lexer
//!
//!     This module orchestrates the complete tokenization pipeline for the lex format.
//!     Lexing runs transformations over tokens. First we store the core tokens as a TokenStream
//!     for easier handling, then run transformations one by one. Each receiving a TokenStream
//!     and returning a TokenStream.
//!
//! Source Token Preservation
//!
//!     In common, all of these processes store the source tokens in the grouped token under
//!     `source_tokens` field, which preserves information entirely and allows for easy unrolling
//!     at the final stages.
//!
//!     Logo's tokens carry the byte range of their source text. This information will not be
//!     used in the parsing pipeline at all, but has to be perfectly preserved for location
//!     tracking on the tooling that will use the AST. It is critical that this be left as it.
//!     The ast building stage will handle this information, but it's key that no other code
//!     changes it, and at every step its integrity is preserved.
//!
//! The Lexing Pipeline
//!
//!     The pipeline consists of:
//!         1. Core tokenization using logos lexer. See [base_tokenization](base_tokenization).
//!            Each newline (\n) is tokenized as a BlankLine token directly by logos.
//!            Indentation tokens (4 spaces or tab) are emitted for each indentation level.
//!
//!         2. Semantic Indentation transformation. See
//!            [semantic_indentation](transformations::semantic_indentation).
//!            This converts indent tokens into semantic events as indent and dedent.
//!
//!         3. Line Grouping. See [line_grouping](line_grouping).
//!            Here we split tokens by line breaks into groups of tokens. Each group is a
//!            Line token and which category is determined by the tokens inside.
//!
//!     At this point, lexing is complete. We have a TokenStream of Line tokens + indent/dedent
//!     tokens.
//!
//! Indentation Handling
//!
//!     In order to make indented blocks tractable by regular parser combinators libraries,
//!     indentation ultimately gets transformed into semantic indent and dedent tokens, which
//!     map nicely to brace tokens for more standard syntaxes. lex will work the same, but
//!     at this original lexing pass we only do simple 4 spaces / 1 tab substitutions for
//!     indentation blocks. This means that a line that is 2 levels indented will produce
//!     two indent tokens.
//!
//!     The rationale for this approach is:
//!         - This allows us to use a vanilla logos lexer, no custom code.
//!         - This isolates the logic for semantic indent and dedent tokens to a later
//!           transformation step, separate from all other tokenization, which helps a lot.
//!         - At some point in the spec, we will handle blocks much like markdown's fenced
//!           blocks that display non-lex strings. In these cases, while we may parse (for
//!           indentation) the lines, we never want to emit the indent and dedent tokens.
//!           Having this happen in two stages gives us more flexibility on how to handle
//!           these cases.

pub mod base_tokenization;
pub mod common;
pub mod line_classification;
pub mod line_grouping;
pub mod transformations;

pub use base_tokenization::tokenize;
pub use common::{LexError, Lexer, LexerOutput};
// Re-export token types for consumers that still import them from `lexing`
pub use crate::lex::token::{LineContainer, LineToken, LineType, Token};

/// Preprocesses source text to ensure it ends with a newline.
///
/// This is required for proper paragraph parsing at EOF.
/// Returns the original string if it already ends with a newline, or empty string.
/// Otherwise, appends a newline.
pub fn ensure_source_ends_with_newline(source: &str) -> String {
    if !source.is_empty() && !source.ends_with('\n') {
        format!("{source}\n")
    } else {
        source.to_string()
    }
}

pub fn lex(
    tokens: Vec<(Token, std::ops::Range<usize>)>,
) -> Result<Vec<(Token, std::ops::Range<usize>)>, LexError> {
    use crate::lex::lexing::transformations::semantic_indentation::SemanticIndentationMapper;
    let mut mapper = SemanticIndentationMapper::new();
    mapper
        .map(tokens)
        .map_err(|e| LexError::Transformation(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::testing::factories::mk_tokens;

    /// Helper to prepare token stream and call lex pipeline
    fn lex_helper(source: &str) -> Vec<(Token, std::ops::Range<usize>)> {
        let source_with_newline = ensure_source_ends_with_newline(source);
        let token_stream = base_tokenization::tokenize(&source_with_newline);
        lex(token_stream).expect("lex failed")
    }

    #[test]
    fn test_paragraph_pattern() {
        let input = "This is a paragraph.\nIt has multiple lines.";
        let tokens = lex_helper(input);

        // Exact token sequence validation
        // lex() adds a trailing newline and applies full transformations
        assert_eq!(
            tokens,
            mk_tokens(&[
                (Token::Text("This".to_string()), 0, 4),
                (Token::Whitespace(1), 4, 5),
                (Token::Text("is".to_string()), 5, 7),
                (Token::Whitespace(1), 7, 8),
                (Token::Text("a".to_string()), 8, 9),
                (Token::Whitespace(1), 9, 10),
                (Token::Text("paragraph".to_string()), 10, 19),
                (Token::Period, 19, 20),
                (Token::BlankLine(Some("\n".to_string())), 20, 21),
                (Token::Text("It".to_string()), 21, 23),
                (Token::Whitespace(1), 23, 24),
                (Token::Text("has".to_string()), 24, 27),
                (Token::Whitespace(1), 27, 28),
                (Token::Text("multiple".to_string()), 28, 36),
                (Token::Whitespace(1), 36, 37),
                (Token::Text("lines".to_string()), 37, 42),
                (Token::Period, 42, 43),
                (Token::BlankLine(Some("\n".to_string())), 43, 44),
            ])
        );
    }

    #[test]
    fn test_list_pattern() {
        let input = "- First item\n- Second item";
        let tokens = lex_helper(input);

        // Exact token sequence validation
        // lex() adds a trailing newline and applies full transformations
        assert_eq!(
            tokens,
            mk_tokens(&[
                (Token::Dash, 0, 1),
                (Token::Whitespace(1), 1, 2),
                (Token::Text("First".to_string()), 2, 7),
                (Token::Whitespace(1), 7, 8),
                (Token::Text("item".to_string()), 8, 12),
                (Token::BlankLine(Some("\n".to_string())), 12, 13),
                (Token::Dash, 13, 14),
                (Token::Whitespace(1), 14, 15),
                (Token::Text("Second".to_string()), 15, 21),
                (Token::Whitespace(1), 21, 22),
                (Token::Text("item".to_string()), 22, 26),
                (Token::BlankLine(Some("\n".to_string())), 26, 27),
            ])
        );
    }

    #[test]
    fn test_session_pattern() {
        let input = "1. Session Title\n    Content here";
        let tokens = lex_helper(input);

        // Exact token sequence validation
        // lex() transforms Indent -> Indent and adds trailing newline
        assert_eq!(
            tokens,
            mk_tokens(&[
                (Token::Number("1".to_string()), 0, 1),
                (Token::Period, 1, 2),
                (Token::Whitespace(1), 2, 3),
                (Token::Text("Session".to_string()), 3, 10),
                (Token::Whitespace(1), 10, 11),
                (Token::Text("Title".to_string()), 11, 16),
                (Token::BlankLine(Some("\n".to_string())), 16, 17),
                (Token::Indent(vec![(Token::Indentation, 17..21)]), 0, 0),
                (Token::Text("Content".to_string()), 21, 28),
                (Token::Whitespace(1), 28, 29),
                (Token::Text("here".to_string()), 29, 33),
                (Token::BlankLine(Some("\n".to_string())), 33, 34),
                (Token::Dedent(vec![]), 0, 0),
            ])
        );
    }

    #[test]
    fn test_lex_marker_pattern() {
        let input = "Some text :: marker";
        let tokens = lex_helper(input);

        // Exact token sequence validation
        // lex() adds a trailing newline
        assert_eq!(
            tokens,
            mk_tokens(&[
                (Token::Text("Some".to_string()), 0, 4),
                (Token::Whitespace(1), 4, 5),
                (Token::Text("text".to_string()), 5, 9),
                (Token::Whitespace(1), 9, 10),
                (Token::LexMarker, 10, 12),
                (Token::Whitespace(1), 12, 13),
                (Token::Text("marker".to_string()), 13, 19),
                (Token::BlankLine(Some("\n".to_string())), 19, 20),
            ])
        );
    }

    #[test]
    fn test_lex_indented_marker() {
        let input = "  ::";
        let tokens = lex_helper(input);
        let token_kinds: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();
        println!("LEXING Tokens: {token_kinds:?}");
    }

    #[test]
    fn test_mixed_content_pattern() {
        let input = "1. Session\n    - Item 1\n    - Item 2\n\nParagraph after.";
        let tokens = lex_helper(input);

        // Exact token sequence validation
        // lex() transforms Indent -> Indent and consecutive Newlines -> BlankLine
        assert_eq!(
            tokens,
            mk_tokens(&[
                (Token::Number("1".to_string()), 0, 1),
                (Token::Period, 1, 2),
                (Token::Whitespace(1), 2, 3),
                (Token::Text("Session".to_string()), 3, 10),
                (Token::BlankLine(Some("\n".to_string())), 10, 11),
                (Token::Indent(vec![(Token::Indentation, 11..15)]), 0, 0),
                (Token::Dash, 15, 16),
                (Token::Whitespace(1), 16, 17),
                (Token::Text("Item".to_string()), 17, 21),
                (Token::Whitespace(1), 21, 22),
                (Token::Number("1".to_string()), 22, 23),
                (Token::BlankLine(Some("\n".to_string())), 23, 24),
                (Token::Dash, 28, 29),
                (Token::Whitespace(1), 29, 30),
                (Token::Text("Item".to_string()), 30, 34),
                (Token::Whitespace(1), 34, 35),
                (Token::Number("2".to_string()), 35, 36),
                (Token::BlankLine(Some("\n".to_string())), 36, 37),
                (Token::BlankLine(Some("\n".to_string())), 37, 38),
                (Token::Dedent(vec![]), 0, 0),
                (Token::Text("Paragraph".to_string()), 38, 47),
                (Token::Whitespace(1), 47, 48),
                (Token::Text("after".to_string()), 48, 53),
                (Token::Period, 53, 54),
                (Token::BlankLine(Some("\n".to_string())), 54, 55),
            ])
        );
    }

    #[test]
    fn test_consecutive_blank_lines() {
        // Test that consecutive newlines are each tokenized as separate BlankLine tokens
        // In lex semantics, 1+ blank lines mean the same thing at parse time,
        // but we preserve the exact count for round-trip fidelity
        let input = "First\n\n\nSecond"; // 3 consecutive newlines
        let tokens = lex_helper(input);

        // Should produce: First, BlankLine("\n"), BlankLine("\n"), BlankLine("\n"), Second, BlankLine("\n")
        assert_eq!(
            tokens,
            mk_tokens(&[
                (Token::Text("First".to_string()), 0, 5),
                (Token::BlankLine(Some("\n".to_string())), 5, 6),
                (Token::BlankLine(Some("\n".to_string())), 6, 7),
                (Token::BlankLine(Some("\n".to_string())), 7, 8),
                (Token::Text("Second".to_string()), 8, 14),
                (Token::BlankLine(Some("\n".to_string())), 14, 15),
            ])
        );
    }

    #[test]
    fn test_blank_line_round_trip() {
        // Verify that tokenization -> detokenization preserves the source
        use crate::lex::formats::detokenizer::detokenize;

        let inputs = vec![
            "First\nSecond",       // Single newline
            "First\n\nSecond",     // Two newlines (one blank line)
            "First\n\n\nSecond",   // Three newlines (two blank lines)
            "First\n\n\n\nSecond", // Four newlines (three blank lines)
        ];

        for input in inputs {
            let tokens_with_spans = lex_helper(input);
            let tokens: Vec<Token> = tokens_with_spans.into_iter().map(|(t, _)| t).collect();
            let detokenized = detokenize(&tokens);

            // Should preserve the exact number of newlines (plus the trailing one added by lex_helper)
            let expected = ensure_source_ends_with_newline(input);
            assert_eq!(
                detokenized, expected,
                "Round-trip failed for input: {input:?}"
            );
        }
    }
}
