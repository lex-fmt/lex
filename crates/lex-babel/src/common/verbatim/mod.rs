//! Verbatim Block Registry and Handling
//!
//! This module provides a flexible and extensible system for handling `verbatim` blocks in Lex.
//! Verbatim blocks are chunks of content that are not parsed by the core Lex parser but are preserved
//! as-is. They are often used for code blocks, raw data, or content that requires specialized
//! processing (like tables, images, or diagrams).
//!
//! # Design Philosophy
//!
//! The core philosophy is that Lex should be able to represent any content, even if it doesn't natively
//! understand it. However, when converting to other formats (like HTML, Markdown, or an intermediate
//! representation), we often want to "hydrate" these verbatim blocks into richer semantic structures.
//!
//! For example, a `doc.table` verbatim block containing a pipe table string should ideally become a
//! structured `Table` node in the IR, rather than just a blob of text.
//!
//! # Architecture
//!
//! The system revolves around two main components:
//!
//! 1.  **`VerbatimHandler` Trait**: Defines how to convert between a raw verbatim block (string content + params)
//!     and a semantic `DocNode` (IR node).
//! 2.  **`VerbatimRegistry`**: A central registry that maps labels (e.g., "doc.table", "image") to
//!     specific handlers.
//!
//! ## The Translation Layer
//!
//! This module acts as a translation layer between the raw Lex AST and the semantic IR.
//!
//! *   **Lex -> IR**: When converting *from* Lex, if a verbatim block's label matches a registered handler,
//!     the handler's `to_ir` method is called. This allows "doc.table" to become a `DocNode::Table`.
//! *   **IR -> Lex**: When converting *to* Lex, if an IR node (like `DocNode::Table`) needs to be serialized,
//!     handlers are queried to see if they can represent it. This allows a `DocNode::Table` to be serialized
//!     back as a `doc.table` verbatim block.
//!
//! # Namespaces
//!
//! To support extensibility and avoid collisions, the registry supports namespaced handlers.
//!
//! *   **Exact Match**: "doc.table" matches exactly.
//! *   **Namespace Match**: "acme.*" matches any label starting with "acme.".
//!
//! This allows plugins to register a catch-all handler for their own custom types.
//!
//! # Standard Handlers
//!
//! Lex provides standard handlers for common types within the `doc` namespace:
//!
//! *   `doc.table`: Markdown-style pipe tables.
//! *   `doc.image`: Image references.
//! *   `doc.video`, `doc.audio`: Media references.
//!
//! # Usage
//!
//! ```rust,ignore
//! let mut registry = VerbatimRegistry::new();
//! registry.register("doc.table", Box::new(TableHandler));
//!
//! // Converting to IR
//! if let Some(handler) = registry.get("doc.table") {
//!     let node = handler.to_ir(content, &params);
//! }
//! ```

use crate::error::FormatError;
use lex_core::lex::ast::Verbatim;
use std::collections::HashMap;

pub mod media;
pub mod table;

/// A handler for a specific verbatim block type.
///
/// Post-refac/label cleanup: the `to_ir` and `convert_from_ir` trait
/// methods are retired. `from_lex_verbatim` now calls free hydration
/// helpers (`table::parse_pipe_table`, `media::image_from_params`,
/// etc.) directly, and `to_lex_table` / `to_lex_media` route through
/// `Registry::dispatch_format` (the `lex.tabular.*` / `lex.media.*`
/// built-in handlers in lex-core own the IR→Lex contract). The trait
/// shrinks to `label()` + `format_content()` — the latter is still
/// consumed by the Lex serializer for reformat-during-format.
pub trait VerbatimHandler: Send + Sync {
    /// Returns the canonical label this handler supports
    /// (e.g., `"lex.tabular.table"`).
    fn label(&self) -> &str;

    /// Formats the content of a verbatim block during `lexd format`.
    ///
    /// Returns `Ok(Some(formatted_content))` if the handler supports formatting,
    /// `Ok(None)` if it doesn't, or `Err` if formatting failed.
    fn format_content(&self, _verbatim: &Verbatim) -> Result<Option<String>, FormatError> {
        Ok(None)
    }
}

/// A registry for verbatim block handlers.
pub struct VerbatimRegistry {
    handlers: HashMap<String, Box<dyn VerbatimHandler>>,
    namespace_handlers: Vec<(String, Box<dyn VerbatimHandler>)>,
}

impl VerbatimRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            namespace_handlers: Vec::new(),
        }
    }

    /// Creates a registry with standard handlers pre-registered under
    /// their canonical `lex.*` labels.
    ///
    /// `STRING_TO_AST` runs `NormalizeLabels` (#570 Phase 3b) before
    /// this lookup ever sees a label, so the AST always carries
    /// canonical names by the time `from_lex_verbatim` calls
    /// `registry.get(...)`. The legacy `doc.*` aliases that were kept
    /// alongside in Phase 3b are removed in the post-refac/label
    /// cleanup — embedders that hand-build IR Verbatim nodes with the
    /// legacy form will now hit a `None` lookup, which is the
    /// documented breaking-change behaviour.
    pub fn default_with_standard() -> Self {
        let mut registry = Self::new();
        registry.register("lex.tabular.table", Box::new(table::TableHandler));
        registry.register("lex.media.image", Box::new(media::ImageHandler));
        registry.register("lex.media.video", Box::new(media::VideoHandler));
        registry.register("lex.media.audio", Box::new(media::AudioHandler));
        registry
    }

    /// Registers a handler for an exact label.
    pub fn register(&mut self, label: &str, handler: Box<dyn VerbatimHandler>) {
        self.handlers.insert(label.to_string(), handler);
    }

    /// Registers a handler for a namespace (e.g., "acme.").
    /// The handler will be used for any label starting with this prefix.
    pub fn register_namespace(&mut self, prefix: &str, handler: Box<dyn VerbatimHandler>) {
        self.namespace_handlers.push((prefix.to_string(), handler));
    }

    /// Gets a handler for the given label.
    pub fn get(&self, label: &str) -> Option<&dyn VerbatimHandler> {
        if let Some(handler) = self.handlers.get(label) {
            return Some(handler.as_ref());
        }

        for (prefix, handler) in &self.namespace_handlers {
            if label.starts_with(prefix) {
                return Some(handler.as_ref());
            }
        }

        None
    }
}

impl Default for VerbatimRegistry {
    fn default() -> Self {
        Self::new()
    }
}
