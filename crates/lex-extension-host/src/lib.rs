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
//! # Writing a handler — the unified registration pattern (#615)
//!
//! Extension authors register one [`lex_extension::Schema`] per label,
//! attach the lifecycle hooks that label participates in, and provide
//! one [`lex_extension::LexHandler`] implementation per namespace. The
//! `Registry` routes each hook to the right method by namespace + label:
//!
//! ```ignore
//! use lex_extension::{LexHandler, Format, RenderOut, WireNode};
//! use lex_extension::handler::HandlerError;
//! use lex_extension::wire::LabelCtx;
//! use lex_extension::schema::{HookSet, RenderHook, Schema};
//! use lex_extension_host::Registry;
//!
//! struct AcmeHandler;
//! impl LexHandler for AcmeHandler {
//!     // IR-construction lifecycle: hydrate verbatim payloads
//!     // (`:: acme.table ::`, `:: acme.image ::`) into typed wire
//!     // nodes the host's IR builder consumes.
//!     fn on_ir_build(&self, ctx: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
//!         match ctx.label.as_str() {
//!             "acme.thing" => Ok(Some(WireNode::Verbatim { /* ... */ })),
//!             _ => Ok(None),
//!         }
//!     }
//!     // Pre-serialisation lifecycle: emit the format-specific
//!     // representation (markdown, HTML, ...). One handler can
//!     // participate in both IR-build and render against the same
//!     // schema — a single registration, both lifecycles.
//!     fn on_render(&self, ctx: &LabelCtx, fmt: Format) -> Result<Option<RenderOut>, HandlerError> {
//!         /* ... */
//!         Ok(None)
//!     }
//! }
//!
//! let registry = Registry::new();
//! registry.register_namespace(
//!     "acme",
//!     vec![Schema {
//!         schema_version: 1,
//!         label: "acme.thing".into(),
//!         hooks: HookSet {
//!             ir_build: true,                              // declare IR-build participation
//!             render: vec![RenderHook::new("html")],       // declare render participation
//!             ..HookSet::default()
//!         },
//!         /* ... rest of Schema ... */
//! #       description: None, params: Default::default(), attaches_to: vec![],
//! #       body: Default::default(), verbatim_label: false,
//! #       capabilities: Default::default(), handler: None,
//!     }],
//!     Box::new(AcmeHandler),
//! ).expect("registration ok");
//! ```
//!
//! ## Lifecycle hooks
//!
//! Three hook surfaces, each on its own lifecycle phase:
//!
//! | Hook            | Lifecycle phase             | Dispatch entry point          | Built-in example      |
//! |-----------------|-----------------------------|-------------------------------|-----------------------|
//! | `on_resolve`    | AST substitution            | [`Registry::dispatch_resolve`]| `lex.include`         |
//! | `on_ir_build`   | IR construction             | [`Registry::dispatch_ir_build`]| `lex.tabular.table`, `lex.media.*` |
//! | `on_render`     | Pre-serialisation           | [`Registry::dispatch_render`] | `doc.title`, `doc.author`, ... |
//!
//! `on_resolve` and `on_ir_build` have the same shape
//! (`Result<Option<WireNode>, HandlerError>`); they're separate hooks
//! because they fire at different lifecycle phases and have different
//! consumer contracts. `on_resolve` returns a wire node spliced into
//! the host AST; `on_ir_build` returns a wire node consumed by the IR
//! builder. Pre-#615 these were a single overloaded hook
//! (`on_resolve`); the unified registry surface separates them so
//! extension authors can declare exactly the lifecycle phase they
//! participate in.
//!
//! # What's in this crate
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
//! - [`sandbox::Sandbox`] — OS-level enforcement facade. The
//!   plumbing-PR default is [`sandbox::NullSandbox`] (no
//!   enforcement, `available() == false`). Per-OS implementations
//!   land in follow-up PRs (12a Linux, 12b macOS, 12c Windows); the
//!   trust matrix flip (PR 12d) consumes [`Sandbox::available`] to
//!   auto-trust declared-pure handlers under enforced sandboxing.
//!
//! Coming in later PRs:
//!
//! - PR 12a/b/c: per-OS sandbox enforcement.
//! - PR 12d: trust matrix flip (auto-trust pure handlers under
//!   enforced sandbox).

pub mod registry;
pub mod resolve;
pub mod sandbox;
pub mod schema;
pub mod transport;
pub mod trust;

pub use registry::{Registry, RegistryError};
pub use resolve::{
    default_fetcher_registry, resolve_namespace, resolve_namespace_with, FetchError, Fetcher,
    FetcherRegistry, ParsedUri, ResolveError, ResolvedNamespace, ResolverCache, UriParseError,
};
pub use sandbox::{NullSandbox, Sandbox, SandboxError};
pub use schema::{SchemaError, SchemaLoader};
pub use trust::{
    detect_ci_environment, Capability, Source, Surface, Transport, TrustDecision, TrustGate,
    TrustKey, TrustPromptContext, TrustPromptHandler, TrustStore, TrustStoreError,
};
