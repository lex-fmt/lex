//! File loading, parsing, and tokenization for Lex test harness
//!
//! This module provides the core loading infrastructure for the Lexplore test harness,
//! handling file discovery, reading, parsing, and tokenization.
//!
//! The Lexplore API now returns `DocumentLoader` which provides a fluent interface
//! for running transforms on test files.

use crate::lex::ast::elements::{
    Annotation, Definition, List, Paragraph, Session, Table, Verbatim,
};
use crate::lex::ast::Document;
use crate::lex::loader::DocumentLoader;
use crate::lex::parsing::parse_document;
use crate::lex::parsing::ParseError;
use crate::lex::testing::lexplore::specfile_finder;
use std::fs;

// Re-export types from specfile_finder for public API
pub use specfile_finder::{DocumentType, ElementType};

// Parser enum is now defined in crate::lex::pipeline::loader and re-exported from pipeline module

/// Errors that can occur when loading element sources
#[derive(Debug, Clone)]
pub enum ElementSourceError {
    FileNotFound(String),
    IoError(String),
    ParseError(String),
    InvalidElement(String),
}

impl std::fmt::Display for ElementSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElementSourceError::FileNotFound(msg) => write!(f, "File not found: {msg}"),
            ElementSourceError::IoError(msg) => write!(f, "IO error: {msg}"),
            ElementSourceError::ParseError(msg) => write!(f, "Parse error: {msg}"),
            ElementSourceError::InvalidElement(msg) => write!(f, "Invalid element: {msg}"),
        }
    }
}

impl std::error::Error for ElementSourceError {}

impl From<std::io::Error> for ElementSourceError {
    fn from(err: std::io::Error) -> Self {
        ElementSourceError::IoError(err.to_string())
    }
}

impl From<ParseError> for ElementSourceError {
    fn from(err: ParseError) -> Self {
        ElementSourceError::ParseError(err.to_string())
    }
}

impl From<specfile_finder::SpecFileError> for ElementSourceError {
    fn from(err: specfile_finder::SpecFileError) -> Self {
        match err {
            specfile_finder::SpecFileError::FileNotFound(msg) => {
                ElementSourceError::FileNotFound(msg)
            }
            specfile_finder::SpecFileError::IoError(msg) => ElementSourceError::IoError(msg),
            specfile_finder::SpecFileError::DuplicateNumber(msg) => {
                ElementSourceError::IoError(msg)
            }
        }
    }
}

// ElementLoader has been replaced by DocumentLoader from lex::loader
// Lexplore methods now return DocumentLoader directly

/// Helper function to load and parse an isolated element file
///
/// This function orchestrates:
/// 1. Path resolution via specfile_finder
/// 2. File parsing via parsing engine (skipping annotation attachment for annotation elements)
/// 3. Returns the parsed Document
///
/// Used internally by the get_* convenience functions.
fn load_isolated_element(element_type: ElementType, number: usize) -> Document {
    let path = specfile_finder::find_element_file(element_type, number)
        .unwrap_or_else(|e| panic!("Failed to find {element_type:?} #{number}: {e}"));
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));

    // For annotation elements, skip annotation attachment so they remain in content tree
    if matches!(element_type, ElementType::Annotation) {
        use crate::lex::testing::parse_without_annotation_attachment;
        parse_without_annotation_attachment(&source).unwrap()
    } else {
        parse_document(&source).unwrap()
    }
}

/// Macro to generate element loader shortcuts
macro_rules! element_shortcuts {
    ($($name:ident => $variant:ident, $label:literal);* $(;)?) => {
        $(
            #[doc = concat!("Load a ", $label, " file (returns DocumentLoader for transforms)")]
            pub fn $name(number: usize) -> DocumentLoader {
                Self::load(ElementType::$variant, number)
            }
        )*
    };
}

/// Macro to generate document loader shortcuts
macro_rules! document_shortcuts {
    ($($name:ident => $variant:ident, $label:literal);* $(;)?) => {
        $(
            #[doc = concat!("Load a ", $label, " document (returns DocumentLoader for transforms)")]
            pub fn $name(number: usize) -> DocumentLoader {
                Self::load_document(DocumentType::$variant, number)
            }
        )*
    };
}

// ============================================================================
// FLUENT API - Delegates to specfile_finder for file resolution
// ============================================================================

/// Interface for loading per-element test sources
pub struct Lexplore;

impl Lexplore {
    // ===== Fluent API - returns DocumentLoader =====

    /// Load an element file by type and number
    ///
    /// Returns a `DocumentLoader` which provides transform shortcuts.
    pub fn load(element_type: ElementType, number: usize) -> DocumentLoader {
        let path = specfile_finder::find_element_file(element_type, number)
            .unwrap_or_else(|e| panic!("Failed to find {element_type:?} #{number}: {e}"));
        DocumentLoader::from_path(path)
            .unwrap_or_else(|e| panic!("Failed to load {element_type:?} #{number}: {e}"))
    }

    /// Load a document collection file by type and number
    ///
    /// Returns a `DocumentLoader` which provides transform shortcuts.
    pub fn load_document(doc_type: DocumentType, number: usize) -> DocumentLoader {
        let path = specfile_finder::find_document_file(doc_type, number)
            .unwrap_or_else(|e| panic!("Failed to find {doc_type:?} #{number}: {e}"));
        DocumentLoader::from_path(path)
            .unwrap_or_else(|e| panic!("Failed to load {doc_type:?} #{number}: {e}"))
    }

    /// Load from an arbitrary file path
    ///
    /// Returns a `DocumentLoader` which provides transform shortcuts.
    pub fn from_path<P: AsRef<std::path::Path>>(path: P) -> DocumentLoader {
        DocumentLoader::from_path(path).unwrap_or_else(|e| panic!("Failed to load from path: {e}"))
    }

    // ===== Isolated element loading (returns AST node directly) =====

    /// Load a paragraph element file and return the paragraph directly
    ///
    /// # Example
    /// ```ignore
    /// let paragraph = Lexplore::get_paragraph(3);
    /// assert!(paragraph.text().starts_with("Expected"));
    /// ```
    pub fn get_paragraph(number: usize) -> &'static Paragraph {
        let doc = Box::leak(Box::new(load_isolated_element(
            ElementType::Paragraph,
            number,
        )));
        doc.root.expect_paragraph()
    }

    /// Load a list element file and return the list directly
    pub fn get_list(number: usize) -> &'static List {
        let doc = Box::leak(Box::new(load_isolated_element(ElementType::List, number)));
        doc.root.expect_list()
    }

    /// Load a session element file and return the session directly
    pub fn get_session(number: usize) -> &'static Session {
        let doc = Box::leak(Box::new(load_isolated_element(
            ElementType::Session,
            number,
        )));
        doc.root.expect_session()
    }

    /// Load a definition element file and return the definition directly
    pub fn get_definition(number: usize) -> &'static Definition {
        let doc = Box::leak(Box::new(load_isolated_element(
            ElementType::Definition,
            number,
        )));
        doc.root.expect_definition()
    }

    /// Load an annotation element file and return the annotation directly
    pub fn get_annotation(number: usize) -> &'static Annotation {
        let doc = Box::leak(Box::new(load_isolated_element(
            ElementType::Annotation,
            number,
        )));
        doc.root.expect_annotation()
    }

    /// Load a verbatim element file and return the verbatim block directly
    pub fn get_verbatim(number: usize) -> &'static Verbatim {
        let doc = Box::leak(Box::new(load_isolated_element(
            ElementType::Verbatim,
            number,
        )));
        doc.root.expect_verbatim()
    }

    /// Load a table element file and return the table directly
    pub fn get_table(number: usize) -> &'static Table {
        let doc = Box::leak(Box::new(load_isolated_element(ElementType::Table, number)));
        doc.root.expect_table()
    }

    // ===== Convenience shortcuts for element files (fluent API) =====

    element_shortcuts! {
        paragraph => Paragraph, "paragraph";
        list => List, "list";
        session => Session, "session";
        definition => Definition, "definition";
        annotation => Annotation, "annotation";
        verbatim => Verbatim, "verbatim";
        table => Table, "table";
        document => Document, "document";
        footnotes => Footnotes, "footnotes";
    }

    // ===== Convenience shortcuts for document collections =====

    document_shortcuts! {
        benchmark => Benchmark, "benchmark";
        trifecta => Trifecta, "trifecta";
    }

    // ===== Utility methods =====

    /// List all available numbers for a given element type
    pub fn list_numbers_for(element_type: ElementType) -> Result<Vec<usize>, ElementSourceError> {
        Ok(specfile_finder::list_element_numbers(element_type)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::traits::Container;
    use crate::lex::lexing::Token;
    use crate::lex::testing::lexplore::extraction::*;
    use crate::lex::testing::workspace_path;

    // Tests for the old direct API (get_source_for, etc.) have been removed.
    // Use the fluent API instead: Lexplore::paragraph(1).parse()

    #[test]
    fn test_list_numbers_for_paragraphs() {
        let numbers = Lexplore::list_numbers_for(ElementType::Paragraph).unwrap();
        assert!(!numbers.is_empty());
        assert!(numbers.contains(&1));
    }

    // ===== Fluent API Tests =====

    #[test]
    fn test_get_paragraph() {
        let paragraph = Lexplore::get_paragraph(1);

        assert!(paragraph_text_starts_with(paragraph, "This is a simple"));
    }

    #[test]
    fn test_get_list() {
        let list = Lexplore::get_list(1);

        assert!(!list.items.is_empty());
    }

    #[test]
    fn test_get_session() {
        let session = Lexplore::get_session(1);

        assert!(!session.label().is_empty());
    }

    #[test]
    fn test_get_definition() {
        let definition = Lexplore::get_definition(1);

        assert!(!definition.label().is_empty());
    }

    // Removed test for deleted API: test_must_methods

    // ===== Document Collection Tests =====

    #[test]
    fn test_benchmark_fluent_api() {
        let doc = Lexplore::benchmark(10).parse().unwrap();

        assert!(!doc.root.children.is_empty());
    }

    #[test]
    fn test_trifecta_fluent_api() {
        let doc = Lexplore::trifecta(0).parse().unwrap();

        assert!(!doc.root.children.is_empty());
    }

    #[test]
    fn test_benchmark_source_only() {
        let source = Lexplore::benchmark(10).source();
        assert!(!source.is_empty());
    }

    #[test]
    fn test_trifecta_source_only() {
        let source = Lexplore::trifecta(0).source();
        assert!(!source.is_empty());
    }

    // Removed test for deleted API: test_get_document_source_for

    // Removed test for deleted API: test_must_get_document_source_for

    // Removed test for deleted API: test_get_document_ast_for

    // Removed test for deleted API: test_must_get_document_ast_for

    // ===== Tokenization Tests =====

    #[test]
    fn test_tokenize_paragraph() {
        let tokens = Lexplore::paragraph(1).tokenize().unwrap();

        assert!(!tokens.is_empty());
    }

    #[test]
    fn test_tokenize_list() {
        let tokens = Lexplore::list(1).tokenize().unwrap();

        assert!(
            tokens.iter().any(|(t, _)| matches!(t, Token::Dash))
                || tokens.iter().any(|(t, _)| matches!(t, Token::Number(_)))
        );
    }

    #[test]
    fn test_tokenize_benchmark() {
        let tokens = Lexplore::benchmark(10).tokenize().unwrap();

        assert!(!tokens.is_empty());
        assert!(tokens.len() > 10);
    }

    #[test]
    fn test_tokenize_trifecta() {
        let tokens = Lexplore::trifecta(0).tokenize().unwrap();

        assert!(!tokens.is_empty());
        assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Text(_))));
    }

    // ===== Path-based Loading Tests =====

    #[test]
    fn test_from_path_parse() {
        let path =
            workspace_path("comms/specs/elements/paragraph.docs/paragraph-01-flat-oneline.lex");
        let doc = Lexplore::from_path(path).parse().unwrap();

        let paragraph = doc.root.expect_paragraph();
        assert!(!paragraph.text().is_empty());
    }

    #[test]
    fn test_from_path_tokenize() {
        let path =
            workspace_path("comms/specs/elements/paragraph.docs/paragraph-01-flat-oneline.lex");
        let tokens = Lexplore::from_path(path).tokenize().unwrap();

        assert!(!tokens.is_empty());
        assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Text(_))));
    }

    #[test]
    fn test_from_path_source() {
        let path =
            workspace_path("comms/specs/elements/paragraph.docs/paragraph-01-flat-oneline.lex");
        let source = Lexplore::from_path(path).source();

        assert!(!source.is_empty());
    }
    // Removed test for deleted API: test_get_source_from_path

    // Removed test for deleted API: test_must_get_source_from_path

    // Removed test for deleted API: test_get_ast_from_path

    // Removed test for deleted API: test_must_get_ast_from_path

    // Removed test for deleted API: test_get_tokens_from_path

    // Removed test for deleted API: test_must_get_tokens_from_path

    #[test]
    fn test_from_path_with_benchmark() {
        let path = workspace_path("comms/specs/benchmark/010-kitchensink.lex");
        let doc = Lexplore::from_path(path).parse().unwrap();

        assert!(!doc.root.children.is_empty());
    }

    #[test]
    fn test_from_path_with_trifecta() {
        let path = workspace_path("comms/specs/trifecta/000-paragraphs.lex");
        let doc = Lexplore::from_path(path).parse().unwrap();

        assert!(!doc.root.children.is_empty());
    }

    // ===== Isolated Element Loading Tests =====

    #[test]
    fn test_get_paragraph_direct() {
        let paragraph = Lexplore::get_paragraph(1);

        assert!(paragraph_text_starts_with(paragraph, "This is a simple"));
    }

    #[test]
    fn test_get_list_direct() {
        let list = Lexplore::get_list(1);

        assert!(!list.items.is_empty());
    }

    #[test]
    fn test_get_session_direct() {
        let session = Lexplore::get_session(1);

        assert!(!session.label().is_empty());
    }

    #[test]
    fn test_get_definition_direct() {
        let definition = Lexplore::get_definition(1);

        assert!(!definition.label().is_empty());
    }

    #[test]
    fn test_get_annotation_direct() {
        let _annotation = Lexplore::get_annotation(1);

        // Just verify it doesn't panic - annotation was successfully loaded
    }

    #[test]
    fn test_get_verbatim_direct() {
        let _verbatim = Lexplore::get_verbatim(1);

        // Just verify it doesn't panic - verbatim block was successfully loaded
    }
}
