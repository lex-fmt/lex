//! Native transport.
//!
//! There is no adapter to write here: a `Box<dyn LexHandler>` registered
//! into the [`Registry`](crate::Registry) is dispatched directly via the
//! trait. This module exists for symmetry with the future `subprocess` and
//! `wasm` modules, and for documentation: the registry treats handlers
//! delivered over any transport identically.
//!
//! Built-ins (`lex.include`, future `lex.toc`) are native handlers in the
//! `lex-core` crate, registered via PR 3's
//! `lex_builtins::register_into(&mut Registry)` helper. Library embedders
//! using `Engine::builder()` (PR 11) likewise register native handlers
//! directly.
