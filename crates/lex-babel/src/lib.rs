//! Multi-format interoperability for Lex documents
//!
//!     This crate provides a uniform interface for converting between Lex AST and various document
//!     formats (Markdown, HTML, Pandoc JSON, etc.).
//!
//!     TLDR: For format authors:
//!         - Babel never parses or serializes any format, but instead relies on the format's libraries
//!         - The conversion should be by converting to the IR, running the common code in common if relevant (it usually is), then to the AST of the target format.
//!         - We should use the testing harness (see lex-core/src/lex/testing.rs) to load documents and process them into ASTs.
//!         - Each element should use the harness above and the available file for isolated element testing with unit tests (load with the lib, assert with AST / IR)
//!         - Each format should have trifecta unit tested in from and to formats to lex.
//!         - Each format should have a kitchensink unit tested in from and to formats to lex
//!         - Read the README.lex for full details
//!
//! Architecture
//!
//!     The goal here is to, as much as possible, split what is the common logic for multiple formats
//!     conversions into a format agnostic layer. This is done by using the IR representation (./ir/mod.rs),
//!     and having the common code in ./common/mod.rs. This allows for the format specific code to be focused on the data format transformations, while having a strong, focused core that can be well tested in isolation.
//!
//!     This is a pure lib, that is, it powers the lexd CLI but is shell agnostic, that is no code
//!     should be written that supposes a shell environment, be it to std print, env vars etc.
//!
//!     The file structure:
//!     .
//!     ├── error.rs
//!     ├── format.rs               # Format trait definition
//!     ├── registry.rs             # FormatRegistry for discovery and selection
//!     ├── formats
//!     │   └── <format>
//!     │       ├── parser.rs       # Parser implementation
//!     │       ├── serializer.rs   # Serializer implementation
//!     │       └── mod.rs
//!     ├── lib.rs
//!     ├── ir                      # Intermediate Representation
//!     │   ├── nodes.rs            # IR data model (DocNode, InlineContent, etc.)
//!     │   ├── events.rs           # Flat event stream representation
//!     │   ├── from_lex.rs         # Lex AST → IR conversion
//!     │   └── to_lex.rs           # IR → Lex AST conversion
//!     └── common                  # Common mapping code
//!         ├── flat_to_nested.rs   # Events → IR tree (with auto-closing)
//!         ├── nested_to_flat.rs   # IR tree → Events
//!         └── verbatim/           # Verbatim handler registry (doc.table, doc.image, etc.)
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
//!     The most complex part of the work is reconstructing a nested representation from a flat document, followed by the reverse operations. For this reason we have a common IR (./ir/mod.rs) that is used for all formats.
//!     Over this representation we implement both algorithms (see ./common/flat_to_nested.rs and ./common/nested_to_flat.rs).
//!     This means that all the heavy lifting is done by a core, well tested and maintained module,
//!     freeing format adaptations to be focused on the simpler data format transformations.
//!
//!
//! Formats
//!
//!     Format specific capabilities are implemented with the Format trait. Formats should have a
//!     parse() and serialize() method, a name and file extensions. See the trait def [./format.rs]
//!     - Format trait: Uniform interface for all formats (parsing and/or serialization)
//!     - FormatRegistry: Centralized discovery and selection of formats
//!     - Format implementations: Concrete implementations for each supported format
//!
//!
//! The Lex Format
//!
//!     The Lex format itself is implemented as a format, see ./formats/lex/mod.rs, which allows for
//!     a homogeneous API where all formats have identical interfaces.
//!
//!     Note that Lex is a more expressive format than most, which means that converting from Lex is
//!     simple, but always lossy. In particular converting to Lex requires some consideration on how
//!     to best represent the author's intent.
//!
//!     This means that full format interop round tripping is not possible.
//!
//! v1 Interop Scope
//!
//!     The full contributor-facing version of this lives in
//!     `comms/docs/interop-scope.lex` (browse on GitHub:
//!     <https://github.com/lex-fmt/comms/blob/main/docs/interop-scope.lex>).
//!     The short version:
//!
//!     | Tier              | Format     | Export | Import | Notes                                                |
//!     |-------------------|------------|--------|--------|------------------------------------------------------|
//!     | Core              | Markdown   | ✓      | ✓      | Lingua franca. Round-trip is the bar.                |
//!     | Core              | HTML       | ✓      | —      | Publishing target; PDF and editor previews consume.  |
//!     | Core              | PDF        | ✓      | —      | Headless Chrome over the HTML output.                |
//!     | Core              | PNG        | ✓      | —      | Headless Chrome screenshot of the HTML output.       |
//!     | Stretch           | HTML       | —      | ✓      | After core lands.                                    |
//!     | Experimental      | RFC XML    | —      | ✓      | Proof-of-concept; no bespoke investment.             |
//!     | Planned           | Pandoc     | —      | —      | Bridge to DOCX/EPUB/RST/Org/etc. Not started.        |
//!     | Planned           | LaTeX      | ✓      | —      | Export only, via Pandoc once Pandoc lands.           |
//!     | Category error    | PDF import | —      | —      | **Will not be implemented.** See below.              |
//!
//!     **PDF import is a category error, not a postponed task.** PDF is a
//!     presentation format — paragraphs, headings, and lists are
//!     reconstructed heuristically from layout (font size, indentation,
//!     glyph positions), and no rule-based importer recovers that
//!     reliably. Pandoc punts to `pdftotext` for the same reason. This
//!     will not be implemented, ever, regardless of adoption. ML-based
//!     extraction (Marker, Nougat, Mathpix, Grobid) is a different
//!     product category and out of scope for lex-babel. Do not list,
//!     advertise, or design around the possibility.
//!
//!     Diagnostic outputs (`tag`, `treeviz`, `linetreeviz`) sit outside
//!     this tiering — they're AST visualizers used by `lexd inspect`,
//!     not interop targets.
//!
//! Library Choices
//!
//!     This, not being Lex's core, means that we will offload as much as possible to better, specialized crates
//!     for each format. The scope here is mainly to adapt the ASTs from Lex to the format or vice
//!     versa. For example we never write the serializer for, say, Markdown, but pass the AST to the
//!     Markdown library. To support a format inbound, we write the format AST → Lex AST adapter.
//!     Likewise, for outbound formats we will do the reverse, converting from the Lex AST to the
//!     format's.
//!
//!     As much as possible, we will use Rust crates, and avoid shelling out and having outside dependencies.
//!
pub mod error;
pub mod format;
pub mod formats;
pub mod publish;
pub mod registry;
pub mod render_dispatch;
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
///
/// Uses the shared default registry ([`default_registry()`]) for
/// verbatim-label `on_resolve` dispatch. Callers that need to plug
/// in third-party namespaces (lex-cli with `boot_registry`, lex-lsp,
/// embedders) should call [`to_ir_with_registry`] directly.
pub fn to_ir(doc: &lex_core::lex::ast::elements::Document) -> ir::nodes::Document {
    to_ir_with_registry(doc, default_registry())
}

/// Converts a Lex AST to its IR representation, dispatching verbatim
/// labels through the supplied registry's `on_resolve` hooks.
///
/// This is the explicit-registry variant of [`to_ir`] — use it when
/// the caller has its own registry (with third-party namespaces
/// registered) and wants those handlers to participate in IR
/// construction.
pub fn to_ir_with_registry(
    doc: &lex_core::lex::ast::elements::Document,
    registry: &lex_extension_host::registry::Registry,
) -> ir::nodes::Document {
    ir::from_lex::from_lex_document(doc, registry)
}

/// Process-wide registry with the built-in `lex.*` schemas
/// registered. Used by `to_ir` and `to_lex_document` for callers
/// that don't supply their own. Constructed lazily on first call.
///
/// Filesystem access is plumbed through a no-op loader — this
/// registry doesn't resolve `lex.include` (the to_ir/to_lex paths
/// never invoke `on_resolve` for that label; includes are resolved
/// elsewhere in the pipeline).
pub fn default_registry() -> &'static lex_extension_host::registry::Registry {
    use lex_core::lex::includes::{LoadError, LoadedFile, Loader, ResolveConfig};
    use lex_extension_host::registry::Registry;
    use std::path::Path;
    use std::sync::{Arc, OnceLock};

    struct NoopLoader;
    impl Loader for NoopLoader {
        fn load(&self, path: &Path) -> Result<LoadedFile, LoadError> {
            Err(LoadError::NotFound {
                path: path.to_path_buf(),
            })
        }
    }

    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let registry = Registry::new();
        lex_core::lex::builtins::register_into(
            &registry,
            Arc::new(NoopLoader),
            ResolveConfig::with_root(std::path::PathBuf::from("/")),
        )
        .expect("registering built-in lex.* handlers must succeed for a fresh registry");
        registry
    })
}

/// Converts an IR document back to Lex AST.
///
/// This is useful for round-trip conversions: Format → IR → Lex.
pub fn from_ir(doc: &ir::nodes::Document) -> lex_core::lex::ast::elements::Document {
    ir::to_lex::to_lex_document(doc)
}
