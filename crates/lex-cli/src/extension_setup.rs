//! CLI-side wiring for the extension boot helper.
//!
//! Re-exports the [`lex_fmt::setup`] types so existing call sites in
//! `main.rs` and `labels_subcommand.rs` keep working with their current
//! `crate::extension_setup::*` paths, and provides a [`boot_registry`]
//! shim that injects the CLI-specific [`CliPromptHandler`] (always
//! denies with a `--enable-handlers` rationale — TTY prompts are an
//! anti-pattern for one-shot CLI use).

use lex_extension_host::{TrustDecision, TrustPromptContext, TrustPromptHandler};

pub use lex_fmt::setup::{BootDiagnostic, BootOutcome, NamespaceSourceKind, RegisteredNamespace};

/// CLI-shaped inputs to [`boot_registry`]. Mirrors
/// [`lex_fmt::setup::ExtensionSetup`] minus the `trust_prompt` field —
/// the CLI's prompt is fixed.
pub struct ExtensionSetup<'a> {
    pub workspace_root: &'a std::path::Path,
    pub labels_config: &'a lex_config::LabelsConfig,
    pub ext_schemas: &'a [std::path::PathBuf],
    pub enable_handlers: bool,
    pub surface_override: Option<lex_extension_host::Surface>,
}

/// CLI surface installs this prompt: trust prompts in CLI mode are
/// a TTY-interrupt anti-pattern, so we always deny and direct the
/// user at `--enable-handlers`.
struct CliPromptHandler;
impl TrustPromptHandler for CliPromptHandler {
    fn prompt(&self, ctx: &TrustPromptContext) -> TrustDecision {
        TrustDecision::Denied {
            reason: format!(
                "subprocess handler `{}` requires --enable-handlers in CLI mode",
                ctx.namespace
            ),
        }
    }
}

/// Boot the registry for the CLI surface. Thin wrapper that fills in
/// the [`CliPromptHandler`] and forwards to [`lex_fmt::setup::boot_registry`].
pub fn boot_registry(setup: ExtensionSetup<'_>) -> BootOutcome {
    lex_fmt::setup::boot_registry(lex_fmt::setup::ExtensionSetup {
        workspace_root: setup.workspace_root,
        labels_config: setup.labels_config,
        ext_schemas: setup.ext_schemas,
        enable_handlers: setup.enable_handlers,
        surface_override: setup.surface_override,
        trust_prompt: Box::new(CliPromptHandler),
        // Reports `lexd`'s version to subprocess handlers in their
        // initialize handshake — what handlers expect to see, not the
        // `lex-fmt` boot crate's version.
        host_version: env!("CARGO_PKG_VERSION"),
    })
}

#[cfg(test)]
mod tests {
    //! CLI-side smoke tests. The exhaustive coverage of every
    //! [`build_handler`] branch lives in `lex-fmt::setup::tests`;
    //! the tests here verify the CLI shim wires the
    //! [`CliPromptHandler`] correctly and the deny rationale mentions
    //! `--enable-handlers` (which is what end users see).
    use super::*;
    use lex_config::LabelsConfig;
    use lex_extension_host::Surface;

    #[test]
    fn cli_subprocess_handler_without_enable_flag_says_enable_handlers() {
        let workspace = tempfile::tempdir().unwrap();
        let acme_dir = workspace.path().join("acme");
        std::fs::create_dir_all(&acme_dir).unwrap();
        std::fs::write(
            acme_dir.join("task.yaml"),
            "schema_version: 1\nlabel: acme.task\nhandler:\n  transport: subprocess\n  command: [\"acme-handler\"]\n",
        )
        .unwrap();

        let labels = LabelsConfig::default();
        let outcome = boot_registry(ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[acme_dir.clone()],
            enable_handlers: false,
            surface_override: Some(Surface::CliOneShot),
        });
        assert!(outcome.registered.iter().any(|r| r.name == "acme"));
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.namespace.as_deref() == Some("acme")
                    && d.message.contains("--enable-handlers")),
            "expected --enable-handlers diagnostic, got: {:?}",
            outcome.diagnostics
        );
    }
}
