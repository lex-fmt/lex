//! Parsing module for the lex format
//!
//!     This module provides the complete processing pipeline from source text to AST:
//!         1. Lexing: Tokenization of source text. See [lexing](crate::lex::lexing) module.
//!         2. Analysis: Syntactic analysis to produce IR nodes. See [engine](engine) module.
//!         3. Building: Construction of AST from IR nodes. See [building](crate::lex::building) module.
//!         4. Inline Parsing: Parse inline elements in text content. See [inlines](crate::lex::inlines) module.
//!         5. Assembling: Post-parsing transformations. See [assembling](crate::lex::assembling) module.
//!
//! Parsing End To End
//!
//!     The complete pipeline transforms a string of Lex source up to the final AST through
//!     these stages:
//!
//!         Lexing (5.1):
//!             Tokenization and transformations that group tokens into lines. At the end of
//!             lexing, we have a TokenStream of Line tokens + indent/dedent tokens.
//!
//!         Parsing - Semantic Analysis (5.2):
//!             At the very beginning of parsing we will group line tokens into a tree of
//!             LineContainers. What this gives us is the ability to parse each level in isolation.
//!             Because we don't need to know what a LineContainer has, but only that it is a
//!             line container, we can parse each level with a regular regex. We simply print
//!             token names and match the grammar patterns against them.
//!
//!             When tokens are matched, we create intermediate representation nodes, which carry
//!             only two bits of information: the node matched and which tokens it uses.
//!
//!             This allows us to separate the semantic analysis from the ast building. This is
//!             a good thing overall, but was instrumental during development, as we ran multiple
//!             parsers in parallel and the ast building had to be unified (correct parsing would
//!             result in the same node types + tokens).
//!
//!         AST Building (5.3):
//!             From the IR nodes, we build the actual AST nodes. During this step, important
//!             things happen:
//!                 1. We unroll source tokens so that ast nodes have access to token values.
//!                 2. The location from tokens is used to calculate the location for the ast node.
//!                 3. The location is transformed from byte range to a dual byte range + line:column
//!                    position.
//!             At this stage we create the root session node; it will be attached to the
//!             [`Document`] during assembling.
//!
//!         Inline Parsing (5.4):
//!             Before assembling the document (while annotations are still part of the content
//!             tree), we parse the TextContent nodes for inline elements. This parsing is much
//!             simpler, as it has formal start/end tokens and has no structural elements.
//!
//!         Document Assembly (5.5):
//!             The assembling stage wraps the root session into a document node and performs
//!             metadata attachment. Annotations, which are metadata, are always attached to AST
//!             nodes, so they can be very targeted. Only with the full document in place we can
//!             attach annotations to their correct target nodes. This is harder than it seems.
//!             Keeping Lex ethos of not enforcing structure, this needs to deal with several
//!             ambiguous cases, including some complex logic for calculating "human
//!             understanding" distance between elements.
//!
//! Terminology
//!
//!     - parse: Colloquial term for the entire process (lexing + analysis + building)
//!     - analyze/analysis: The syntactic analysis phase specifically
//!     - build: The AST construction phase specifically
//!
//! Testing
//!
//!     All parser tests must follow strict guidelines. See the [testing module](crate::lex::testing)
//!     for comprehensive documentation on using verified lex sources and AST assertions.

// Parser implementations
pub mod common;
pub mod engine;
pub mod ir;
pub mod parser;

// Re-export common parser interfaces
pub use common::{ParseError, ParserInput};

// Re-export AST types and utilities from the ast module
pub use crate::lex::ast::{
    format_at_position, Annotation, AstNode, Container, ContentItem, Definition, Document, Label,
    List, ListItem, Paragraph, Parameter, Position, Range, Session, SourceLocation, TextNode,
    Verbatim,
};

pub use crate::lex::formats::{serialize_ast_tag, to_treeviz_str};
/// Type alias for processing results returned by helper APIs.
type ProcessResult = Result<Document, String>;

/// Process source text through the complete pipeline: lex, analyze, and build.
///
/// This is the primary entry point for processing lex documents. It performs:
/// 1. Lexing: Tokenizes the source text
/// 2. Analysis: Performs syntactic analysis to produce IR nodes
/// 3. Building: Constructs the root session tree from IR nodes (assembling wraps it in a
///    `Document` and attaches metadata)
///
/// # Arguments
///
/// * `source` - The source text to process
///
/// # Returns
///
/// A `Document` containing the complete AST, or parsing errors.
///
/// # Example
///
/// ```rust,ignore
/// use lex::lex::parsing::process_full;
///
/// let source = "Hello world\n";
/// let document = process_full(source)?;
/// ```
pub fn process_full(source: &str) -> ProcessResult {
    use crate::lex::transforms::standard::STRING_TO_AST;
    STRING_TO_AST
        .run(source.to_string())
        .map_err(|e| e.to_string())
}

/// Alias for `process_full` to maintain backward compatibility.
///
/// The term "parse" colloquially refers to the entire processing pipeline
/// (lexing + analysis + building), even though technically parsing is just
/// the syntactic analysis phase.
pub fn parse_document(source: &str) -> ProcessResult {
    process_full(source)
}
