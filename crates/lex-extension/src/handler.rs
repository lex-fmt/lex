//! The [`LexHandler`] trait — the protocol's source of truth.
//!
//! Native handlers (built-ins, in-process Rust embedders) `impl` this trait
//! directly. Subprocess and WASM transports are delivered as generic adapters
//! that `impl` the same trait by serialising calls to JSON-RPC or component
//! imports respectively.
//!
//! Methods that produce non-trivial output return
//! `Result<Option<T>, HandlerError>`. The `Result` distinguishes "I hit an
//! error you should surface as a diagnostic" from "I succeeded but have
//! nothing to contribute"; the inner `Option`/`Vec` covers the latter.
//! [`LexHandler::on_label`] returns `()` because it is a notification.

use crate::wire::{
    CodeAction, Completion, Diagnostic, Format, FormatCtx, Hover, LabelCtx, LexAnnotationOut,
    RenderOut, WireNode,
};

/// The hook-event interface a Lex extension implements.
///
/// Every method has a default implementation that returns the identity
/// (`Ok(None)`, `Ok(Vec::new())`, `()`), so an extension only needs to
/// override the methods it cares about. An empty `impl LexHandler for Foo {}`
/// is a no-op handler that compiles and runs.
pub trait LexHandler: Send + Sync {
    /// Informational notification fired during the analyse phase. No response
    /// is expected. Use this for handlers that maintain external state
    /// (caches, indices, link graphs).
    fn on_label(&self, _ctx: &LabelCtx) {}

    /// Returns diagnostics for a labelled node. Fires during analyse, after
    /// resolve.
    fn on_validate(&self, _ctx: &LabelCtx) -> Result<Vec<Diagnostic>, HandlerError> {
        Ok(Vec::new())
    }

    /// Returns an AST replacement subtree, which the host splices into the
    /// parent in place of the labelled node. Fires during the resolve phase,
    /// before analyse. `Ok(None)` leaves the original node in place.
    ///
    /// `on_resolve` is the AST-substitution lifecycle: the canonical example
    /// is `lex.include`, which splices the resolved file's content into the
    /// host document. Verbatim labels that hydrate into typed IR nodes
    /// (`lex.tabular.table`, `lex.media.*`) belong on
    /// [`on_ir_build`](Self::on_ir_build) instead — that hook is the
    /// IR-construction lifecycle and is invoked during `from_lex` IR build.
    fn on_resolve(&self, _ctx: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
        Ok(None)
    }

    /// Returns a typed wire node consumed by the host while building its
    /// in-memory IR from the parsed source. Fires during IR construction
    /// (`from_lex`), strictly after parsing and strictly before render.
    /// `Ok(None)` falls back to the host's generic verbatim/annotation IR.
    ///
    /// This is the lifecycle hook for **content-typing** labels — the
    /// canonical examples are `lex.tabular.table` (verbatim body → typed
    /// `WireNode::Table`) and `lex.media.{image,video,audio}` (params →
    /// typed `WireNode::Image|Video|Audio`). Pair an `on_ir_build` hook
    /// with an [`on_render`](Self::on_render) hook on the same schema to
    /// give one label both an IR shape and per-format serialization
    /// behaviour through the unified registry surface (#615).
    ///
    /// IR-build hooks do **not** receive the host's lex-core AST: they
    /// see only the parsed verbatim payload (label + params + body) via
    /// [`LabelCtx`]. Coupling content-typing to the IR phase rather than
    /// to parsing keeps a buggy or slow handler from corrupting the
    /// parser, and gives extension authors a single registration point
    /// for both lifecycle phases.
    fn on_ir_build(&self, _ctx: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
        Ok(None)
    }

    /// Returns the labelled node's representation in a target format. Fires
    /// during `lexd convert` or library-driven rendering. `Ok(None)` falls
    /// back to default rendering of the underlying node.
    fn on_render(&self, _ctx: &LabelCtx, _fmt: Format) -> Result<Option<RenderOut>, HandlerError> {
        Ok(None)
    }

    /// Returns hover content for a labelled node. Fires in response to
    /// `textDocument/hover` LSP requests.
    fn on_hover(&self, _ctx: &LabelCtx) -> Result<Option<Hover>, HandlerError> {
        Ok(None)
    }

    /// Returns completion items for a position inside a labelled node's
    /// params or body. Fires in response to `textDocument/completion`.
    fn on_completion(&self, _ctx: &LabelCtx) -> Result<Vec<Completion>, HandlerError> {
        Ok(Vec::new())
    }

    /// Returns code actions for a labelled node. Fires in response to
    /// `textDocument/codeAction`.
    fn on_code_action(&self, _ctx: &LabelCtx) -> Result<Vec<CodeAction>, HandlerError> {
        Ok(Vec::new())
    }

    /// Returns the Lex-source representation of a typed AST subtree
    /// owned by this handler's namespace — the inverse of
    /// [`on_resolve`](Self::on_resolve), and the reverse-direction
    /// sibling of [`on_render`](Self::on_render) for the Lex target
    /// format.
    ///
    /// Phase 4a of #570 ships this trait method, the `FormatCtx` /
    /// `LexAnnotationOut` wire types, and the
    /// [`Registry::dispatch_format`](`lex_extension_host::registry::Registry::dispatch_format`)
    /// entry point. Phase 4b implements `on_format` in the built-in
    /// `lex.tabular.*` / `lex.media.*` handlers. Production call
    /// sites in `to_lex.rs` and `lexd format` get wired in a Phase 4b
    /// follow-up — until that lands, the hook is invocable through
    /// the registry (tests + library embedders use it) but no
    /// built-in pass dispatches through it yet, so a handler
    /// implementing `on_format` will be exercised by direct
    /// `Registry::dispatch_format` callers only.
    ///
    /// `Ok(None)` lets the host fall back to its built-in formatter
    /// for the underlying node kind — there is no separate
    /// "not handled" error code. See `comms/specs/proposals/lex-extension-wire.lex`
    /// §4.8 for the full wire contract.
    fn on_format(&self, _ctx: &FormatCtx) -> Result<Option<LexAnnotationOut>, HandlerError> {
        Ok(None)
    }
}

/// Errors a [`LexHandler`] method can surface.
///
/// A handler that hits an internal failure returns `Err(HandlerError::...)`;
/// the host folds the error into a synthetic diagnostic at the labelled
/// node's range and continues processing other labels. Subprocess transports
/// map these variants onto JSON-RPC error responses with the standard
/// reserved code ranges (`-32000..=-32099` for handler-defined; `-32601` for
/// unsupported method/format).
#[derive(Debug, Clone, PartialEq)]
pub enum HandlerError {
    /// Handler hit an internal error (panic, library failure, unexpected
    /// state). Maps to JSON-RPC `-32603`.
    Internal { message: String },
    /// Handler does not support the requested operation — for example,
    /// `on_render` was called with a format the handler does not produce.
    /// Maps to JSON-RPC `-32601`.
    Unsupported { detail: String },
    /// Handler-defined error. `code` should fall in the
    /// `-32000..=-32099` range reserved for handler use. Maps to
    /// JSON-RPC `error` with the supplied code, message, and optional data.
    Custom {
        code: i32,
        message: String,
        data: Option<serde_json::Value>,
    },
}

impl HandlerError {
    /// Convenience constructor for the common case of an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Convenience constructor for an unsupported operation.
    pub fn unsupported(detail: impl Into<String>) -> Self {
        Self::Unsupported {
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for HandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandlerError::Internal { message } => {
                write!(f, "handler internal error: {message}")
            }
            HandlerError::Unsupported { detail } => {
                write!(f, "handler does not support: {detail}")
            }
            HandlerError::Custom { code, message, .. } => {
                write!(f, "handler error {code}: {message}")
            }
        }
    }
}

impl std::error::Error for HandlerError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{LabelCtx, NodeRef, Position, Range};

    /// A no-op handler should compile with no method overrides — the
    /// ergonomics check called out in PR 1's success criteria.
    struct NoOp;
    impl LexHandler for NoOp {}

    fn ctx() -> LabelCtx {
        LabelCtx {
            label: "test.label".into(),
            params: serde_json::json!({}),
            body: crate::wire::AnnotationBody::None,
            node: NodeRef {
                kind: "annotation".into(),
                range: Range {
                    start: Position(0, 0),
                    end: Position(0, 0),
                },
                origin: None,
            },
        }
    }

    #[test]
    fn noop_handler_returns_defaults() {
        let h = NoOp;
        let c = ctx();
        h.on_label(&c);
        assert!(h.on_validate(&c).unwrap().is_empty());
        assert!(h.on_resolve(&c).unwrap().is_none());
        assert!(h.on_ir_build(&c).unwrap().is_none());
        assert!(h.on_render(&c, Format::Html).unwrap().is_none());
        assert!(h.on_hover(&c).unwrap().is_none());
        assert!(h.on_completion(&c).unwrap().is_empty());
        assert!(h.on_code_action(&c).unwrap().is_empty());
        // on_format added in #570 Phase 4a — same Ok(None) default.
        let format_ctx = crate::wire::FormatCtx {
            label: "test.label".into(),
            params: vec![],
            node: WireNode::Paragraph {
                range: Range {
                    start: Position(0, 0),
                    end: Position(0, 0),
                },
                origin: None,
                inlines: vec![],
            },
            format_options: None,
        };
        assert!(h.on_format(&format_ctx).unwrap().is_none());
    }

    #[test]
    fn handler_error_display() {
        assert_eq!(
            HandlerError::internal("boom").to_string(),
            "handler internal error: boom"
        );
        assert_eq!(
            HandlerError::unsupported("png").to_string(),
            "handler does not support: png"
        );
        assert_eq!(
            HandlerError::Custom {
                code: -32001,
                message: "custom".into(),
                data: None,
            }
            .to_string(),
            "handler error -32001: custom"
        );
    }
}
