//! Lazy boot of the extension registry for an LSP session.
//!
//! The registry is built on first extension-aware request and cached for the
//! life of the workspace. Boot does blocking filesystem IO (schema load, trust
//! store) and may spawn subprocess handlers, so it runs on the blocking pool;
//! a mutex serializes concurrent first-requests into a single boot.

use std::sync::Arc;

use lex_config::LabelsConfig;
use tower_lsp::lsp_types::MessageType;

use crate::extension_dispatch::LspExtensionState;
use crate::server::{LexLanguageServer, LspClient};

impl<C> LexLanguageServer<C>
where
    C: LspClient,
{
    /// Lazily boot the extension registry against the current workspace
    /// root + config. Idempotent: once built, returns the cached state.
    /// Returns `None` when no workspace is set (e.g. single-file mode);
    /// extension dispatch is a no-op without a workspace anchor.
    ///
    /// Concurrency: the first request to land on a fresh workspace
    /// takes the `extension_init` mutex and runs the boot; every
    /// other concurrent request blocks on the mutex, then re-checks
    /// the cache and reuses what the first one produced. Without
    /// this serialization, an open-file event that fires several
    /// providers at once (hover, completion, semantic tokens, folding,
    /// document-symbols, …) would launch N parallel boots, with N
    /// concurrent reads of the schema directory, N concurrent
    /// subprocess spawns, and N `lex/trustRequest` prompts to the
    /// editor. The mutex keeps the observable side effects to one
    /// prompt and one set of spawns.
    pub(crate) async fn extension_state(&self) -> Option<Arc<LspExtensionState>> {
        // Fast path: already booted, no lock needed.
        if let Some(s) = self.extension.read().await.clone() {
            return Some(s);
        }

        // Slow path: serialize boot. Hold the init lock for the whole
        // boot so the second-arriving request waits for the first.
        let _guard = self.extension_init.lock().await;

        // Re-check after acquiring the init lock — another task may
        // have completed boot while we were waiting on the mutex.
        if let Some(s) = self.extension.read().await.clone() {
            return Some(s);
        }

        let workspace_root = {
            let roots = self.workspace_roots.read().await;
            roots.first().cloned()?
        };
        let labels_config = LabelsConfig {
            namespaces: self.config.read().await.config.labels.clone(),
        };

        // boot_registry does synchronous filesystem IO (schema load,
        // trust store open) and may attempt to spawn subprocess
        // handlers — a few hundred milliseconds in the worst case. Run
        // it on the blocking thread pool so the tokio runtime keeps
        // serving other LSP requests while boot runs.
        //
        // The trust prompt handler bridges sync→async via
        // `Handle::block_on` — safe because we're on a blocking-pool
        // thread, not a runtime worker.
        let workspace_root_owned = workspace_root.clone();
        let trust_requester = std::sync::Arc::new(self.client.clone());
        let runtime_handle = tokio::runtime::Handle::current();
        let outcome = match tokio::task::spawn_blocking(move || {
            lex_fmt::boot_registry(lex_fmt::ExtensionSetup {
                workspace_root: workspace_root_owned.as_path(),
                labels_config: &labels_config,
                // The LSP server has no `--ext-schema` flag; only
                // `[labels]` entries from `lex.toml` register schemas
                // in this surface.
                ext_schemas: &[],
                // `enable_handlers` is irrelevant on the Lsp surface —
                // that flag is the CLI shortcut for the trust-prompt
                // path. The LSP consults the trust store + prompt
                // handler directly.
                enable_handlers: false,
                surface_override: Some(lex_extension_host::Surface::LspSession),
                // Forwards `lex/trustRequest` to the editor and awaits
                // the user's decision. Already-pinned decisions in
                // `<workspace>/.lex/trust.json` short-circuit before
                // the prompt fires.
                trust_prompt: Box::new(crate::trust_prompt::LspPromptHandler::new(
                    trust_requester,
                    runtime_handle,
                )),
                // Reports `lexd-lsp`'s version to subprocess handlers
                // in their initialize handshake — what handlers expect
                // to see, not the `lex-fmt` boot crate's version.
                host_version: env!("CARGO_PKG_VERSION"),
            })
        })
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => {
                // Blocking task panicked or was cancelled. Skip
                // extension boot for this session; the next request
                // will retry.
                return None;
            }
        };

        // Surface boot diagnostics to the editor before we cache the
        // state. Per-namespace failures (resolver errors, denied
        // subprocess handlers, schema load problems) are stored on
        // the outcome but the user has no way to see them otherwise
        // — pre-validation diagnostics are attached to documents,
        // but boot diagnostics aren't. `window/showMessage` is the
        // right surface for one-shot status that's not tied to a
        // specific document range.
        for diag in &outcome.diagnostics {
            let prefix = match &diag.namespace {
                Some(ns) => format!("lex extension `{ns}`: "),
                None => "lex extensions: ".to_string(),
            };
            self.client
                .show_message(MessageType::WARNING, format!("{prefix}{}", diag.message))
                .await;
        }

        // Cross-check `[diagnostics.rules]` extension entries against the
        // freshly-booted registry. A `<namespace>.<code>` rule whose
        // namespace is registered but doesn't declare the code is a dead
        // letter — it retunes nothing. Surface each as a warning so the
        // misspelling is visible; like boot diagnostics, it's session
        // status with no document range to attach to. Unregistered
        // namespaces pass silently (the user may install the extension
        // later), so this never fires for staged-ahead rules.
        // Collect findings under the lock, then drop it before awaiting
        // any `show_message` — holding the config read lock across the
        // network await could starve a concurrent config write.
        let rule_findings = {
            let cfg = self.config.read().await;
            lex_fmt::validate_extension_diagnostic_rules(
                &cfg.extension_diagnostic_rules,
                &outcome.registry,
            )
        };
        for finding in rule_findings {
            self.client
                .show_message(MessageType::WARNING, finding.message)
                .await;
        }

        let state = Arc::new(LspExtensionState::from(outcome));
        *self.extension.write().await = Some(state.clone());
        Some(state)
    }

    /// Discard the cached extension registry. Called when workspace
    /// folders change so the next request picks up the new root +
    /// config.
    pub(crate) async fn invalidate_extension_state(&self) {
        *self.extension.write().await = None;
    }
}
