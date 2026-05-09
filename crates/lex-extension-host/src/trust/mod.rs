//! Trust store and decision matrix for extension handlers.
//!
//! This module decides whether a registered handler is allowed to run
//! at all. Per the β/γ correctness rule baked into the master tracking
//! issue (lex#516, correction #1), subprocess handlers always require
//! explicit user approval — `capabilities: { fs: false, net: false }`
//! in the schema is just a string until OS-level enforcement lands in
//! PR 12 (δ). Until then:
//!
//! - Native handlers (the bundled `lex.*` built-ins): always trusted by
//!   linkage.
//! - Subprocess handlers from any source: prompt or `--enable-handlers`,
//!   regardless of declared capabilities.
//! - WASM handlers: rejected at schema load (the schema loader's
//!   `WasmTransportDeferred` variant fires before the gate sees them).
//!
//! # Surfaces
//!
//! Three host surfaces consume the gate:
//!
//! - `CliOneShot`: the `lexd` CLI, single-document conversion. No
//!   interactive prompt; subprocess handlers are denied unless the
//!   user upfronts `--enable-handlers`. Persisting trust would not be
//!   useful (CLI runs are stateless).
//! - `LspSession`: `lex-lsp` running in an editor. Subprocess handlers
//!   prompt the user via the [`TrustPromptHandler`] callback; the
//!   answer pins to the specific `command` string and persists in the
//!   workspace's `.lex/trust.json`. PR 10 wires the callback to a
//!   `lex/trustRequest` LSP notification.
//! - `Ci`: auto-detected from env vars (`CI`, `GITHUB_ACTIONS`, …).
//!   Same denial rule as `CliOneShot` — but firing the deny is
//!   important because CI scripts shouldn't accidentally run untrusted
//!   handlers even when the underlying flag is set elsewhere.
//!
//! # What this module does *not* do
//!
//! - Spawn or dispatch handlers. The gate is consulted by the host
//!   *before* it constructs a `SubprocessHandler`; if the gate says
//!   `Denied`, the host emits a diagnostic and skips registration.
//! - Honor the schema's declared `capabilities`. That's the post-δ
//!   matrix's job. The `Capability` field on the evaluator is stored
//!   for forward-compat but does not influence the decision today.
//! - OS-level sandboxing. Handler processes run with the host's
//!   privileges. PR 12 lands the sandbox.

mod decision;
mod store;

pub use decision::{
    detect_ci_environment, Capability, Source, Surface, Transport, TrustDecision, TrustGate,
    TrustPromptContext, TrustPromptHandler,
};
pub use store::{TrustKey, TrustStore, TrustStoreError};
