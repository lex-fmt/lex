//! Runtime for the Lex extension system.
//!
//! This crate hosts the registry, schema loader, namespace URI resolver,
//! transport adapters (subprocess, future WASM), and trust gate that turn
//! a set of [`lex_extension::Schema`]s plus [`lex_extension::LexHandler`]
//! implementations into a dispatch fabric the `lexd` CLI, `lex-lsp`
//! server, `lex-core` (for built-in `lex.*` resolvers), and Rust
//! embedders all share.
//!
//! Pre-1.0 the public API surface is unstable per Cargo convention. The
//! crate is published so that downstream crates in the lex toolchain —
//! especially `lex-core`, which carries the `lex.include` resolver as
//! the first built-in `LexHandler` — can depend on it. Handler authors
//! should depend on `lex-extension`, not this crate.
//!
//! What's in this crate today:
//!
//! - [`Registry`] — namespace registration, label lookup, and dispatch
//!   helpers wrapping every hook event with `HandlerError` folding and
//!   panic catch.
//! - [`schema::SchemaLoader`] — YAML schema loader + post-deserialise
//!   validator.
//! - [`transport::native`] — the trivial transport: a registered
//!   `Box<dyn LexHandler>` is its own transport, no adapter required.
//! - [`transport::subprocess`] (behind the `subprocess` feature) —
//!   spawn a handler binary and dispatch over LSP-framed JSON-RPC.
//! - [`trust::TrustGate`] — decides whether a handler is allowed to
//!   run, per the β/γ-correct policy in the master tracking issue
//!   (subprocess always prompts; native trusted by linkage).
//!
//! Coming in later PRs:
//!
//! - PR 12: OS-level sandboxing for declared-pure subprocess handlers
//!   (the post-δ trust matrix flip).

pub mod registry;
pub mod schema;
pub mod transport;
pub mod trust;

pub use registry::{Registry, RegistryError};
pub use schema::{SchemaError, SchemaLoader};
pub use trust::{
    detect_ci_environment, Capability, Source, Surface, Transport, TrustDecision, TrustGate,
    TrustKey, TrustPromptContext, TrustPromptHandler, TrustStore, TrustStoreError,
};
