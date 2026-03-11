//! Detokenizer for the lex format
//!
//! This module provides functionality to convert a stream of tokens back into a string.
//!
//! Unlike other formatters in this module which work on AST `Document` objects,
//! the detokenizer works at the token level, converting token streams back to
//! source text. This is useful for:
//!
//! - Round-trip testing (source -> tokens -> source)
//! - Token-level transformations that preserve the original format
//! - Debugging and visualization of token streams
//!
//! The detokenizer handles:
//! - Raw tokens (basic token -> string conversion)
//! - Semantic indentation tokens (Indent/Dedent) for proper formatting

use super::core::Token;

/// Trait for converting a token to its string representation
pub trait ToLexString {
    fn to_lex_string(&self) -> String;
}

impl ToLexString for Token {
    fn to_lex_string(&self) -> String {
        match self {
            Token::LexMarker => "::".to_string(),
            Token::Indentation => "    ".to_string(),
            Token::Whitespace(count) => " ".repeat(*count),
            // BlankLine should always contain the newline character(s) for round-trip fidelity.
            // The logos regex always produces Some(...), but we default to "\n" for safety.
            Token::BlankLine(s) => s.as_deref().unwrap_or("\n").to_string(),
            Token::Dash => "-".to_string(),
            Token::Period => ".".to_string(),
            Token::OpenParen => "(".to_string(),
            Token::CloseParen => ")".to_string(),
            Token::Colon => ":".to_string(),
            Token::ExclamationMark => "!".to_string(),
            Token::QuestionMark => "?".to_string(),
            Token::Semicolon => ";".to_string(),
            Token::InvertedExclamationMark => "¡".to_string(),
            Token::InvertedQuestionMark => "¿".to_string(),
            Token::Ellipsis => "…".to_string(),
            Token::IdeographicFullStop => "。".to_string(),
            Token::FullwidthExclamationMark => "！".to_string(),
            Token::FullwidthQuestionMark => "？".to_string(),
            Token::ExclamationQuestionMark => "⁉".to_string(),
            Token::QuestionExclamationMark => "⁈".to_string(),
            Token::ArabicQuestionMark => "؟".to_string(),
            Token::ArabicFullStop => "۔".to_string(),
            Token::ArabicTripleDot => "؍".to_string(),
            Token::ArabicComma => "،".to_string(),
            Token::Danda => "।".to_string(),
            Token::DoubleDanda => "॥".to_string(),
            Token::BengaliCurrencyNumeratorFour => "৷".to_string(),
            Token::EthiopianFullStop => "።".to_string(),
            Token::ArmenianFullStop => "։".to_string(),
            Token::TibetanShad => "།".to_string(),
            Token::ThaiFongman => "๏".to_string(),
            Token::MyanmarComma => "၊".to_string(),
            Token::MyanmarFullStop => "။".to_string(),
            Token::Comma => ",".to_string(),
            Token::Quote => "\"".to_string(),
            Token::Equals => "=".to_string(),
            Token::Number(s) => s.clone(),
            Token::Text(s) => s.clone(),
            // The following tokens are synthetic and should not be part of the detokenized output
            Token::Indent(_) | Token::Dedent(_) => String::new(),
        }
    }
}

/// Detokenize a stream of tokens into a string
///
/// This function converts a sequence of tokens back to source text,
/// handling semantic indentation (Indent/Dedent tokens) to reconstruct
/// the proper indentation structure.
///
/// # Arguments
///
/// * `tokens` - Slice of tokens to detokenize
///
/// # Returns
///
/// A string representation of the tokens with proper indentation
///
/// # Examples
///
/// ```ignore
/// use lex::lex::formats::detokenizer::detokenize;
/// use lex::lex::lexing::tokenize;
///
/// let source = "Hello world";
/// let tokens: Vec<_> = tokenize(source).into_iter().map(|(t, _)| t).collect();
/// let result = detokenize(&tokens);
/// assert_eq!(result, source);
/// ```
pub fn detokenize(tokens: &[Token]) -> String {
    let mut result = String::new();
    let mut indent_level = 0;

    for token in tokens {
        match token {
            Token::Indent(_) => indent_level += 1,
            Token::Dedent(_) => indent_level -= 1,
            Token::BlankLine(_) => {
                result.push_str(&token.to_lex_string());
            }
            _ => {
                if result.ends_with('\n') || result.is_empty() {
                    for _ in 0..indent_level {
                        result.push_str("    ");
                    }
                }
                result.push_str(&token.to_lex_string());
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::lexing::{ensure_source_ends_with_newline, lex, tokenize};
    use crate::lex::testing::lexplore::specfile_finder::{self, DocumentType, ElementType};
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn to_lex_string_maps_every_literal_token() {
        let cases: Vec<(Token, &str)> = vec![
            (Token::LexMarker, "::"),
            (Token::Indentation, "    "),
            (Token::Whitespace(1), " "),
            (Token::BlankLine(Some("\n".to_string())), "\n"),
            (Token::Dash, "-"),
            (Token::Period, "."),
            (Token::OpenParen, "("),
            (Token::CloseParen, ")"),
            (Token::Colon, ":"),
            (Token::ExclamationMark, "!"),
            (Token::QuestionMark, "?"),
            (Token::Semicolon, ";"),
            (Token::InvertedExclamationMark, "¡"),
            (Token::InvertedQuestionMark, "¿"),
            (Token::Ellipsis, "…"),
            (Token::IdeographicFullStop, "。"),
            (Token::FullwidthExclamationMark, "！"),
            (Token::FullwidthQuestionMark, "？"),
            (Token::ExclamationQuestionMark, "⁉"),
            (Token::QuestionExclamationMark, "⁈"),
            (Token::ArabicQuestionMark, "؟"),
            (Token::ArabicFullStop, "۔"),
            (Token::ArabicTripleDot, "؍"),
            (Token::ArabicComma, "،"),
            (Token::Danda, "।"),
            (Token::DoubleDanda, "॥"),
            (Token::BengaliCurrencyNumeratorFour, "৷"),
            (Token::EthiopianFullStop, "።"),
            (Token::ArmenianFullStop, "։"),
            (Token::TibetanShad, "།"),
            (Token::ThaiFongman, "๏"),
            (Token::MyanmarComma, "၊"),
            (Token::MyanmarFullStop, "။"),
            (Token::Comma, ","),
            (Token::Quote, "\""),
            (Token::Equals, "="),
            (Token::Number("42".to_string()), "42"),
            (Token::Text("Hello".to_string()), "Hello"),
        ];

        for (token, expected) in cases {
            assert_eq!(token.to_lex_string(), expected, "Token {token:?}");
        }
    }

    #[test]
    fn to_lex_string_handles_blank_line_fallback_and_semantic_tokens() {
        assert_eq!(Token::BlankLine(None).to_lex_string(), "\n");
        assert_eq!(Token::Indent(vec![]).to_lex_string(), "");
        assert_eq!(Token::Dedent(vec![]).to_lex_string(), "");
    }

    #[test]
    fn detokenize_applies_indentation_levels() {
        let tokens = vec![
            Token::Text("Session".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Indent(vec![]),
            Token::Dash,
            Token::Whitespace(1),
            Token::Text("Item".to_string()),
            Token::Whitespace(1),
            Token::Number("1".to_string()),
            Token::BlankLine(Some("\n".to_string())),
            Token::Dedent(vec![]),
            Token::Text("After".to_string()),
            Token::BlankLine(Some("\n".to_string())),
        ];

        let expected = "Session\n    - Item 1\nAfter\n";
        assert_eq!(detokenize(&tokens), expected);
    }

    #[test]
    fn round_trips_all_element_specs() {
        for path in collect_element_spec_files() {
            assert_round_trip(&path);
        }
    }

    #[test]
    fn round_trips_all_document_specs() {
        for doc in [DocumentType::Benchmark, DocumentType::Trifecta] {
            let category = doc.dir_name();
            for path in collect_files_by_number(category, None) {
                assert_round_trip(&path);
            }
        }
    }

    fn collect_element_spec_files() -> Vec<PathBuf> {
        let mut files = Vec::new();
        for element in [
            ElementType::Paragraph,
            ElementType::List,
            ElementType::Session,
            ElementType::Definition,
            ElementType::Annotation,
            ElementType::Verbatim,
        ] {
            let subcategory = element.dir_name();
            files.extend(collect_files_by_number("elements", Some(subcategory)));
        }
        files
    }

    fn collect_files_by_number(category: &str, subcategory: Option<&str>) -> Vec<PathBuf> {
        let root = specfile_finder::get_doc_root(category, subcategory);
        let entries = specfile_finder::list_files_by_number(&root)
            .unwrap_or_else(|err| panic!("Failed to read {}: {}", root.display(), err));
        let mut items: Vec<_> = entries.into_iter().collect();
        items.sort_by_key(|(num, _)| *num);
        items.into_iter().map(|(_, path)| path).collect()
    }

    fn assert_round_trip(path: &Path) {
        let source = fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("Failed to read {}: {}", path.display(), err));
        let source_with_newline = ensure_source_ends_with_newline(&source);
        let canonical_source = canonicalize_indentation(&source_with_newline);
        let semantic_expected = strip_blank_line_whitespace(&canonical_source);
        let raw_with_spans = tokenize(&source_with_newline);
        let raw_tokens: Vec<_> = raw_with_spans.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(
            detokenize(&raw_tokens),
            canonical_source,
            "Raw token round trip failed for {}",
            path.display()
        );

        let semantic_tokens = lex(raw_with_spans).unwrap();
        let semantic_only: Vec<_> = semantic_tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(
            detokenize(&semantic_only),
            semantic_expected,
            "Semantic token round trip failed for {}",
            path.display()
        );
    }

    fn canonicalize_indentation(source: &str) -> String {
        source.replace('\t', "    ")
    }

    fn strip_blank_line_whitespace(source: &str) -> String {
        let mut normalized = String::with_capacity(source.len());
        for chunk in source.split_inclusive('\n') {
            if let Some(content) = chunk.strip_suffix('\n') {
                if content.trim().is_empty() {
                    normalized.push('\n');
                } else {
                    normalized.push_str(content);
                    normalized.push('\n');
                }
            } else {
                normalized.push_str(chunk);
            }
        }
        normalized
    }
}
