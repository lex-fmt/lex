//! Document loading and transform execution
//!
//! This module provides [`DocumentLoader`] - the universal API for loading and processing
//! lex source text. It abstracts over input sources (file vs. string) and provides convenient
//! shortcuts for common transform operations.
//!
//! # Purpose
//!
//! `DocumentLoader` serves as the primary entry point for most lex processing tasks:
//!
//! - Universal Input: Load from files or strings with the same API
//! - Transform Shortcuts: Common operations (`.parse()`, `.tokenize()`) built-in
//! - Custom Transforms: Execute any transform via `.with()`
//! - Source Access: Retrieve original source text via `.source()`
//! - Reusable: Create once, run multiple transforms on the same source
//!
//! # Relationship to Transform System
//!
//! `DocumentLoader` is a convenience layer on top of the [transform system](crate::lex::transforms).
//! It manages source text loading and delegates to the appropriate transform:
//!
//! - `.parse()` → Uses [`STRING_TO_AST`]
//! - `.tokenize()` → Uses [`LEXING`]
//! - `.base_tokens()` → Uses [`CORE_TOKENIZATION`]
//! - `.with(transform)` → Runs any custom transform
//!
//! # Common Usage Patterns
//!
//! ## Parse a Document
//!
//! ```rust
//! use lex_parser::lex::loader::DocumentLoader;
//!
//! let loader = DocumentLoader::from_string("Session:\n    Content\n");
//! let doc = loader.parse()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Load from File
//!
//! ```rust,no_run
//! use lex_parser::lex::loader::DocumentLoader;
//!
//! let loader = DocumentLoader::from_path("document.lex")?;
//! let doc = loader.parse()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Get Tokens
//!
//! ```rust
//! use lex_parser::lex::loader::DocumentLoader;
//!
//! let loader = DocumentLoader::from_string("Hello world\n");
//! let tokens = loader.tokenize()?;  // With semantic indentation
//! let base = loader.base_tokens()?; // Core tokens only
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Custom Transform
//!
//! ```rust
//! use lex_parser::lex::loader::DocumentLoader;
//! use lex_parser::lex::transforms::standard::TO_IR;
//!
//! let loader = DocumentLoader::from_string("Hello\n");
//! let ir = loader.with(&*TO_IR)?;  // Get intermediate representation
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Multiple Operations on Same Source
//!
//! ```rust
//! use lex_parser::lex::loader::DocumentLoader;
//!
//! let loader = DocumentLoader::from_string("Hello\n");
//! let source = loader.source();      // Get original text
//! let tokens = loader.tokenize()?;   // Get tokens
//! let doc = loader.parse()?;         // Get AST
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Use Cases
//!
//! - CLI Tools: Load files and apply stage+format transforms
//! - Tests: Load test fixtures and verify different processing stages
//! - Library Code: Process lex documents programmatically
//! - REPL/Interactive: Parse user input on-the-fly

use crate::lex::parsing::Document;
use crate::lex::transforms::standard::{TokenStream, CORE_TOKENIZATION, LEXING, STRING_TO_AST};
use crate::lex::transforms::{Transform, TransformError};
use std::fs;
use std::path::Path;

/// Error that can occur when loading documents
#[derive(Debug, Clone)]
pub enum LoaderError {
    /// IO error when reading file
    IoError(String),
    /// Transform/parsing error
    TransformError(TransformError),
}

impl std::fmt::Display for LoaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoaderError::IoError(msg) => write!(f, "IO error: {msg}"),
            LoaderError::TransformError(err) => write!(f, "Transform error: {err}"),
        }
    }
}

impl std::error::Error for LoaderError {}

impl From<std::io::Error> for LoaderError {
    fn from(err: std::io::Error) -> Self {
        LoaderError::IoError(err.to_string())
    }
}

impl From<TransformError> for LoaderError {
    fn from(err: TransformError) -> Self {
        LoaderError::TransformError(err)
    }
}

/// Document loader with transform shortcuts
///
/// `DocumentLoader` provides a convenient API for loading source text and running
/// transforms on it. It's used by both production code (CLI, libraries) and tests.
///
/// # Example
///
/// ```rust
/// use lex_parser::lex::loader::DocumentLoader;
///
/// // Load from file and parse
/// let doc = DocumentLoader::from_path("example.lex")
///     .unwrap()
///     .parse()
///     .unwrap();
///
/// // Load from string and get tokens
/// let tokens = DocumentLoader::from_string("Hello world\n")
///     .tokenize()
///     .unwrap();
/// ```
pub struct DocumentLoader {
    source: String,
}

impl DocumentLoader {
    /// Load from a file path
    ///
    /// # Example
    ///
    /// ```rust
    /// use lex_parser::lex::loader::DocumentLoader;
    ///
    /// let loader = DocumentLoader::from_path("example.lex").unwrap();
    /// ```
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, LoaderError> {
        let source = fs::read_to_string(path)?;
        Ok(DocumentLoader { source })
    }

    /// Load from a string
    ///
    /// # Example
    ///
    /// ```rust
    /// use lex_parser::lex::loader::DocumentLoader;
    ///
    /// let loader = DocumentLoader::from_string("Hello world\n");
    /// ```
    pub fn from_string<S: Into<String>>(source: S) -> Self {
        DocumentLoader {
            source: source.into(),
        }
    }

    /// Run a custom transform on the source
    ///
    /// This is the generic method that all shortcuts use internally.
    ///
    /// # Example
    ///
    /// ```rust
    /// use lex_parser::lex::loader::DocumentLoader;
    /// use lex_parser::lex::transforms::standard::LEXING;
    ///
    /// let loader = DocumentLoader::from_string("Hello\n");
    /// let tokens = loader.with(&*LEXING).unwrap();
    /// ```
    pub fn with<O: 'static>(&self, transform: &Transform<String, O>) -> Result<O, LoaderError> {
        Ok(transform.run(self.source.clone())?)
    }

    /// Parse the source into a Document AST
    ///
    /// This is a shortcut for `.with(&STRING_TO_AST)`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use lex_parser::lex::loader::DocumentLoader;
    ///
    /// let doc = DocumentLoader::from_string("Hello world\n")
    ///     .parse()
    ///     .unwrap();
    /// ```
    pub fn parse(&self) -> Result<Document, LoaderError> {
        self.with(&STRING_TO_AST)
    }

    /// Tokenize the source with full lexing (including semantic indentation)
    ///
    /// This is a shortcut for `.with(&LEXING)`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use lex_parser::lex::loader::DocumentLoader;
    ///
    /// let tokens = DocumentLoader::from_string("Session:\n    Content\n")
    ///     .tokenize()
    ///     .unwrap();
    /// // tokens include Indent/Dedent
    /// ```
    pub fn tokenize(&self) -> Result<TokenStream, LoaderError> {
        self.with(&LEXING)
    }

    /// Get base tokens (core tokenization only, no semantic indentation)
    ///
    /// This is a shortcut for `.with(&CORE_TOKENIZATION)`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use lex_parser::lex::loader::DocumentLoader;
    ///
    /// let tokens = DocumentLoader::from_string("Hello\n")
    ///     .base_tokens()
    ///     .unwrap();
    /// // tokens include raw Indentation tokens, not Indent/Dedent
    /// ```
    pub fn base_tokens(&self) -> Result<TokenStream, LoaderError> {
        self.with(&CORE_TOKENIZATION)
    }

    /// Get the raw source string
    ///
    /// # Example
    ///
    /// ```rust
    /// use lex_parser::lex::loader::DocumentLoader;
    ///
    /// let loader = DocumentLoader::from_string("Hello\n");
    /// assert_eq!(loader.source(), "Hello\n");
    /// ```
    pub fn source(&self) -> String {
        self.source.clone()
    }

    /// Get a reference to the raw source string
    ///
    /// Use this when you don't need an owned copy.
    pub fn source_ref(&self) -> &str {
        &self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::testing::workspace_path;
    use crate::lex::token::Token;

    #[test]
    fn test_from_string() {
        let loader = DocumentLoader::from_string("Hello world\n");
        assert_eq!(loader.source(), "Hello world\n");
    }

    #[test]
    fn test_from_path() {
        let path =
            workspace_path("comms/specs/elements/paragraph.docs/paragraph-01-flat-oneline.lex");
        let loader = DocumentLoader::from_path(path).unwrap();
        assert!(!loader.source().is_empty());
    }

    #[test]
    fn test_from_path_nonexistent() {
        let result = DocumentLoader::from_path("nonexistent.lex");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse() {
        let loader = DocumentLoader::from_string("Hello world\n");
        let doc = loader.parse().unwrap();
        assert!(!doc.root.children.is_empty());
    }

    #[test]
    fn test_parse_with_session() {
        let loader = DocumentLoader::from_string("Session:\n    Content here\n");
        let doc = loader.parse().unwrap();
        assert!(!doc.root.children.is_empty());
    }

    #[test]
    fn test_tokenize() {
        let loader = DocumentLoader::from_string("Session:\n    Content\n");
        let tokens = loader.tokenize().unwrap();

        // Should have Indent/Dedent tokens
        assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Indent(_))));
        assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Dedent(_))));
    }

    #[test]
    fn test_base_tokens() {
        let loader = DocumentLoader::from_string("Hello world\n");
        let tokens = loader.base_tokens().unwrap();

        assert!(!tokens.is_empty());
        // Should not have Indent/Dedent (those come from semantic indentation)
        assert!(!tokens.iter().any(|(t, _)| matches!(t, Token::Indent(_))));
    }

    #[test]
    fn test_base_tokens_has_indentation() {
        let loader = DocumentLoader::from_string("    Hello\n");
        let tokens = loader.base_tokens().unwrap();

        // Should have raw Indentation tokens
        assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Indentation)));
    }

    #[test]
    fn test_source() {
        let loader = DocumentLoader::from_string("Test content\n");
        assert_eq!(loader.source(), "Test content\n");
    }

    #[test]
    fn test_with_custom_transform() {
        let loader = DocumentLoader::from_string("Hello\n");
        let tokens = loader.with(&CORE_TOKENIZATION).unwrap();
        assert!(!tokens.is_empty());
    }

    #[test]
    fn test_loader_is_reusable() {
        let loader = DocumentLoader::from_string("Hello\n");

        // Can call multiple methods on the same loader
        let _tokens = loader.tokenize().unwrap();
        let _doc = loader.parse().unwrap();
        let _source = loader.source();

        // All should work
    }

    #[test]
    fn test_from_path_integration() {
        let path = workspace_path("comms/specs/benchmark/010-kitchensink.lex");
        let loader = DocumentLoader::from_path(path).unwrap();

        let doc = loader.parse().unwrap();
        assert!(!doc.root.children.is_empty());
    }
}
