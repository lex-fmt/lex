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
//! Today this module hosts only the native transport. Subprocess and WASM
//! adapters land in their own PRs and slot into the same dispatch path
//! by virtue of also implementing `LexHandler`.

pub mod native;
