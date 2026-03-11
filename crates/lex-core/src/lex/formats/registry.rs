//! Format registry for AST serialization
//!
//! This module provides a pluggable registry system for document serialization formats.
//! Each format implements the `Formatter` trait and can be registered with `FormatRegistry`.

use crate::lex::ast::Document;
use std::collections::HashMap;
use std::fmt;

/// Error that can occur during formatting
#[derive(Debug, Clone, PartialEq)]
pub enum FormatError {
    /// Format not found in registry
    FormatNotFound(String),
    /// Error during serialization
    SerializationError(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::FormatNotFound(name) => write!(f, "Format '{name}' not found"),
            FormatError::SerializationError(msg) => write!(f, "Serialization error: {msg}"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Trait for document formatters
///
/// Implementors provide a way to serialize a Document to a string representation.
pub trait Formatter: Send + Sync {
    /// The name of this format (e.g., "treeviz", "tag")
    fn name(&self) -> &str;

    /// Serialize a document to this format
    fn serialize(&self, doc: &Document) -> Result<String, FormatError>;

    /// Optional description of this format
    fn description(&self) -> &str {
        ""
    }
}

/// Registry of document formatters
///
/// Provides a centralized registry for all available serialization formats.
/// Formats can be registered and retrieved by name.
pub struct FormatRegistry {
    formatters: HashMap<String, Box<dyn Formatter>>,
}

impl FormatRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        FormatRegistry {
            formatters: HashMap::new(),
        }
    }

    /// Register a formatter
    ///
    /// If a formatter with the same name already exists, it will be replaced.
    pub fn register<F: Formatter + 'static>(&mut self, formatter: F) {
        self.formatters
            .insert(formatter.name().to_string(), Box::new(formatter));
    }

    /// Get a formatter by name
    pub fn get(&self, name: &str) -> Option<&dyn Formatter> {
        self.formatters.get(name).map(|f| f.as_ref())
    }

    /// Check if a format exists
    pub fn has(&self, name: &str) -> bool {
        self.formatters.contains_key(name)
    }

    /// Serialize a document using the specified format
    pub fn serialize(&self, doc: &Document, format: &str) -> Result<String, FormatError> {
        let formatter = self
            .get(format)
            .ok_or_else(|| FormatError::FormatNotFound(format.to_string()))?;
        formatter.serialize(doc)
    }

    /// List all available format names (sorted)
    pub fn list_formats(&self) -> Vec<String> {
        let mut names: Vec<_> = self.formatters.keys().cloned().collect();
        names.sort();
        names
    }

    /// Create a registry with default formatters
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Register built-in formatters
        registry.register(super::TreevizFormatter);
        registry.register(super::TagFormatter);

        registry
    }
}

impl Default for FormatRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::{ContentItem, Paragraph};

    // Test formatter
    struct TestFormatter;
    impl Formatter for TestFormatter {
        fn name(&self) -> &str {
            "test"
        }
        fn serialize(&self, _doc: &Document) -> Result<String, FormatError> {
            Ok("test output".to_string())
        }
        fn description(&self) -> &str {
            "Test formatter"
        }
    }

    #[test]
    fn test_registry_creation() {
        let registry = FormatRegistry::new();
        assert_eq!(registry.formatters.len(), 0);
    }

    #[test]
    fn test_registry_register() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormatter);

        assert!(registry.has("test"));
        assert_eq!(registry.list_formats(), vec!["test"]);
    }

    #[test]
    fn test_registry_get() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormatter);

        let formatter = registry.get("test");
        assert!(formatter.is_some());
        assert_eq!(formatter.unwrap().name(), "test");
    }

    #[test]
    fn test_registry_get_nonexistent() {
        let registry = FormatRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_has() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormatter);

        assert!(registry.has("test"));
        assert!(!registry.has("nonexistent"));
    }

    #[test]
    fn test_registry_serialize() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormatter);

        let doc = Document::with_content(vec![ContentItem::Paragraph(Paragraph::from_line(
            "Hello".to_string(),
        ))]);

        let result = registry.serialize(&doc, "test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test output");
    }

    #[test]
    fn test_registry_serialize_not_found() {
        let registry = FormatRegistry::new();
        let doc = Document::with_content(vec![]);

        let result = registry.serialize(&doc, "nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            FormatError::FormatNotFound(name) => assert_eq!(name, "nonexistent"),
            _ => panic!("Expected FormatNotFound error"),
        }
    }

    #[test]
    fn test_registry_list_formats() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormatter);

        let formats = registry.list_formats();
        assert_eq!(formats.len(), 1);
        assert_eq!(formats[0], "test");
    }

    #[test]
    fn test_registry_with_defaults() {
        let registry = FormatRegistry::with_defaults();
        assert!(registry.has("treeviz"));
        assert!(registry.has("tag"));
    }

    #[test]
    fn test_registry_default_trait() {
        let registry = FormatRegistry::default();
        assert!(registry.has("treeviz"));
        assert!(registry.has("tag"));
    }

    #[test]
    fn test_format_error_display() {
        let err1 = FormatError::FormatNotFound("test".to_string());
        assert_eq!(format!("{err1}"), "Format 'test' not found");

        let err2 = FormatError::SerializationError("error".to_string());
        assert_eq!(format!("{err2}"), "Serialization error: error");
    }

    #[test]
    fn test_registry_replace_formatter() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormatter);
        registry.register(TestFormatter); // Replace

        assert_eq!(registry.list_formats().len(), 1);
    }
}
