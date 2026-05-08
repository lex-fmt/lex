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
    CodeAction, Completion, Diagnostic, Format, Hover, LabelCtx, RenderOut, WireNode,
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
    fn on_resolve(&self, _ctx: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
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
        assert!(h.on_render(&c, Format::Html).unwrap().is_none());
        assert!(h.on_hover(&c).unwrap().is_none());
        assert!(h.on_completion(&c).unwrap().is_empty());
        assert!(h.on_code_action(&c).unwrap().is_empty());
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
