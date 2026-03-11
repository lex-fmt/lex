//! Diagnostic collection and validation for LSP support
//!
//! This module provides structured error and warning information that can be consumed
//! by LSP implementations to provide diagnostics in editors.
//!
//! ## Problem
//!
//! The LSP diagnostics feature needs structured error/warning information, but the parser currently:
//! - Only tracks fatal `ParserError::InvalidNesting`
//! - Doesn't collect indentation errors
//! - Doesn't validate references
//! - Doesn't detect malformed structures
//!
//! ## Solution
//!
//! This module provides:
//! - `Diagnostic` struct matching LSP protocol
//! - Validation functions for different error types
//! - Collection API for gathering diagnostics from Documents
//!
//! ## Validation Checks
//!
//! 1. **Reference validation**: Broken footnote/citation references
//! 2. **Structure validation**: Single-item lists, malformed elements
//! 3. **Annotation validation**: Invalid annotation syntax
//!
//! Note: Indentation validation requires access to source text and is implemented
//! separately in the validation functions.

use super::range::Range;
use super::Document;
use std::fmt;

/// Diagnostic severity levels matching LSP protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagnosticSeverity::Error => write!(f, "error"),
            DiagnosticSeverity::Warning => write!(f, "warning"),
            DiagnosticSeverity::Information => write!(f, "info"),
            DiagnosticSeverity::Hint => write!(f, "hint"),
        }
    }
}

/// Structured diagnostic for LSP consumption
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub code: Option<String>,
    pub source: String,
}

impl Diagnostic {
    pub fn new(range: Range, severity: DiagnosticSeverity, message: String) -> Self {
        Self {
            range,
            severity,
            message,
            code: None,
            source: "lex-parser".to_string(),
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}]: {} at {}",
            self.severity, self.source, self.message, self.range.start
        )
    }
}

impl Document {
    /// Get all diagnostics for this document
    ///
    /// This collects diagnostics from various validation checks:
    /// - Broken references (footnotes, citations, session links)
    /// - Malformed structures (single-item lists, etc.)
    /// - Invalid annotation syntax
    ///
    /// # Example
    /// ```rust,ignore
    /// let doc = parse_document(source)?;
    /// let diagnostics = doc.diagnostics();
    /// for diag in diagnostics {
    ///     eprintln!("{}", diag);
    /// }
    /// ```
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Collect reference validation errors
        diagnostics.extend(validate_references(self));

        // Collect structure validation errors
        diagnostics.extend(validate_structure(self));

        diagnostics
    }
}

/// Validate all references in the document
///
/// Checks for:
/// - Broken footnote references `[42]` without matching annotation
/// - Broken citation references `[@key]` without matching annotation
/// - Broken session references `[#section]` without matching session
///
/// # Arguments
/// * `document` - The document to validate
///
/// # Returns
/// Vector of diagnostics for broken references
pub fn validate_references(document: &Document) -> Vec<Diagnostic> {
    use super::traits::{AstNode, Container};
    use crate::lex::inlines::ReferenceType;

    let mut diagnostics = Vec::new();

    // Iterate all references in the document
    for reference in document.iter_all_references() {
        match &reference.reference_type {
            ReferenceType::FootnoteNumber { number } => {
                // Check if annotation with this label exists
                let label = number.to_string();
                if document.find_annotation_by_label(&label).is_none() {
                    // We don't have location info for inline elements yet
                    // Using document root location as fallback
                    let range = document.root.range().clone();
                    let diag = Diagnostic::new(
                        range,
                        DiagnosticSeverity::Warning,
                        format!(
                            "Broken footnote reference: no annotation found with label '{label}'"
                        ),
                    )
                    .with_code("broken-reference");
                    diagnostics.push(diag);
                }
            }
            ReferenceType::FootnoteLabeled { label } => {
                if document.find_annotation_by_label(label).is_none() {
                    let range = document.root.range().clone();
                    let diag = Diagnostic::new(
                        range,
                        DiagnosticSeverity::Warning,
                        format!(
                            "Broken footnote reference: no annotation found with label '{label}'"
                        ),
                    )
                    .with_code("broken-reference");
                    diagnostics.push(diag);
                }
            }
            ReferenceType::Citation(citation_data) => {
                for key in &citation_data.keys {
                    if document.find_annotation_by_label(key).is_none() {
                        let range = document.root.range().clone();
                        let diag = Diagnostic::new(
                            range,
                            DiagnosticSeverity::Warning,
                            format!(
                                "Broken citation reference: no annotation found with label '{key}'"
                            ),
                        )
                        .with_code("broken-citation");
                        diagnostics.push(diag);
                    }
                }
            }
            ReferenceType::Session { target } => {
                // Check if a session with this label exists
                let sessions: Vec<_> = document.root.iter_sessions_recursive().collect();
                let found = sessions.iter().any(|s| s.label() == target);
                if !found {
                    let range = document.root.range().clone();
                    let diag = Diagnostic::new(
                        range,
                        DiagnosticSeverity::Warning,
                        format!("Broken session reference: no session found with title '{target}'"),
                    )
                    .with_code("broken-session-ref");
                    diagnostics.push(diag);
                }
            }
            _ => {
                // URL, File, General, ToCome, and NotSure references don't need validation
            }
        }
    }

    diagnostics
}

/// Validate document structure for common errors
///
/// Checks for:
/// - Single-item lists (should be paragraphs instead)
/// - Invalid annotation syntax
/// - Malformed data nodes
///
/// # Arguments
/// * `document` - The document to validate
///
/// # Returns
/// Vector of diagnostics for structural issues
pub fn validate_structure(document: &Document) -> Vec<Diagnostic> {
    use super::elements::content_item::ContentItem;
    use super::traits::AstNode;

    let mut diagnostics = Vec::new();

    // Iterate all nodes to find structural issues
    for (item, _depth) in document.root.iter_all_nodes_with_depth() {
        match item {
            ContentItem::List(list) => {
                // Check for single-item lists
                if list.items.len() == 1 {
                    let diag = Diagnostic::new(
                        list.range().clone(),
                        DiagnosticSeverity::Information,
                        "Single-item list: consider using a paragraph instead".to_string(),
                    )
                    .with_code("single-item-list");
                    diagnostics.push(diag);
                }
            }
            ContentItem::Annotation(annotation) => {
                // Check for annotations with empty labels
                if annotation.data.label.value.is_empty() {
                    let diag = Diagnostic::new(
                        annotation.range().clone(),
                        DiagnosticSeverity::Error,
                        "Annotation has empty label".to_string(),
                    )
                    .with_code("empty-annotation-label");
                    diagnostics.push(diag);
                }

                // Check for duplicate parameters
                let param_names: Vec<_> = annotation
                    .data
                    .parameters
                    .iter()
                    .map(|p| p.key.as_str())
                    .collect();
                for (i, name) in param_names.iter().enumerate() {
                    if param_names[..i].contains(name) {
                        let diag = Diagnostic::new(
                            annotation.range().clone(),
                            DiagnosticSeverity::Warning,
                            format!("Duplicate parameter: '{name}'"),
                        )
                        .with_code("duplicate-parameter");
                        diagnostics.push(diag);
                        break;
                    }
                }
            }
            ContentItem::VerbatimBlock(verbatim) => {
                // Check for empty verbatim block label
                if verbatim.closing_data.label.value.is_empty() {
                    let diag = Diagnostic::new(
                        verbatim.range().clone(),
                        DiagnosticSeverity::Warning,
                        "Verbatim block has empty closing label".to_string(),
                    )
                    .with_code("empty-verbatim-label");
                    diagnostics.push(diag);
                }
            }
            _ => {
                // Other content types don't need structural validation
            }
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::parsing::parse_document;

    #[test]
    fn test_diagnostic_creation() {
        use super::super::range::Position;

        let range = Range::new(0..10, Position::new(1, 0), Position::new(1, 10));
        let diag = Diagnostic::new(range, DiagnosticSeverity::Error, "Test error".to_string())
            .with_code("test-001");

        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert_eq!(diag.message, "Test error");
        assert_eq!(diag.code, Some("test-001".to_string()));
        assert_eq!(diag.source, "lex-parser");
    }

    #[test]
    fn test_broken_footnote_reference() {
        let source = "A paragraph with a footnote reference [42].\n\n";
        let doc = parse_document(source).unwrap();

        let diagnostics = validate_references(&doc);

        // Should find broken reference to [42]
        assert!(!diagnostics.is_empty());
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("Broken footnote reference")
                    && d.message.contains("'42'"))
        );
    }

    #[test]
    fn test_valid_footnote_reference() {
        let source =
            "A paragraph with a footnote reference [42].\n\n:: 42 :: Footnote content.\n\n";
        let doc = parse_document(source).unwrap();

        let diagnostics = validate_references(&doc);

        // Should not find broken reference
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("Broken footnote reference")),
            "Expected no broken footnote reference diagnostics, got: {diagnostics:?}"
        );
    }

    #[test]
    fn test_valid_structure_no_warnings() {
        let source = ":: note :: A valid annotation.\n\n";
        let doc = parse_document(source).unwrap();

        let diagnostics = validate_structure(&doc);

        // Should not find any structural issues
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_document_diagnostics_api() {
        let source = "A paragraph with [42].\n\n:: 42 :: Valid footnote.\n\n";
        let doc = parse_document(source).unwrap();

        let diagnostics = doc.diagnostics();

        // Should NOT find broken reference since annotation with label "42" exists
        assert!(!diagnostics
            .iter()
            .any(|d| d.message.contains("Broken footnote reference")));
    }
}
