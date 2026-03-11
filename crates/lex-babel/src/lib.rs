//! Multi-format interoperability for Lex documents
//!
//!     This crate provides a uniform interface for converting between Lex AST and various document
//!     formats (Markdown, HTML, Pandoc JSON, etc.).
//!
//!     TLDR: For format authors:
//!         - Babel never parses or serializes any format, but instead relies on the format's libraries
//!         - The convertion should be by converting to the IR, running the common code in common if releveant (it usually is), then to the ast of the target format.
//!         - We should use the testing harness see (lex-parser/src/lex/testing.rs) to load documents and process them into asts.
//!         - Each element should use the harness above and the available file for isolated elements testings with unit tests (load with the lib, assert with ast / ir)
//!         - Each format should have trifecta unit tested in from and to formats to lex.
//!         - Each format should have a kitchensink unit tested in from and to formats to lex
//!         - Read the README.lex the full details)
//!
//! Architecture
//!
//!     The goal here is to, as much as possible, split what is the common logic for multiple formats
//!     conversions into a format agnoistic layer. This is done by the using the IR representation (./ir/mod.rs),
//!     and having the common code in ./common/mod.rs. This allows for the format specific code to be focused on the data format transformations, while having a strong, focused core that can be well tested in isolation.
//!
//!     This is a pure lib, that is , it powers the lex-cli but is shell agnostic, that is no code
//!     should be written that supposes a shell environment, be it to std print, env vars etc.

//!
//!     The file structure :
//!     .
//!     ├── error.rs
//!     ├── format.rs               # Format trait definition
//!     ├── registry.rs             # FormatTregistry for discovery and selection
//!     ├── formats
//!     │   ├── <format>
//!     │   │   ├── parser.rs       # Parser implementation
//!     │   │   ├── serializer.rs   # Serializer implementation
//!     │   │   └── mod.rs
//!     |   ├─  interop             # Shared conversion utilities
//!     ├── lib.rs
//!     ├── ir                      # Intermediate Representation
//!     ├── common                # Common mapping code
//!
//! Testing   
//!     tests
//!     └── <format>
//!         ├── <testname>.rs
//!         └── fixtures
//!         ├── <docname>.<format>
//!         ├── kitchensink.html
//!         ├── kitchensink.lex
//!         └── kitchensink.md
//!
//!     Note that rust does not by default discover tests in subdirectories, so we need to include these
//!     in the mod.
//!
//!
//! Core Algorithms
//!
//!     The most complex part of the work is reconstructing a nested representation from a flat document, followed by the reverse operations.  For this reason we have a common IR (./ir/mod.rs) that is used for all formats.
//!     Over this representation we implement both algorithms (see ./common/flat_to_nested.rs and ./common/nested_to_flat.rs).
//!     This means that all the heavy lifting is done by a core, well tested and maintained module,
//! freeing format adaptations to be focused on the simpler data format transformations.
//!
//!
//! Formats
//!
//!     Format specific capabilities are implemented with the Format trait. formats should have a
//!     parse() and serialize() method, a name and file extensions. See the trait def [./format.rs ]
//!     - Format trait: Uniform interface for all formats (parsing and/or serialization)
//!     - FormatRegistry: Centralized discovery and selection of formats
//!     - Format implementations: Concrete implementations for each supported format
//!
//!
//! The Lex Format
//!
//!     The Lex format itself is implemented as a format, see ./formats/lex/mod.rs, which allows for
//!     a homogeneous API where all formats have identical interfaces:
//!
//!     Note that Lex is a more expressive format than most, which means that converting from is
//!     simple , but always lossy. In particular converting from requires some cosnideartion on how
//!     to best represent the author's intent.
//!
//!     This means that full format interop round tripping is not possible.
//!
//! Format Selection
//!
//!     The choice for the formats is pretty sensible:
//!
//!     - HTML Output: should be self arguing, as it's the most common format for publishing and viewing.
//!     - Markdown: both in and to, as Mardown is the universal format for plain text editing.
//!     - XML: serializing Lex's is trivial and can be useful as a structured format for storage.
//!
//!     These are table stakes, that is a format that can't export to HTML, convert to markdown or
//! lack a good semantic pure xml output is a non starter.
//!
//!
//!     For everything else, there is good arguments for a variety of formats. The one that has the strongest fit
//!  and use case is Latex, as Lex can be very useful for scientific writing. But latex is
//!  complicated, and having pandoc in the pipeline allows us to serve reasonably well pretty much
//!  any other format.
//!
//! Library Choices
//!
//!     This, not being lex's core means that we will offload as much as possible to better, specialized creates
//!  for each format. the escope here is mainly to adapt the ast's from lex to the format or vice
//!  versa. For example we never write the serializer for , say markdown, but pass the AST to the
//!     mardown library. To support a format inbound, we write the format ast -> lex ast adapter.
//!  likewise, for outbound formats we will do the reverse, converting from the lex ast to the
//!  format's.
//!
//!     As much as possible, we will use rust crates, and avoid shelling out and having outside dependencies.
//!
pub mod error;
pub mod format;
pub mod formats;
pub mod publish;
pub mod registry;
pub mod templates;
pub mod transforms;

pub mod common;
pub mod ir;

pub use error::FormatError;
pub use format::{Format, SerializedDocument};
pub use registry::FormatRegistry;

/// Converts a lex document to the Intermediate Representation (IR).
///
/// # Information Loss
///
/// The IR is a simplified, semantic representation. The following
/// Lex information is lost during conversion:
/// - Blank line grouping (BlankLineGroup nodes)
/// - Source positions and token information
/// - Comment annotations at document level
///
/// For lossless Lex representation, use the AST directly.
pub fn to_ir(doc: &lex_core::lex::ast::elements::Document) -> ir::nodes::Document {
    ir::from_lex::from_lex_document(doc)
}

/// Converts an IR document back to Lex AST.
///
/// This is useful for round-trip conversions: Format → IR → Lex.
pub fn from_ir(doc: &ir::nodes::Document) -> lex_core::lex::ast::elements::Document {
    ir::to_lex::to_lex_document(doc)
}
