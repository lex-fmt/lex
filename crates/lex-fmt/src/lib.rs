//! Canonical Rust embedder API for the lex document format.
//!
//! This crate is the recommended entry point for any application
//! embedding Lex: docs pipelines, publishing servers, batch
//! converters, custom CLIs. It wraps the workspace's individual
//! library crates — `lex-core` (parser), `lex-babel` (format
//! conversion), `lex-analysis` (semantic analysis), and
//! `lex-extension-host` (extension registry) — behind a single
//! [`Engine::builder()`] entry point.
//!
//! ## Quick start
//!
//! ```ignore
//! use lex_fmt::Engine;
//!
//! let engine = Engine::builder()
//!     .workspace_root("/path/to/project")
//!     .load_lex_toml("/path/to/project/lex.toml")?
//!     .build()?;
//!
//! let doc = engine.resolve_source(source_text, Some(source_path))?;
//! let diagnostics = engine.analyze(&doc);
//! let html = engine.render(&doc, "html")?;
//! ```
//!
//! ## Pre-supplied trust prompts
//!
//! Embedders that don't install a custom [`TrustPromptHandler`] get
//! [`prompts::AutoDenyPrompt`] by default — every subprocess handler
//! is denied with a clear rationale. For fixture-driven tests,
//! [`prompts::AutoTrustPrompt`] auto-trusts (and emits a stderr
//! warning per invocation so production misuse leaves a paper trail).
//! Host-specific prompts (CLI TTY-friendly, LSP request-forwarded)
//! live in the `lexd` and `lexd-lsp` crates respectively.
//!
//! ## Boot helper (shared with `lexd` / `lexd-lsp`)
//!
//! [`boot_registry`] and friends — the host-agnostic registry boot
//! pipeline — also live in this crate, exposed via the [`setup`]
//! module. Use these directly when you need to drive registry
//! construction without [`Engine`]'s pipeline methods (e.g.,
//! implementing a custom LSP request handler).
//!
//! [`TrustPromptHandler`]: lex_extension_host::TrustPromptHandler

pub mod engine;
pub mod prompts;
pub mod setup;

pub use engine::{BuildError, Engine, EngineBuilder, ParseError, RenderError, ResolveError};
pub use prompts::{AutoDenyPrompt, AutoTrustPrompt};
pub use setup::{
    boot_registry, BootDiagnostic, BootOutcome, ExtensionSetup, NamespaceSourceKind,
    RegisteredNamespace,
};
