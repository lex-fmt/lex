//! Base tokenization implementation for the lex lexer
//!
//!     This module provides the raw tokenization using the logos lexer library.
//!     This is the entry point where source strings become token streams.
//!
//!     We leverage the logos lexer to tokenize the source text into core tokens. This is done
//!     declaratively with no custom logic, and could not be simpler. The logos lexer produces
//!     tokens based on the grammar specification defined in the Token enum.
//!
//!     This is NOT a transformation - transformations operate on token streams. This is the
//!     source that creates the initial token stream from a string.
//!
//!     The tokens produced by this stage carry byte ranges into the source text. These byte
//!     ranges are preserved through all transformations and are used at the AST building stage
//!     for location tracking. It is critical that these ranges are not modified by any
//!     transformation step.

use crate::lex::token::Token;
use logos::Logos;

/// Tokenize source code with location information
///
/// This function performs raw tokenization using the logos lexer, returning tokens
/// paired with their source locations. This is the base tokenization step that
/// converts source strings into token streams.
///
/// Pipelines and transformations should operate on the token stream produced by this function,
/// not call it directly. The caller (e.g., LexerRegistry implementations) should call this
/// and pass the result to pipelines.
pub fn tokenize(source: &str) -> Vec<(Token, logos::Span)> {
    let mut lexer = Token::lexer(source);
    let mut tokens = Vec::new();

    while let Some(result) = lexer.next() {
        if let Ok(token) = result {
            tokens.push((token, lexer.span()));
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenizes() {
        let tokenss = tokenize("hello world");
        assert_eq!(tokenss.len(), 3);

        // Check that tokens are correct
        assert_eq!(tokenss[0].0, Token::Text("hello".to_string()));
        assert_eq!(tokenss[1].0, Token::Whitespace(1));
        assert_eq!(tokenss[2].0, Token::Text("world".to_string()));
    }

    #[test]
    fn test_empty_input() {
        let tokenss = tokenize("");
        assert_eq!(tokenss, vec![]);
    }

    #[test]
    fn test_complex_tokenization() {
        let input = "1. Session Title\n    - Item 1\n    - Item 2";
        let tokenss = tokenize(input);

        // Expected tokens for "1. Session Title"
        assert_eq!(tokenss[0].0, Token::Number("1".to_string())); // "1"
        assert_eq!(tokenss[1].0, Token::Period); // "."
        assert_eq!(tokenss[2].0, Token::Whitespace(1)); // " "
        assert_eq!(tokenss[3].0, Token::Text("Session".to_string())); // "Session"
        assert_eq!(tokenss[4].0, Token::Whitespace(1)); // " "
        assert_eq!(tokenss[5].0, Token::Text("Title".to_string())); // "Title"
        assert_eq!(tokenss[6].0, Token::BlankLine(Some("\n".to_string()))); // "\n"

        // Expected tokens for "    - Item 1"
        assert_eq!(tokenss[7].0, Token::Indentation); // "    "
        assert_eq!(tokenss[8].0, Token::Dash); // "-"
        assert_eq!(tokenss[9].0, Token::Whitespace(1)); // " "
        assert_eq!(tokenss[10].0, Token::Text("Item".to_string())); // "Item"
        assert_eq!(tokenss[11].0, Token::Whitespace(1)); // " "
        assert_eq!(tokenss[12].0, Token::Number("1".to_string())); // "1"
        assert_eq!(tokenss[13].0, Token::BlankLine(Some("\n".to_string()))); // "\n"

        // Expected tokens for "    - Item 2"
        assert_eq!(tokenss[14].0, Token::Indentation); // "    "
        assert_eq!(tokenss[15].0, Token::Dash); // "-"
        assert_eq!(tokenss[16].0, Token::Whitespace(1)); // " "
        assert_eq!(tokenss[17].0, Token::Text("Item".to_string())); // "Item"
        assert_eq!(tokenss[18].0, Token::Whitespace(1)); // " "
        assert_eq!(tokenss[19].0, Token::Number("2".to_string()));
        // "2"
    }

    #[test]
    fn test_whitespace_only() {
        let tokenss = tokenize("   \t  ");
        // Expected: 3 spaces -> Whitespace, 1 tab -> Indent, 2 spaces -> Whitespace
        assert_eq!(tokenss.len(), 3);
        assert_eq!(tokenss[0].0, Token::Whitespace(3));
        assert_eq!(tokenss[1].0, Token::Indentation);
        assert_eq!(tokenss[2].0, Token::Whitespace(2));
    }
}
