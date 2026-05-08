//! Public surface for Lex extensions.
//!
//! This crate is the stable contract that handler authors and Rust embedders
//! depend on. It defines:
//!
//! - The [`LexHandler`] trait — the protocol's source of truth.
//! - Wire types ([`LabelCtx`], [`WireNode`], [`WireInline`], diagnostics,
//!   render output, hover, completions, code actions) — the cross-version
//!   stable representation of Lex content and hook payloads.
//! - Schema types ([`Schema`] and friends) — the read-only structs a YAML
//!   loader produces.
//! - The [`Format`] enum used by render hooks.
//!
//! The runtime registry, schema loader, transports (subprocess, WASM), trust
//! gate, and sandboxing live in the internal `lex-extension-host` crate.
//! Handler authors depend on `lex-extension` only.
//!
//! # Versioning
//!
//! [`WIRE_VERSION`] tracks the wire-format contract. The crate's major
//! version mirrors it. A handler built against `lex-extension` 1.x speaks
//! `wire_version: 1`. Non-breaking additions (new methods, new optional
//! fields, new node kinds, new severity levels) are minor/patch bumps; any
//! change to existing field shapes or method semantics is a major bump.
//!
//! See `comms/specs/proposals/lex-extension-wire.lex` for the normative wire
//! format spec.

pub mod handler;
pub mod schema;
pub mod wire;

pub use handler::{HandlerError, LexHandler};
pub use wire::{
    AnnotationBody, CodeAction, CodeActionKind, Completion, CompletionKind, Diagnostic,
    DiagnosticSeverity, Format, Hover, HoverFormat, LabelCtx, NodeRef, Position, Range, RefKind,
    RelatedDiagnostic, RenderOut, TextEdit, WireFootnote, WireInline, WireListItem, WireNode,
    WireRow, WireTableCell,
};

pub use schema::{
    BodyKind, BodyPresence, BodyShape, Capabilities, EnumValue, HandlerSpec, HandlerTransport,
    HookSet, ParamSpec, ParamType, RenderHook, Schema,
};

/// The wire-format protocol version exchanged in the `initialize` handshake.
///
/// A host that receives a higher `wire_version` than it understands negotiates
/// down to the highest version both sides support. A host that receives a
/// lower `wire_version` than this constant refuses the handler with a startup
/// diagnostic.
pub const WIRE_VERSION: u32 = 1;
