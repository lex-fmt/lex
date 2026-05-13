//! Wire-format types: the cross-version stable representation of Lex content
//! and hook payloads.
//!
//! These types are the public contract that JSON-RPC subprocess transports
//! and (eventually) WASM transports both serialise to. Native handlers
//! consume them directly. The serde derives on each type produce JSON shapes
//! matching the wire spec under `comms/specs/proposals/lex-extension-wire.lex`.

mod ast;
mod ctx;
mod format;
mod format_out;
mod host_node_kind;
mod inline;
mod payload;
mod range;

pub use ast::{WireFootnote, WireListItem, WireNode, WireRow, WireTableCell};
pub use ctx::{AnnotationBody, LabelCtx, NodeRef};
pub use format::Format;
pub use format_out::{FormatCtx, LexAnnotationOut};
pub use host_node_kind::HostNodeKind;
pub use inline::{RefKind, WireInline};
pub use payload::{
    CodeAction, CodeActionKind, Completion, CompletionKind, Diagnostic, DiagnosticSeverity, Hover,
    HoverFormat, RelatedDiagnostic, RenderOut, TextEdit,
};
pub use range::{Position, Range};
