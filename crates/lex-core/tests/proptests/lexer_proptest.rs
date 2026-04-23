//! Property-based tests for the lex lexer
//!
//! These tests use the proptest framework to generate random inputs and verify
//! that the lexer behaves correctly across a wide range of valid lex documents.
//!
//! For sample document snapshot tests, see lexer_samples.rs
//! For exact pattern integration tests, see src/lex/lexing.rs (tests module)

use lex_core::lex::lexing::{lex, Token};
use proptest::prelude::*;

/// Helper to prepare token stream and call lex pipeline
fn lex_helper(source: &str) -> Vec<(Token, std::ops::Range<usize>)> {
    let source_with_newline = lex_core::lex::lexing::ensure_source_ends_with_newline(source);
    let token_stream = lex_core::lex::lexing::base_tokenization::tokenize(&source_with_newline);
    lex(token_stream).expect("lex failed")
}

/// Generate valid lex text content
fn lex_text_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            // Simple text
            "[a-zA-Z0-9]+",
            // Text with spaces
            "[a-zA-Z0-9]+ [a-zA-Z0-9]+",
            // Text with punctuation
            "[a-zA-Z0-9]+[.,!?]",
            // Empty string
            "",
        ],
        1..10,
    )
    .prop_map(|lines| lines.join("\n"))
}

/// Generate valid indentation
fn indentation_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // No indentation
        "",
        // Single level (4 spaces)
        "    ",
        // Multiple levels
        prop::collection::vec("    ", 1..5).prop_map(|levels| levels.join("")),
        // Tab indentation
        "\t",
        // Mixed indentation
        prop::collection::vec(prop_oneof!["    ", "\t"], 1..3).prop_map(|levels| levels.join("")),
    ]
}

/// Generate valid list items
fn list_item_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Plain dash list
        "- [a-zA-Z0-9 ]+",
        // Numbered list
        "[0-9]+\\. [a-zA-Z0-9 ]+",
        // Letter list
        "[a-z]\\. [a-zA-Z0-9 ]+",
        // Parenthetical list
        "\\([0-9]+\\) [a-zA-Z0-9 ]+",
    ]
}

/// Generate valid session titles
fn session_title_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Numbered session
        "[0-9]+\\. [a-zA-Z0-9 ]+",
        // Unnumbered session
        "[a-zA-Z0-9 ]+:",
        // Plain session title
        "[a-zA-Z0-9 ]+",
    ]
}

/// Generate valid lex documents
fn lex_document_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            // Paragraphs
            lex_text_strategy(),
            // List items
            list_item_strategy(),
            // Session titles
            session_title_strategy(),
        ],
        1..20,
    )
    .prop_map(|lines| lines.join("\n"))
}

// Property-based tests using the strategies above
proptest! {
    // Reduce cases to speed up slow tests
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn test_tokenize_never_panics(input in lex_document_strategy()) {
        // The lexer should never panic on any valid lex input
        let _tokens = lex_helper(&input);
    }

    #[test]
    fn test_tokenize_produces_valid_tokens(input in lex_document_strategy()) {
        // All tokens should be valid Token variants
        let tokens = lex_helper(&input);
        for (token, _) in tokens {
            match token {
                Token::LexMarker | Token::Indentation | Token::Indent(_) | Token::Dedent(_) |
                Token::BlankLine(_) | Token::Whitespace(_) | Token::Dash | Token::Period |
                Token::OpenParen | Token::CloseParen | Token::Colon | Token::ExclamationMark |
                Token::QuestionMark | Token::Semicolon | Token::InvertedExclamationMark |
                Token::InvertedQuestionMark | Token::Ellipsis | Token::IdeographicFullStop |
                Token::FullwidthExclamationMark | Token::FullwidthQuestionMark |
                Token::ExclamationQuestionMark | Token::QuestionExclamationMark |
                Token::ArabicQuestionMark | Token::ArabicFullStop | Token::ArabicTripleDot |
                Token::ArabicComma | Token::Danda | Token::DoubleDanda |
                Token::BengaliCurrencyNumeratorFour | Token::EthiopianFullStop |
                Token::ArmenianFullStop | Token::TibetanShad | Token::ThaiFongman |
                Token::MyanmarComma | Token::MyanmarFullStop | Token::Comma |
                Token::Quote | Token::Equals | Token::Number(_) | Token::Text(_) => {
                    // All valid tokens
                }
            }
        }
    }

    #[test]
    fn test_indentation_tokenization(input in indentation_strategy()) {
        // Indentation should produce appropriate indentation-related tokens
        // Note: lex() transforms Indent tokens to Indent/Dedent
        let tokens = lex_helper(&input);

        // After lex(), indentation tokens are transformed:
        // - Indent tokens become Indent tokens (only if line has content after indentation)
        // - Blank lines (indentation followed only by newline) don't produce Indent
        // - At end of file, Dedent tokens close the indentation

        if input.is_empty() {
            // No indentation means no indent/dedent tokens
            let indent_related_count = tokens.iter().filter(|(t, _)| {
                matches!(t, Token::Indent(_) | Token::Dedent(_) | Token::Indentation)
            }).count();
            assert_eq!(indent_related_count, 0);
        } else if !input.chars().any(|c| !c.is_whitespace()) {
            // Pure whitespace (with or without indentation) becomes a blank line
            // Blank lines don't produce Indent tokens
            let indent_related_count = tokens.iter().filter(|(t, _)| {
                matches!(t, Token::Indent(_) | Token::Dedent(_) | Token::Indentation)
            }).count();
            assert_eq!(indent_related_count, 0);
        } else {
            // Input has actual content after indentation
            let indent_level_count = tokens.iter().filter(|(t, _)| matches!(t, Token::Indent(_))).count();

            // Count expected indent levels based on input
            let expected_indents = {
                // Count tabs (each tab = 1 indent)
                let tab_count = input.matches('\t').count();
                // Count groups of 4 spaces (each group = 1 indent)
                let space_count = input.split('\t').map(|s| s.len() / 4).sum::<usize>();
                tab_count + space_count
            };

            assert_eq!(indent_level_count, expected_indents);
        }
    }

    #[test]
    fn test_list_item_tokenization(input in list_item_strategy()) {
        // List items should contain appropriate markers
        let tokens = lex_helper(&input);

        if input.starts_with('-') {
            assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Dash)));
        } else if input.contains('.') && input.chars().next().unwrap().is_ascii_digit() {
            assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Number(_))));
            assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Period)));
        } else if input.starts_with('(') {
            assert!(tokens.iter().any(|(t, _)| matches!(t, Token::OpenParen)));
            assert!(tokens.iter().any(|(t, _)| matches!(t, Token::CloseParen)));
        }
    }

    #[test]
    fn test_session_title_tokenization(input in session_title_strategy()) {
        // Session titles should contain appropriate markers
        let tokens = lex_helper(&input);

        if input.contains(':') {
            assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Colon)));
        } else if input.contains('.') && input.chars().next().unwrap().is_ascii_digit() {
            assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Number(_))));
            assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Period)));
        }
    }

    #[test]
    fn test_multiline_tokenization(input in lex_text_strategy()) {
        // Multiline text should contain blank line tokens
        let tokens = lex_helper(&input);

        if input.contains('\n') {
            assert!(tokens.iter().any(|(t, _)| matches!(t, Token::BlankLine(_))));
        }
    }

    #[test]
    fn test_empty_input_tokenization(input in "") {
        // Empty input should produce no tokens
        let tokens = lex_helper(&input);
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_whitespace_only_tokenization(input in "[ ]{0,10}") {
        // Whitespace-only input should produce appropriate tokens
        let tokens = lex_helper(&input);

        if input.is_empty() {
            assert!(tokens.is_empty());
        } else {
            // Should contain only whitespace-related tokens
            for (token, _) in tokens {
                assert!(token.is_whitespace());
            }
        }
    }
}
