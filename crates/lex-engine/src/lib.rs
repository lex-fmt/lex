//! Engine glue for lex hosts (lexd CLI, lexd-lsp, future embedders).
//!
//! Today this crate hosts the [`setup::boot_registry`] helper that turns a
//! workspace root + `lex.toml` `[labels]` block + ext-schema flags + a
//! [`TrustPromptHandler`] of the host's choosing into a populated
//! [`Registry`] with the bundled `lex.*` built-ins, third-party namespaces
//! (resolved through `lex-extension-host`'s URI resolver), and any
//! `--ext-schema <dir>` directories the host wants to add. Diagnostics are
//! surfaced rather than fatal — a single misconfigured namespace shouldn't
//! break the rest of the host.
//!
//! In a later PR (the planned PR 11), this crate gains a public
//! [`Engine::builder()`] API for third-party Rust embedders. The setup
//! helper here is the implementation seam under the hood.
//!
//! [`Registry`]: lex_extension_host::Registry
//! [`TrustPromptHandler`]: lex_extension_host::TrustPromptHandler

pub mod setup;

pub use setup::{
    boot_registry, BootDiagnostic, BootOutcome, ExtensionSetup, NamespaceSourceKind,
    RegisteredNamespace,
};
