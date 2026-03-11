//! AST definitions and utilities for the lex format
//!
//!     This module provides the core Abstract Syntax Tree (AST) definitions,
//!     along with utilities for working with AST nodes, tracking source positions,
//!     and performing position-based lookups.
//!
//! Document and Sessions
//!
//!     Lex documents are plain text, utf-8 encoded files with the file extension .lex. Line width
//!     is not limited, and is considered a presentation detail. Best practice dictates only
//!     limiting line length when publishing, not while authoring.
//!
//!     The document node holds the document metadata and the content's root node, which is a
//!     session node. The structure of the document then is a tree of sessions, which can be nested
//!     arbitrarily. This creates powerful addressing capabilities as one can target any sub-session
//!     from an index.
//!
//!     See [Document](elements::Document) for the document node definition, and [Session](elements::Session)
//!     for session nodes.
//!
//! Nesting
//!
//!     The ability to make deep structures is core to Lex, and this is reflected throughout the
//!     grammar. In fact the only element that does not contain children is the paragraph and the
//!     verbatim block (by definition content that is not parsed).
//!
//!     Nesting is pretty unrestricted with the following logical exceptions:
//!
//!         - Only sessions can contain other sessions: you don't want a session popping up in the
//!           middle of a list item.
//!         - Annotations (metadata) cannot host inner annotations, that is you can't have metadata
//!           on metadata (pretty reasonable, no?).
//!
//!     This nesting structure is enforced at compile time through type-safe containers. Containers
//!     such as Session, Definition, and Annotation take typed vectors (SessionContent,
//!     ContentElement, etc.) so invalid nesting is ruled out at compile time. See the
//!     [container](elements::container) module for details.
//!
//!     For more details on element types, how they structure content, and the relationship between
//!     indentation and the AST, see the [elements](elements) module.
//!
//! ## How Location Tracking Works in lex
//!
//! Location tracking flows through the entire compilation pipeline from raw source code to AST nodes.
//!
//! ### 1. Tokenization (Lexer)
//!
//! The lexer produces tokens paired with byte-offset ranges into the source:
//!
//! ```text
//! Source: "Hello\nWorld"
//!         012345678901
//!                  ↓
//! Lexer: (Token::Text("Hello"), 0..5)
//!        (Token::Newline, 5..6)
//!        (Token::Text("World"), 6..11)
//! ```
//!
//! The lexer pipeline applies transformations while preserving byte ranges:
//! - Whitespace processing - removes tokens, preserves ranges
//! - Indentation transformation - converts to semantic Indent/Dedent tokens
//! - Blank line transformation - aggregates multiple Newlines
//!
//! ### 2. Byte-to-Line Conversion
//!
//! Before building AST nodes, byte ranges are converted to line:column positions
//! using `SourceLocation` (one-time setup, O(log n) per conversion):
//!
//! ```text
//! SourceLocation pre-computes line starts: [0, 6]
//! byte_to_position(8) → binary search → Position { line: 1, column: 2 }
//! ```
//!
//! ### 3. Parser (AST Construction)
//!
//! The parser builds AST nodes with `Range` objects via bottom-up construction:
//! 1. Parse child elements (which have locations)
//! 2. Convert byte ranges to `Range` objects
//! 3. Aggregate child locations via `compute_location_from_locations()`
//! 4. Create parent node with aggregated location (bounding box)
//!
//! See `src/lex/building/location.rs` for the canonical implementations.
//!
//! ### 4. Complete Document Structure
//!
//! The final document has location information at every level - every element
//! knows its exact position in the source (start line:column to end line:column).
//!
//! ## Modules
//!
//! - `range` - Position and Range types for source code locations
//! - `elements` - AST node type definitions organized by element type
//! - `traits` - Common traits for AST nodes (AstNode, Container, TextNode, Visitor)
//! - `lookup` - Position-based AST node lookup functionality
//! - `snapshot` - Normalized intermediate representation for serialization
//! - `error` - Error types for AST operations
//!
//! ## Type-Safe Containers
//!
//! Containers such as `Session`, `Definition`, and `Annotation` now take typed
//! vectors (`SessionContent`, `ContentElement`, etc.) so invalid nesting is ruled
//! out at compile time. See `docs/architecture/type-safe-containers.md` for
//! details and compile-fail examples.

pub mod diagnostics;
pub mod elements;
pub mod error;
pub mod links;
pub mod range;
pub mod snapshot;
pub mod text_content;
pub mod trait_helpers;
pub mod traits;

// Re-export commonly used types at module root
pub use diagnostics::{validate_references, validate_structure, Diagnostic, DiagnosticSeverity};
pub use elements::{
    Annotation, ContentItem, Data, Definition, Document, Label, List, ListItem, Paragraph,
    Parameter, Session, TextLine, Verbatim,
};
pub use error::PositionLookupError;
pub use links::{DocumentLink, LinkType};
pub use range::{Position, Range, SourceLocation};
pub use snapshot::{
    snapshot_from_content, snapshot_from_content_with_options, snapshot_from_document,
    snapshot_from_document_with_options, snapshot_node, AstSnapshot,
};
pub use text_content::TextContent;
pub use traits::{AstNode, Container, TextNode, Visitor, VisualStructure};

// Convenience functions that delegate to Document methods
// These are provided for backwards compatibility with existing code

/// Find nodes at a given position in the document
///
/// This is a convenience wrapper around `Document::find_nodes_at_position()`.
/// Returns a vector containing the deepest AST node at the given position.
#[inline]
pub fn find_nodes_at_position(document: &Document, position: Position) -> Vec<&dyn AstNode> {
    document.root.find_nodes_at_position(position)
}

/// Find the path of nodes at a given position in the document
///
/// Returns a vector containing the path of AST nodes from root to the deepest node at the given position.
#[inline]
pub fn find_node_path_at_position(document: &Document, position: Position) -> Vec<&dyn AstNode> {
    document.node_path_at_position(position)
}

/// Format information about nodes at a given position
///
/// This is a convenience wrapper around `Document::format_at_position()`.
/// Returns a formatted string describing the AST nodes at the given position.
#[inline]
pub fn format_at_position(document: &Document, position: Position) -> String {
    document.root.format_at_position(position)
}
