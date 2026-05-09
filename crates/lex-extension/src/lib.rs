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
//! There are two version axes, intentionally decoupled:
//!
//! - [`WIRE_VERSION`] identifies the JSON-RPC wire-format contract
//!   exchanged at the `initialize` handshake. Handlers across any
//!   transport (subprocess, native, future WASM) speaking the same
//!   `WIRE_VERSION` interoperate. Once stable, breaking changes to the
//!   wire format bump this integer.
//! - The crate's *Cargo* version (`0.1.x` today) tracks the Rust API.
//!
//! The two axes line up at 1.0: this crate ticks to `1.0.0` when its Rust
//! API stabilises, and at that point a handler built against
//! `lex-extension = "1"` speaks `WIRE_VERSION = 1`. Until then the Rust
//! API is per-Cargo unstable: any 0.x → 0.y bump may be source-incompatible.
//!
//! Where forward-compatibility is implementable today without API churn,
//! it is:
//!
//! - String-shaped enums (severity, completion kind, code-action kind,
//!   ref kind, hover format) deserialise unknown wire values as a
//!   documented fallback (`Info`, `Value`, `Refactor`, `General`,
//!   `Plaintext`). New variants in the wire protocol are non-breaking
//!   *for handlers* — older handlers see the fallback.
//! - Block AST kinds ([`WireNode`], [`WireInline`]) are *closed* within a
//!   `WIRE_VERSION`. Adding a new kind is a wire-version bump, not a
//!   silent extension. The `#[non_exhaustive]` attribute on those enums
//!   keeps the Rust side additive across `lex-extension` major versions.
//! - Public structs (`Diagnostic`, `Hover`, `Completion`, etc.) are *not*
//!   `#[non_exhaustive]` today, by design — pre-1.0, struct-literal
//!   construction is the right ergonomics for handler authors. Before
//!   tagging 1.0 the public structs will gain `#[non_exhaustive]` plus
//!   constructors, so post-1.0 field additions stay non-breaking.
//!
//! See `comms/specs/proposals/lex-extension-wire.lex` for the normative
//! wire-format spec.

pub mod handler;
pub mod schema;
pub mod wire;

pub use handler::{HandlerError, LexHandler};
pub use wire::{
    AnnotationBody, CodeAction, CodeActionKind, Completion, CompletionKind, Diagnostic,
    DiagnosticSeverity, Format, HostNodeKind, Hover, HoverFormat, LabelCtx, NodeRef, Position,
    Range, RefKind, RelatedDiagnostic, RenderOut, TextEdit, WireFootnote, WireInline, WireListItem,
    WireNode, WireRow, WireTableCell,
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
