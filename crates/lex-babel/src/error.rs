//! Error types for format operations

use std::fmt;

/// Errors that can occur during format operations
#[derive(Debug, Clone, PartialEq)]
pub enum FormatError {
    /// Format not found in registry
    FormatNotFound(String),
    /// Error during parsing
    ParseError(String),
    /// Error during serialization
    SerializationError(String),
    /// Format does not support parsing
    NotSupported(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::FormatNotFound(name) => write!(f, "Format '{name}' not found"),
            FormatError::ParseError(msg) => write!(f, "Parse error: {msg}"),
            FormatError::SerializationError(msg) => write!(f, "Serialization error: {msg}"),
            FormatError::NotSupported(msg) => write!(f, "Operation not supported: {msg}"),
        }
    }
}

impl std::error::Error for FormatError {}
