//! Markdown format implementation
//!
//! This module implements bidirectional conversion between Lex and CommonMark Markdown.
//!
//! # Library Choice
//!
//! We use the `comrak` crate for Markdown parsing and serialization. This choice is based on:
//! - Single crate for both parsing and serialization
//! - Feature-rich with CommonMark compliance
//! - Robust and well-maintained
//! - Supports extensions (tables, strikethrough, etc.)
//!
//! # Element Mapping Table
//!
//! Complete Lex ↔ Markdown Mapping:
//!
//! | Lex Element      | Markdown Equivalent     | Export Notes                           | Import Notes                          |
//! |------------------|-------------------------|----------------------------------------|---------------------------------------|
//! | Session          | Heading (# ## ###)      | Session level → heading level (1-6)    | Heading level → session nesting       |
//! | Paragraph        | Paragraph               | Direct mapping                         | Direct mapping                        |
//! | List             | List (- or 1. 2. 3.)    | Ordered/unordered preserved            | Detect type from first item marker    |
//! | ListItem         | List item (- item)      | Direct mapping with nesting            | Direct mapping with nesting           |
//! | Definition       | **Term**: Description   | Bold term + colon + content            | Parse bold + colon pattern            |
//! | Verbatim         | Code block (```)        | Language → info string                 | Info string → language                |
//! | Annotation       | HTML comment            | `<!-- lex:label key=val -->` format    | Not implemented (annotations lost)    |
//! | InlineContent:   |                         |                                        |                                       |
//! |   Text           | Plain text              | Direct                                 | Direct                                |
//! |   Bold           | **bold** or __bold__    | Use **                                 | Parse both                            |
//! |   Italic         | *italic* or _italic_    | Use *                                  | Parse both                            |
//! |   Code           | `code`                  | Direct                                 | Direct                                |
//! |   Math           | $math$ or $$math$$      | Use $...$                              | Parse if extension enabled            |
//! |   Reference      | \[text\]                | Plain text (Lex refs are citations)    | Parse link/reference syntax           |
//!
//! # Lossy Conversions
//!
//! The following conversions lose information on round-trip:
//! - Lex sessions beyond level 6 → h6 with nested content (Markdown max is h6)
//! - Lex annotations → HTML comments (exported but not parsed on import)
//! - Lex definition structure → bold text pattern (not native Markdown)
//! - Lex references → plain text (citations, not URLs)
//! - Multiple blank lines → single blank line (Markdown normalization)
//! - Verbatim post-wall indentation → lost (see issue #276)
//!
//! # Architecture Notes
//!
//! There is a fundamental mismatch between Markdown's flat model and Lex's hierarchical structure.
//! We leverage the IR event system (lex-babel/src/common/) to handle the nested-to-flat and
//! flat-to-nested conversions. This keeps format-specific code focused on Markdown AST transformations.
//!
//! Lists are the only Markdown element that are truly nested, making them straightforward to map.
//!
//! # Testing
//!
//! Export tests use Lex spec files from specs/v1/elements/ for isolated element testing.
//! Integration tests use the kitchensink benchmark and a CommonMark reference document.
//! See the testing guide in docs/local/tasks/86-babel-markdown.lex for details.
//!
//! # Implementation Status
//!
//! - [x] Export (Lex → Markdown)
//!   - [x] Paragraph
//!   - [x] Heading (Session) - nested sessions → flat heading hierarchy
//!   - [x] Bold, Italic, Code inlines
//!   - [x] Lists - ordered/unordered detection, tight formatting
//!   - [x] Code blocks (Verbatim)
//!   - [x] Definitions - term paragraph + description siblings
//!   - [x] Annotations - as HTML comments with content
//!   - [x] Math - rendered as $...$ text
//!   - [x] References - rendered as plain text citations
//! - [x] Import (Markdown → Lex)
//!   - [x] Paragraph
//!   - [x] Heading → Session (flat heading hierarchy → nested sessions)
//!   - [x] Bold, Italic, Code inlines
//!   - [x] Lists
//!   - [x] Code blocks → Verbatim
//!   - [x] Annotations (HTML comment parsing)
//!   - [x] Definitions (pattern matching)

pub mod parser;
pub mod serializer;

use crate::error::FormatError;
use crate::format::Format;
use lex_core::lex::ast::Document;

/// Format implementation for Markdown
pub struct MarkdownFormat;

impl Format for MarkdownFormat {
    fn name(&self) -> &str {
        "markdown"
    }

    fn description(&self) -> &str {
        "CommonMark Markdown format"
    }

    fn file_extensions(&self) -> &[&str] {
        &["md", "markdown"]
    }

    fn supports_parsing(&self) -> bool {
        true
    }

    fn supports_serialization(&self) -> bool {
        true
    }

    fn parse(&self, source: &str) -> Result<Document, FormatError> {
        parser::parse_from_markdown(source)
    }

    fn serialize(&self, doc: &Document) -> Result<String, FormatError> {
        serializer::serialize_to_markdown(doc)
    }
}
