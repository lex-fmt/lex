//! Common parser module
//!
//! This module contains shared interfaces for parser implementations.

use std::fmt;

use crate::lex::lexing::Token;
/// Input type for parsers
pub type ParserInput = Vec<(Token, std::ops::Range<usize>)>;

/// Errors that can occur during parsing
#[derive(Debug, Clone)]
pub enum ParseError {
    /// Generic error message
    Error(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Error(msg) => write!(f, "Parse error: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {}
