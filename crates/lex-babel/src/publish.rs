//! Document publishing pipeline.
//!
//! Provides a high-level API for converting Lex documents to output formats.
//! This module bridges the gap between the format registry and file I/O,
//! handling both in-memory and file-based output.
//!
//! Use this for editor commands like "Export to PDF" or "Convert to Markdown"
//! where you want a single function call that handles format selection,
//! serialization, and optional file writing.
//!
//! For more control over the conversion process, use [`FormatRegistry`] directly.

use crate::error::FormatError;
use crate::format::SerializedDocument;
use crate::registry::FormatRegistry;
use lex_core::lex::ast::Document;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Specifies how to publish a document.
///
/// Use the builder pattern to configure the publication:
///
/// ```ignore
/// let spec = PublishSpec::new(&document, "html")
///     .with_output_path("output.html")
///     .with_option("theme", "modern");
/// ```
///
/// If no output path is provided, text formats return in-memory content.
/// Binary formats (like PDF) require an explicit output path.
#[derive(Debug)]
pub struct PublishSpec<'a> {
    /// The parsed Lex document to convert.
    pub document: &'a Document,
    /// Target format name (e.g., "html", "markdown", "pdf").
    pub format: &'a str,
    /// Optional file path for writing output. Required for binary formats.
    pub output: Option<PathBuf>,
    /// Format-specific options (e.g., theme selection, page size).
    pub options: HashMap<String, String>,
}

impl<'a> PublishSpec<'a> {
    /// Creates a new publish specification for the given document and format.
    pub fn new(document: &'a Document, format: &'a str) -> Self {
        Self {
            document,
            format,
            output: None,
            options: HashMap::new(),
        }
    }

    /// Sets the output file path. If provided, content is written to disk.
    pub fn with_output_path(mut self, path: impl AsRef<Path>) -> Self {
        self.output = Some(path.as_ref().to_path_buf());
        self
    }

    /// Adds a format-specific option (e.g., theme, page size).
    pub fn with_option(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }
}

/// The output from a successful publish operation.
#[derive(Debug, Clone, PartialEq)]
pub enum PublishArtifact {
    /// Content held in memory (for text formats without an output path).
    InMemory(String),
    /// Path to the written file (when output path was specified).
    File(PathBuf),
}

/// Result of a publish operation.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishResult {
    /// The published artifact (in-memory content or file path).
    pub artifact: PublishArtifact,
}

/// Publishes a document according to the specification.
///
/// Uses the default format registry to find the appropriate serializer.
/// For text formats, returns in-memory content unless an output path is specified.
/// For binary formats (like PDF), requires an output path.
///
/// # Errors
///
/// Returns [`FormatError`] if:
/// - The format is not supported
/// - Serialization fails
/// - File I/O fails
/// - A binary format is requested without an output path
pub fn publish(spec: PublishSpec<'_>) -> Result<PublishResult, FormatError> {
    let registry = FormatRegistry::with_defaults();
    let serialized = registry.serialize_with_options(spec.document, spec.format, &spec.options)?;
    match serialized {
        SerializedDocument::Text(text) => write_or_return_text(text, spec.output),
        SerializedDocument::Binary(bytes) => write_binary(bytes, spec.output),
    }
}

fn write_or_return_text(
    text: String,
    output: Option<PathBuf>,
) -> Result<PublishResult, FormatError> {
    if let Some(path) = output {
        write_to_path(path, text.into_bytes()).map(|path| PublishResult {
            artifact: PublishArtifact::File(path),
        })
    } else {
        Ok(PublishResult {
            artifact: PublishArtifact::InMemory(text),
        })
    }
}

fn write_binary(bytes: Vec<u8>, output: Option<PathBuf>) -> Result<PublishResult, FormatError> {
    let path = output.ok_or_else(|| {
        FormatError::SerializationError(
            "binary formats require an explicit output path".to_string(),
        )
    })?;
    write_to_path(path, bytes).map(|path| PublishResult {
        artifact: PublishArtifact::File(path),
    })
}

fn write_to_path(path: PathBuf, bytes: Vec<u8>) -> Result<PathBuf, FormatError> {
    fs::write(&path, &bytes)
        .map(|_| path.clone())
        .map_err(|err| FormatError::SerializationError(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::parsing;
    use tempfile::tempdir;

    const SAMPLE: &str = "Title:\n\nParagraph text.\n";

    fn sample_document() -> Document {
        parsing::parse_document(SAMPLE).unwrap()
    }

    #[test]
    fn publishes_to_memory_when_no_output_path() {
        let doc = sample_document();
        let result = publish(PublishSpec::new(&doc, "html")).expect("publish");
        match result.artifact {
            PublishArtifact::InMemory(content) => {
                assert!(content.contains("Paragraph text."));
            }
            PublishArtifact::File(_) => panic!("expected in-memory artifact"),
        }
    }

    #[test]
    fn writes_to_disk_when_output_path_provided() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output.html");
        let doc = sample_document();
        let result =
            publish(PublishSpec::new(&doc, "html").with_output_path(&path)).expect("publish");
        match result.artifact {
            PublishArtifact::File(p) => assert_eq!(p, path),
            PublishArtifact::InMemory(_) => panic!("expected file artifact"),
        }
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains("Paragraph text."));
    }
}
