//! Format registry for format discovery and selection
//!
//! This module provides a centralized registry for all available formats.
//! Formats can be registered and retrieved by name.

use crate::error::FormatError;
use crate::format::{Format, SerializedDocument};
use lex_core::lex::ast::Document;
use std::collections::HashMap;

/// Registry of document formats
///
/// Provides a centralized registry for all available formats.
/// Formats can be registered and retrieved by name.
///
/// # Examples
///
/// ```ignore
/// let mut registry = FormatRegistry::new();
/// registry.register(MyFormat);
///
/// let format = registry.get("my-format")?;
/// let doc = format.parse("source text")?;
/// ```
pub struct FormatRegistry {
    formats: HashMap<String, Box<dyn Format>>,
}

impl FormatRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        FormatRegistry {
            formats: HashMap::new(),
        }
    }

    /// Register a format
    ///
    /// If a format with the same name already exists, it will be replaced.
    pub fn register<F: Format + 'static>(&mut self, format: F) {
        self.formats
            .insert(format.name().to_string(), Box::new(format));
    }

    /// Get a format by name
    pub fn get(&self, name: &str) -> Result<&dyn Format, FormatError> {
        self.formats
            .get(name)
            .map(|f| f.as_ref())
            .ok_or_else(|| FormatError::FormatNotFound(name.to_string()))
    }

    /// Check if a format exists
    pub fn has(&self, name: &str) -> bool {
        self.formats.contains_key(name)
    }

    /// List all available format names (sorted)
    pub fn list_formats(&self) -> Vec<String> {
        let mut names: Vec<_> = self.formats.keys().cloned().collect();
        names.sort();
        names
    }

    /// Detect format from filename based on file extension
    ///
    /// Returns the format name if a matching extension is found, or None otherwise.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let registry = FormatRegistry::default();
    /// assert_eq!(registry.detect_format_from_filename("doc.lex"), Some("lex".to_string()));
    /// assert_eq!(registry.detect_format_from_filename("doc.md"), Some("markdown".to_string()));
    /// assert_eq!(registry.detect_format_from_filename("doc.unknown"), None);
    /// ```
    pub fn detect_format_from_filename(&self, filename: &str) -> Option<String> {
        // Extract extension from filename
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|ext| ext.to_str())?;

        // Search for a format that supports this extension
        for format in self.formats.values() {
            if format.file_extensions().contains(&extension) {
                return Some(format.name().to_string());
            }
        }

        None
    }

    /// Parse source text using the specified format
    pub fn parse(&self, source: &str, format: &str) -> Result<Document, FormatError> {
        let fmt = self.get(format)?;
        if !fmt.supports_parsing() {
            return Err(FormatError::NotSupported(format!(
                "Format '{format}' does not support parsing"
            )));
        }
        fmt.parse(source)
    }

    /// Serialize a document using the specified format
    pub fn serialize(&self, doc: &Document, format: &str) -> Result<String, FormatError> {
        let empty = HashMap::new();
        match self.serialize_with_options(doc, format, &empty)? {
            SerializedDocument::Text(text) => Ok(text),
            SerializedDocument::Binary(_) => Err(FormatError::SerializationError(format!(
                "Format '{format}' produced binary output when text was expected"
            ))),
        }
    }

    /// Serialize a document using the specified format and options
    pub fn serialize_with_options(
        &self,
        doc: &Document,
        format: &str,
        options: &HashMap<String, String>,
    ) -> Result<SerializedDocument, FormatError> {
        let fmt = self.get(format)?;
        if !fmt.supports_serialization() {
            return Err(FormatError::NotSupported(format!(
                "Format '{format}' does not support serialization"
            )));
        }
        fmt.serialize_with_options(doc, options)
    }

    /// Create a registry with default formats
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Register built-in formats
        registry.register(crate::formats::lex::LexFormat::default());
        registry.register(crate::formats::html::HtmlFormat::default());
        #[cfg(feature = "native-export")]
        registry.register(crate::formats::pdf::PdfFormat::default());
        #[cfg(feature = "native-export")]
        registry.register(crate::formats::png::PngFormat::default());
        registry.register(crate::formats::markdown::MarkdownFormat);
        registry.register(crate::formats::rfc_xml::RfcXmlFormat);
        registry.register(crate::formats::tag::TagFormat);
        registry.register(crate::formats::treeviz::TreevizFormat);
        registry.register(crate::formats::linetreeviz::LinetreevizFormat);

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
    use crate::format::Format;
    use lex_core::lex::ast::{ContentItem, Document, Paragraph};

    // Test format
    struct TestFormat;
    impl Format for TestFormat {
        fn name(&self) -> &str {
            "test"
        }
        fn description(&self) -> &str {
            "Test format"
        }
        fn supports_parsing(&self) -> bool {
            true
        }
        fn supports_serialization(&self) -> bool {
            true
        }
        fn parse(&self, _source: &str) -> Result<Document, FormatError> {
            Ok(Document::with_content(vec![ContentItem::Paragraph(
                Paragraph::from_line("test".to_string()),
            )]))
        }
        fn serialize(&self, _doc: &Document) -> Result<String, FormatError> {
            Ok("test output".to_string())
        }
    }

    #[test]
    fn test_registry_creation() {
        let registry = FormatRegistry::new();
        assert_eq!(registry.formats.len(), 0);
    }

    #[test]
    fn test_registry_register() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormat);

        assert!(registry.has("test"));
        assert_eq!(registry.list_formats(), vec!["test"]);
    }

    #[test]
    fn test_registry_get() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormat);

        let format = registry.get("test");
        assert!(format.is_ok());
        assert_eq!(format.unwrap().name(), "test");
    }

    #[test]
    fn test_registry_get_nonexistent() {
        let registry = FormatRegistry::new();
        let result = registry.get("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_registry_has() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormat);

        assert!(registry.has("test"));
        assert!(!registry.has("nonexistent"));
    }

    #[test]
    fn test_registry_parse() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormat);

        let result = registry.parse("input", "test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_registry_parse_not_found() {
        let registry = FormatRegistry::new();

        let result = registry.parse("input", "nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            FormatError::FormatNotFound(name) => assert_eq!(name, "nonexistent"),
            _ => panic!("Expected FormatNotFound error"),
        }
    }

    #[test]
    fn test_registry_serialize() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormat);

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
    fn test_registry_serialize_with_options_default_behavior() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormat);

        let doc = Document::with_content(vec![ContentItem::Paragraph(Paragraph::from_line(
            "Hello".to_string(),
        ))]);
        let mut options = HashMap::new();
        options.insert("unused".to_string(), "true".to_string());

        let result = registry.serialize_with_options(&doc, "test", &options);
        assert!(result.is_err());
    }

    #[test]
    fn test_registry_list_formats() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormat);

        let formats = registry.list_formats();
        assert_eq!(formats.len(), 1);
        assert_eq!(formats[0], "test");
    }

    #[test]
    fn test_registry_with_defaults() {
        let registry = FormatRegistry::with_defaults();
        assert!(registry.has("lex"));
        assert!(registry.has("markdown"));
        assert!(registry.has("tag"));
        assert!(registry.has("treeviz"));
    }

    #[test]
    fn test_registry_default_trait() {
        let registry = FormatRegistry::default();
        assert!(registry.has("lex"));
        assert!(registry.has("markdown"));
        assert!(registry.has("tag"));
        assert!(registry.has("treeviz"));
    }

    #[test]
    fn test_registry_replace_format() {
        let mut registry = FormatRegistry::new();
        registry.register(TestFormat);
        registry.register(TestFormat); // Replace

        assert_eq!(registry.list_formats().len(), 1);
    }

    #[test]
    fn test_detect_format_from_filename() {
        let registry = FormatRegistry::with_defaults();

        // Test lex extension
        assert_eq!(
            registry.detect_format_from_filename("doc.lex"),
            Some("lex".to_string())
        );
        assert_eq!(
            registry.detect_format_from_filename("/path/to/file.lex"),
            Some("lex".to_string())
        );

        // Test tag extensions
        assert_eq!(
            registry.detect_format_from_filename("doc.tag"),
            Some("tag".to_string())
        );
        assert_eq!(
            registry.detect_format_from_filename("doc.xml"),
            Some("tag".to_string())
        );

        // Test treeviz extensions
        assert_eq!(
            registry.detect_format_from_filename("doc.tree"),
            Some("treeviz".to_string())
        );
        assert_eq!(
            registry.detect_format_from_filename("doc.treeviz"),
            Some("treeviz".to_string())
        );

        // Test unknown extension
        assert_eq!(registry.detect_format_from_filename("doc.unknown"), None);

        // Test no extension
        assert_eq!(registry.detect_format_from_filename("doc"), None);
    }

    #[test]
    fn test_detect_format_case_sensitive() {
        let registry = FormatRegistry::with_defaults();

        // Extensions are case-sensitive by default (as returned by Path::extension())
        assert_eq!(
            registry.detect_format_from_filename("doc.lex"),
            Some("lex".to_string())
        );
        // Note: On some systems, extensions might be case-insensitive
        // but we treat them as case-sensitive for consistency
    }
}
