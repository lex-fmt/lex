//! Test harness for per-element testing
//!
//!     This module provides utilities for testing individual element variations using the
//!     per-element library in `comms/specs/elements/`.
//!
//! The Spec Sample Files
//!
//!     For this reason, all testing must use the official sample files, which are vetted,
//!     curated and reviewed during spec changes. Of course this does not apply to the string
//!     form only, but for tokens and any intermediary processed formats. If we can't reliably
//!     come up with the string form, never mind with that string after tokenization and
//!     processed.
//!
//!     Here is where Lexplore comes in. It includes a loader that will load from the official
//!     sample files and can return the data in various formats: string, tokens, line container,
//!     ir nodes and ast nodes.
//!
//!     The sample files are organized:
//!
//!         - By elements:
//!             - Isolated elements (only the element itself): Individual test cases for each
//!               element variation
//!             - In Document: mixed with other elements: Integration test cases showing how
//!               elements interact
//!         - Benchmark: full documents that are used to test the parser performance and
//!           correctness on complex real-world documents
//!         - Trifecta: a mix of sessions, paragraphs and lists, the structural elements.
//!           These test the core structural elements together
//!
//!     These come with handy functions to load them, get the isolated element ast node and more.
//!
//! Module Organization
//!
//!     - `loader`: File loading, parsing, and tokenization infrastructure. See [loader](loader).
//!     - `extraction`: AST node extraction and assertion helpers. See [extraction](extraction).
//!     - `specfile_finder`: File discovery and path resolution. See [specfile_finder](specfile_finder).
//!
//! Usage Examples
//!
//!     Fluent API (Recommended):
//!
//!     ```rust,ignore
//!     use crate::lex::testing::lexplore::Lexplore;
//!
//!     // Load and parse an element - returns DocumentLoader for chaining transforms
//!     let doc = Lexplore::paragraph(1).parse().unwrap();
//!     let paragraph = doc.root.expect_paragraph();
//!
//!     // Load and tokenize
//!     let tokens = Lexplore::paragraph(1).tokenize().unwrap();
//!
//!     // Get source only
//!     let source = Lexplore::list(1).source();
//!
//!     // Load documents
//!     let doc = Lexplore::benchmark(10).parse().unwrap();
//!     let doc = Lexplore::trifecta(0).parse().unwrap();
//!
//!     // Load from arbitrary paths
//!     let doc = Lexplore::from_path("path/to/file.lex").parse().unwrap();
//!     ```
//!
//!     Direct Element Access:
//!
//!     ```rust,ignore
//!     use crate::lex::testing::lexplore::Lexplore;
//!
//!     // Get the AST node directly (convenience for simple tests)
//!     let paragraph = Lexplore::get_paragraph(1);
//!     let list = Lexplore::get_list(1);
//!     let session = Lexplore::get_session(1);
//!     let definition = Lexplore::get_definition(1);
//!     let annotation = Lexplore::get_annotation(1);
//!     let verbatim = Lexplore::get_verbatim(1);
//!     ```
//!
//!     Available Element Types:
//!
//!         - `Lexplore::paragraph(n)` - Load paragraph element #n
//!         - `Lexplore::list(n)` - Load list element #n
//!         - `Lexplore::session(n)` - Load session element #n
//!         - `Lexplore::definition(n)` - Load definition element #n
//!         - `Lexplore::annotation(n)` - Load annotation element #n
//!         - `Lexplore::verbatim(n)` - Load verbatim element #n
//!
//!     Available Document Types:
//!
//!         - `Lexplore::benchmark(n)` - Load benchmark document #n
//!         - `Lexplore::trifecta(n)` - Load trifecta document #n
//!
//!     For complete API details, see the [loader](loader) module.

mod extraction;
mod loader;
pub mod specfile_finder;

// Re-export everything public from submodules
pub use extraction::*;
pub use loader::*;
