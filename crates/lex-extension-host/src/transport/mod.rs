//! Transport tiers: how a [`LexHandler`](lex_extension::LexHandler)
//! invocation is delivered.
//!
//! The protocol contract is a single `LexHandler` trait. Three transports
//! satisfy that trait through different mechanisms:
//!
//! - [`native`] — direct calls into a `Box<dyn LexHandler>`. Built-ins and
//!   in-process Rust embedders use this. Zero IPC.
//! - subprocess (PR 5) — spawns a handler binary and serialises calls to
//!   JSON-RPC over stdio.
//! - WASM (deferred) — same wire format delivered as component-model
//!   imports.
//!
//! WASM lands later. Subprocess sits behind the `subprocess` Cargo
//! feature (default-on for CLI/LSP, off in `lex-core`) so native-only
//! consumers don't pull in `tokio`.

pub mod native;

#[cfg(feature = "subprocess")]
pub(crate) mod jsonrpc;
#[cfg(feature = "subprocess")]
pub mod subprocess;

#[cfg(feature = "subprocess")]
pub use subprocess::{SpawnEnv, SpawnError, SubprocessHandler};
