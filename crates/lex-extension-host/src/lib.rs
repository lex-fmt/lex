//! Internal runtime for the Lex extension system.
//!
//! This crate is *not* published. It hosts the registry, schema loader,
//! namespace URI resolver, transport adapters (subprocess, future WASM), and
//! trust gate that turn a set of [`lex_extension::Schema`]s plus
//! [`lex_extension::LexHandler`] implementations into a dispatch fabric the
//! `lexd` CLI, `lex-lsp` server, and Rust embedders all share.
//!
//! What's in this crate today (PR 2 of the extension-system rollout):
//!
//! - [`Registry`] — namespace registration, label lookup, and dispatch
//!   helpers wrapping every hook event with `HandlerError` folding and
//!   panic catch.
//! - [`transport::native`] — the trivial transport: a registered
//!   `Box<dyn LexHandler>` is its own transport, no adapter required.
//!
//! Coming in later PRs:
//!
//! - PR 4: schema YAML loader and namespace URI resolver.
//! - PR 5: subprocess transport (JSON-RPC over stdio + `initialize`
//!   handshake).
//! - PR 6: trust store and decision matrix.
//! - PR 12: OS-level sandboxing for declared-pure subprocess handlers.

pub mod registry;
pub mod transport;

pub use registry::{Registry, RegistryError};
