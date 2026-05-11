//! Trust decision matrix.
//!
//! [`TrustGate::evaluate`] takes the four input axes (source, surface,
//! transport, capability) and returns a [`TrustDecision`]. The
//! decision encodes the β/γ-correct policy described in the
//! master-issue correction #1: subprocess handlers always require
//! explicit approval; declared `capabilities: { fs/net: false }` is
//! ignored until PR 12 lands OS-level enforcement.

use super::store::{TrustKey, TrustStore};

/// Where the schema came from. Combined with [`Surface`] and
/// [`Transport`] this determines whether the handler may run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// `--ext-schema ./local.yaml` — user passed the schema path
    /// explicitly on the command line.
    LocalFile { path: std::path::PathBuf },
    /// `[labels]` block in `lex.toml` — the workspace owner declared
    /// the namespace.
    LexTomlNamespace { name: String },
    /// Schema fetched from a marketplace / registry / cache. The host
    /// did not see an explicit user gesture pointing at this schema.
    CacheOnly { uri: String },
}

/// Which host surface is consulting the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// `lexd` CLI, single-shot. No interactive prompt.
    CliOneShot,
    /// `lex-lsp` running inside an editor. Has the prompt callback.
    LspSession,
    /// CI environment, auto-detected from env vars (see
    /// [`detect_ci_environment`]).
    Ci,
}

/// Schema's declared capability set. Stored on the evaluator and
/// passed to the matrix for forward-compat with PR 12; today it does
/// not influence the decision (β/γ matrix prompts subprocess
/// regardless).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// `capabilities: { fs: false, net: false }` — the handler
    /// declares it doesn't need filesystem or network access. Trusted
    /// to run under sandbox once PR 12 ships.
    Pure,
    /// At least one of `fs: true` or `net: true`. Always prompts even
    /// post-δ.
    Full,
}

impl Capability {
    /// Build from the schema's `Capabilities` struct. Maps the bool
    /// pair to the binary classifier the gate cares about.
    pub fn from_schema(caps: lex_extension::schema::Capabilities) -> Self {
        if caps.is_pure() {
            Capability::Pure
        } else {
            Capability::Full
        }
    }
}

/// Which transport the handler will use. The gate's matrix only
/// distinguishes Native (trusted by linkage) from everything else
/// (Subprocess and Wasm both prompt). Wasm shouldn't reach the gate
/// — the schema loader rejects it — but the variant is here so a
/// future enable can be a single-line matrix change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Native,
    Subprocess,
    Wasm,
}

impl Transport {
    /// Build from the schema's `HandlerTransport`.
    pub fn from_schema(t: lex_extension::schema::HandlerTransport) -> Self {
        match t {
            lex_extension::schema::HandlerTransport::Native => Transport::Native,
            lex_extension::schema::HandlerTransport::Subprocess => Transport::Subprocess,
            lex_extension::schema::HandlerTransport::Wasm => Transport::Wasm,
            // HandlerTransport is #[non_exhaustive]; conservatively
            // treat unknown variants as Subprocess — they'll prompt,
            // which is the safer default than silently allowing.
            _ => Transport::Subprocess,
        }
    }
}

/// The verdict the gate returns for one (source, surface, transport,
/// capability) tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustDecision {
    /// Handler may run.
    Trusted,
    /// Handler may NOT run. The string is a user-facing diagnostic
    /// explaining why and what to do (e.g., "use --enable-handlers").
    Denied { reason: String },
    /// LSP-only: prompt the user via the [`TrustPromptHandler`]
    /// callback. The result is pinned in the trust store keyed by the
    /// `(workspace, namespace, command_string)` tuple inside
    /// [`TrustPromptContext`].
    Pending,
}

/// Context handed to a [`TrustPromptHandler`] when the gate needs a
/// user decision. The same fields make up the [`TrustKey`] that
/// pins the answer in the trust store, so a re-prompt only happens
/// when one of those changes (typically the `command_string` after
/// a schema bump).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustPromptContext {
    pub namespace: String,
    /// The schema's `handler.command` joined into a single string.
    /// Pin granularity — a different command string means a new
    /// prompt.
    pub command_string: String,
    /// Where the schema came from. Surfaces in the prompt UI so the
    /// user can tell `--ext-schema ./acme.yaml` from a
    /// marketplace-fetched namespace.
    pub source: Source,
    /// What the schema declares it needs. Surfaces in the prompt UI
    /// for the user's awareness; doesn't change the matrix outcome
    /// in β/γ.
    pub capability: Capability,
}

/// User-defined callback the gate invokes when [`TrustDecision::Pending`]
/// is reached. CLI installs a callback that returns `Denied`
/// (interactive trust prompts in CLI mode would be a mid-pipeline
/// TTY interrupt — instead, the policy is "use `--enable-handlers`
/// upfront or it's denied"). LSP installs a callback that surfaces
/// a `lex/trustRequest` notification and waits for the editor's
/// response (PR 10).
pub trait TrustPromptHandler: Send + Sync {
    /// Decide trust for one prompt context. The returned variant must
    /// be either [`TrustDecision::Trusted`] or
    /// [`TrustDecision::Denied`] — returning `Pending` is a
    /// programmer error and is treated as `Denied`.
    fn prompt(&self, ctx: &TrustPromptContext) -> TrustDecision;
}

/// The gate. Constructed once per host session with a [`Surface`],
/// a [`TrustStore`] for persistence, and a [`TrustPromptHandler`]
/// for the LSP path. Owned (not `Clone`): the contained
/// `Box<dyn TrustPromptHandler>` and the file-backed `TrustStore`
/// don't have a sensible cheap copy, and the gate is a session-
/// scoped singleton in practice. Wrap in `Arc<Mutex<…>>` if a
/// caller really needs shared mutability.
pub struct TrustGate {
    surface: Surface,
    /// `--enable-handlers` flag — when set, CLI/CI surfaces treat
    /// subprocess invocations as `Trusted` for the run.
    enable_handlers: bool,
    store: TrustStore,
    prompt: Box<dyn TrustPromptHandler>,
    /// OS-level enforcement available to the host. With
    /// [`crate::sandbox::NullSandbox`] (the default for PR
    /// 12-plumbing), [`crate::sandbox::Sandbox::available`] returns
    /// `false` and the post-δ pure-handler auto-trust path is
    /// inactive — behaviour matches β/γ. PR 12d (the matrix flip)
    /// becomes a one-line consult here.
    sandbox: Box<dyn crate::sandbox::Sandbox>,
}

impl TrustGate {
    pub fn new(
        surface: Surface,
        enable_handlers: bool,
        store: TrustStore,
        prompt: Box<dyn TrustPromptHandler>,
    ) -> Self {
        Self {
            surface,
            enable_handlers,
            store,
            prompt,
            sandbox: Box::new(crate::sandbox::NullSandbox),
        }
    }

    /// Install an OS-level sandbox for post-δ auto-trust of declared-
    /// pure handlers. Replaces the default
    /// [`crate::sandbox::NullSandbox`]. Today (PR 12-plumbing) the
    /// gate consults [`crate::sandbox::Sandbox::available`] but the
    /// only available impls report `false`, so behaviour doesn't
    /// change yet. PR 12a/b/c ship per-OS impls; PR 12d flips the
    /// matrix to actually use the result.
    pub fn set_sandbox(&mut self, sandbox: Box<dyn crate::sandbox::Sandbox>) {
        self.sandbox = sandbox;
    }

    /// Borrow the sandbox installed on this gate. Mainly here for
    /// host-side diagnostics (e.g., "this workspace would auto-
    /// trust pure handlers" status output).
    pub fn sandbox(&self) -> &dyn crate::sandbox::Sandbox {
        self.sandbox.as_ref()
    }

    /// Surface the gate was constructed with. Useful for diagnostics
    /// that want to mention the active mode.
    pub fn surface(&self) -> Surface {
        self.surface
    }

    /// Whether `--enable-handlers` was set.
    pub fn enable_handlers(&self) -> bool {
        self.enable_handlers
    }

    /// Apply the matrix to one handler invocation.
    ///
    /// `command_string` is the schema's `handler.command` joined by
    /// spaces — this is what the trust store keys on for pin
    /// granularity. A different command string is a different trust
    /// decision.
    pub fn evaluate(
        &mut self,
        source: &Source,
        transport: Transport,
        capability: Capability,
        namespace: &str,
        command_string: &str,
    ) -> TrustDecision {
        // Native handlers run by linkage. Bundled `lex.*` built-ins
        // hit this path; PR 12d will extend it to declared-pure
        // subprocess handlers under an enforced sandbox.
        if matches!(transport, Transport::Native) {
            return TrustDecision::Trusted;
        }

        // WASM should never reach the gate (schema loader rejects).
        // If it does — defence in depth — treat as denied.
        if matches!(transport, Transport::Wasm) {
            return TrustDecision::Denied {
                reason: "WASM handlers are not yet supported".into(),
            };
        }

        // Post-δ pure-handler auto-trust path (the matrix flip
        // tracked at lex#528 / PR 12d). Only fires when both:
        //   1. the handler declared `pure` capabilities, AND
        //   2. an OS-level sandbox is available to enforce that
        //      declaration on the running platform.
        //
        // PR 12-plumbing wires this consultation but ships
        // [`crate::sandbox::NullSandbox`] as the default, which
        // reports `available() == false` everywhere. PR 12a/b/c add
        // real per-OS impls; PR 12d switches the default install in
        // `lex-engine` from `NullSandbox` to the OS-appropriate
        // impl so this branch starts firing.
        //
        // Independent of `surface`: a pure handler under an enforced
        // sandbox is trustworthy regardless of whether we're in CLI,
        // CI, or LSP mode.
        if matches!(capability, Capability::Pure) && self.sandbox.available() {
            return TrustDecision::Trusted;
        }

        match self.surface {
            Surface::CliOneShot | Surface::Ci => {
                if self.enable_handlers {
                    TrustDecision::Trusted
                } else {
                    TrustDecision::Denied {
                        reason: format!(
                            "subprocess handler `{namespace}` requires --enable-handlers in {} mode",
                            match self.surface {
                                Surface::Ci => "CI",
                                _ => "CLI",
                            }
                        ),
                    }
                }
            }
            Surface::LspSession => {
                let key = TrustKey {
                    namespace: namespace.to_string(),
                    command_string: command_string.to_string(),
                };
                if let Some(stored) = self.store.get(&key) {
                    return stored.clone();
                }
                let ctx = TrustPromptContext {
                    namespace: namespace.to_string(),
                    command_string: command_string.to_string(),
                    source: source.clone(),
                    capability,
                };
                let decision = self.prompt.prompt(&ctx);
                let to_store = match &decision {
                    TrustDecision::Trusted => Some(decision.clone()),
                    TrustDecision::Denied { .. } => Some(decision.clone()),
                    // Programmer error — `prompt()` must not return
                    // Pending. Treat as Denied for safety; don't
                    // persist (the prompt may be retriable on a
                    // subsequent session).
                    TrustDecision::Pending => None,
                };
                if let Some(decision_to_store) = to_store {
                    if let Err(e) = self.store.set(key, decision_to_store) {
                        // Persist failed — most often a read-only
                        // workspace. The store's atomicity contract
                        // guarantees in-memory matches disk, so the
                        // pin really wasn't recorded. We honor the
                        // prompt's verdict for *this* session
                        // (returning `decision` below) and log the
                        // failure so the user can see why their
                        // approval isn't sticking. Next session
                        // they'll be prompted again with the same
                        // diagnostic visible.
                        eprintln!(
                            "[lex-extension-host] trust store persist failed for `{namespace}`: {e}; approval applies for this session only"
                        );
                    }
                }
                match decision {
                    TrustDecision::Pending => TrustDecision::Denied {
                        reason: format!(
                            "trust prompt for `{namespace}` returned Pending — treating as denied"
                        ),
                    },
                    other => other,
                }
            }
        }
    }

    /// Borrow the underlying store for inspection. Tests use this; PR
    /// 10's editor UI will too (so it can show "currently trusted
    /// namespaces").
    pub fn store(&self) -> &TrustStore {
        &self.store
    }
}

/// Detect whether the host process is running in a CI environment.
///
/// Checks the standard set of env vars shipped by major providers
/// (the `CI` superset variable plus a few well-known specific
/// flags). Returns `true` if any one is set, regardless of value.
///
/// This is the auto-detection the `lexd` CLI uses to choose
/// [`Surface::Ci`] over [`Surface::CliOneShot`] when no explicit
/// surface override is supplied.
pub fn detect_ci_environment<F>(env_lookup: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    const CI_VARS: &[&str] = &[
        "CI",
        "CONTINUOUS_INTEGRATION",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "BUILDKITE",
        "CIRCLECI",
        "TRAVIS",
        "JENKINS_URL",
    ];
    CI_VARS.iter().any(|v| env_lookup(v).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Always returns the configured decision. Used to drive
    /// matrix-cell tests for the LSP path without touching the
    /// real prompt UI.
    struct FixedPrompt(TrustDecision);
    impl TrustPromptHandler for FixedPrompt {
        fn prompt(&self, _ctx: &TrustPromptContext) -> TrustDecision {
            self.0.clone()
        }
    }

    fn store_in_tmp() -> (TrustStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TrustStore::open(dir.path()).expect("open");
        (store, dir)
    }

    fn gate_with_surface(
        surface: Surface,
        enable_handlers: bool,
        prompt_decision: TrustDecision,
    ) -> (TrustGate, tempfile::TempDir) {
        let (store, dir) = store_in_tmp();
        let gate = TrustGate::new(
            surface,
            enable_handlers,
            store,
            Box::new(FixedPrompt(prompt_decision)),
        );
        (gate, dir)
    }

    #[test]
    fn native_is_trusted_under_every_surface() {
        for surface in [Surface::CliOneShot, Surface::LspSession, Surface::Ci] {
            let (mut gate, _dir) = gate_with_surface(
                surface,
                false,
                TrustDecision::Denied {
                    reason: "should not be called".into(),
                },
            );
            let d = gate.evaluate(
                &Source::LexTomlNamespace { name: "lex".into() },
                Transport::Native,
                Capability::Full,
                "lex",
                "/usr/bin/never-spawned",
            );
            assert_eq!(d, TrustDecision::Trusted, "surface={surface:?}");
        }
    }

    #[test]
    fn cli_subprocess_without_flag_is_denied() {
        let (mut gate, _dir) = gate_with_surface(
            Surface::CliOneShot,
            false,
            TrustDecision::Denied {
                reason: "n/a".into(),
            },
        );
        let d = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Pure,
            "acme",
            "acme-handler",
        );
        match d {
            TrustDecision::Denied { reason } => {
                assert!(reason.contains("--enable-handlers"));
                assert!(reason.contains("acme"));
            }
            other => panic!("expected Denied, got: {other:?}"),
        }
    }

    #[test]
    fn cli_subprocess_with_flag_is_trusted() {
        let (mut gate, _dir) = gate_with_surface(
            Surface::CliOneShot,
            true,
            TrustDecision::Denied {
                reason: "n/a".into(),
            },
        );
        let d = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Pure,
            "acme",
            "acme-handler",
        );
        assert_eq!(d, TrustDecision::Trusted);
    }

    #[test]
    fn cli_with_flag_does_not_persist_to_store() {
        let (mut gate, _dir) = gate_with_surface(
            Surface::CliOneShot,
            true,
            TrustDecision::Denied {
                reason: "n/a".into(),
            },
        );
        gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Pure,
            "acme",
            "acme-handler",
        );
        let key = TrustKey {
            namespace: "acme".into(),
            command_string: "acme-handler".into(),
        };
        assert!(
            gate.store().get(&key).is_none(),
            "CLI --enable-handlers must not persist trust"
        );
    }

    #[test]
    fn ci_subprocess_without_flag_is_denied() {
        let (mut gate, _dir) = gate_with_surface(
            Surface::Ci,
            false,
            TrustDecision::Denied {
                reason: "n/a".into(),
            },
        );
        let d = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Pure,
            "acme",
            "acme-handler",
        );
        match d {
            TrustDecision::Denied { reason } => assert!(reason.contains("CI")),
            other => panic!("expected Denied, got: {other:?}"),
        }
    }

    #[test]
    fn ci_subprocess_with_flag_is_trusted() {
        let (mut gate, _dir) = gate_with_surface(
            Surface::Ci,
            true,
            TrustDecision::Denied {
                reason: "n/a".into(),
            },
        );
        let d = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Pure,
            "acme",
            "acme-handler",
        );
        assert_eq!(d, TrustDecision::Trusted);
    }

    #[test]
    fn lsp_first_call_invokes_prompt_and_persists_trusted() {
        let (mut gate, _dir) =
            gate_with_surface(Surface::LspSession, false, TrustDecision::Trusted);
        let d = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Pure,
            "acme",
            "acme-handler",
        );
        assert_eq!(d, TrustDecision::Trusted);
        // Pinned for next time.
        let key = TrustKey {
            namespace: "acme".into(),
            command_string: "acme-handler".into(),
        };
        assert_eq!(gate.store().get(&key), Some(&TrustDecision::Trusted));
    }

    #[test]
    fn lsp_subsequent_call_uses_pinned_decision_without_prompt() {
        // Prompt would deny — but the store was pre-populated as
        // Trusted, so the gate must short-circuit.
        let (store, _dir) = store_in_tmp();
        let mut store = store;
        let key = TrustKey {
            namespace: "acme".into(),
            command_string: "acme-handler".into(),
        };
        store.set(key.clone(), TrustDecision::Trusted).unwrap();

        let mut gate = TrustGate::new(
            Surface::LspSession,
            false,
            store,
            Box::new(FixedPrompt(TrustDecision::Denied {
                reason: "MUST NOT FIRE".into(),
            })),
        );
        let d = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Pure,
            "acme",
            "acme-handler",
        );
        assert_eq!(d, TrustDecision::Trusted);
    }

    #[test]
    fn lsp_command_string_change_re_prompts() {
        // Pin trust for the v1 command, then ask about a v2 command.
        // The store key includes command_string, so the second call
        // misses and re-prompts.
        let (store, _dir) = store_in_tmp();
        let mut store = store;
        store
            .set(
                TrustKey {
                    namespace: "acme".into(),
                    command_string: "acme-handler-v1".into(),
                },
                TrustDecision::Trusted,
            )
            .unwrap();

        let mut gate = TrustGate::new(
            Surface::LspSession,
            false,
            store,
            Box::new(FixedPrompt(TrustDecision::Denied {
                reason: "v2 command needs fresh approval".into(),
            })),
        );
        let d = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Pure,
            "acme",
            "acme-handler-v2",
        );
        match d {
            TrustDecision::Denied { reason } => {
                assert!(reason.contains("v2"));
            }
            other => panic!("expected fresh prompt to deny, got: {other:?}"),
        }
    }

    #[test]
    fn lsp_denied_decision_persists() {
        // Denied decisions are also pinned so a future session
        // doesn't re-prompt unless the command changes.
        let (mut gate, _dir) = gate_with_surface(
            Surface::LspSession,
            false,
            TrustDecision::Denied {
                reason: "user rejected".into(),
            },
        );
        let _ = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Pure,
            "acme",
            "acme-handler",
        );
        let key = TrustKey {
            namespace: "acme".into(),
            command_string: "acme-handler".into(),
        };
        assert!(matches!(
            gate.store().get(&key),
            Some(TrustDecision::Denied { .. })
        ));
    }

    #[test]
    fn wasm_transport_is_denied_defensively() {
        // Schema loader rejects WASM upfront so the gate shouldn't
        // see it, but if it does the matrix denies rather than
        // silently accepts.
        let (mut gate, _dir) = gate_with_surface(Surface::CliOneShot, true, TrustDecision::Trusted);
        let d = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Wasm,
            Capability::Pure,
            "acme",
            "acme.wasm",
        );
        assert!(matches!(d, TrustDecision::Denied { .. }));
    }

    #[test]
    fn ci_detection_recognises_standard_env_vars() {
        for var in ["CI", "GITHUB_ACTIONS", "GITLAB_CI", "BUILDKITE", "CIRCLECI"] {
            let lookup = |name: &str| -> Option<String> {
                if name == var {
                    Some("1".into())
                } else {
                    None
                }
            };
            assert!(detect_ci_environment(lookup), "var={var}");
        }
    }

    #[test]
    fn ci_detection_returns_false_when_no_var_set() {
        assert!(!detect_ci_environment(|_| None));
    }

    // ───────── Sandbox plumbing (lex#528, PR 12-plumbing) ─────────

    /// Test sandbox impl that reports availability per its
    /// constructor flag and is a no-op in `apply_to`. Used to drive
    /// the post-δ auto-trust path without depending on any real OS
    /// sandbox.
    struct FixedAvailabilitySandbox(bool);
    impl crate::sandbox::Sandbox for FixedAvailabilitySandbox {
        fn apply_to(
            &self,
            _cmd: &mut std::process::Command,
            _caps: lex_extension::schema::Capabilities,
        ) -> Result<(), crate::sandbox::SandboxError> {
            Ok(())
        }
        fn available(&self) -> bool {
            self.0
        }
    }

    #[test]
    fn default_gate_installs_null_sandbox_which_reports_unavailable() {
        let (gate, _dir) = gate_with_surface(
            Surface::LspSession,
            false,
            TrustDecision::Denied {
                reason: "n/a".into(),
            },
        );
        // Default sandbox is NullSandbox, which always reports
        // unavailable. The post-δ auto-trust branch never fires.
        assert!(!gate.sandbox().available());
    }

    #[test]
    fn pure_handler_auto_trusts_when_sandbox_available() {
        // PR 12d-style behavior, tested with a mock sandbox so the
        // plumbing PR can lock in the contract before any per-OS
        // impl ships. With sandbox.available() == true and the
        // handler declaring pure capabilities, the gate auto-trusts
        // without consulting the prompt — regardless of surface.
        for surface in [Surface::CliOneShot, Surface::LspSession, Surface::Ci] {
            let (mut gate, _dir) = gate_with_surface(
                surface,
                false,
                TrustDecision::Denied {
                    reason: "prompt should not fire".into(),
                },
            );
            gate.set_sandbox(Box::new(FixedAvailabilitySandbox(true)));
            let d = gate.evaluate(
                &Source::LexTomlNamespace {
                    name: "acme".into(),
                },
                Transport::Subprocess,
                Capability::Pure,
                "acme",
                "acme-handler",
            );
            assert_eq!(d, TrustDecision::Trusted, "surface={surface:?}");
        }
    }

    #[test]
    fn full_capability_handler_does_not_auto_trust_even_under_sandbox() {
        // Auto-trust is reserved for `pure` declarations. A handler
        // that declared `fs: true` or `net: true` still prompts /
        // requires --enable-handlers, because the sandbox can only
        // enforce what was declared.
        let (mut gate, _dir) = gate_with_surface(
            Surface::CliOneShot,
            false,
            TrustDecision::Denied {
                reason: "n/a".into(),
            },
        );
        gate.set_sandbox(Box::new(FixedAvailabilitySandbox(true)));
        let d = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Full,
            "acme",
            "acme-handler",
        );
        match d {
            TrustDecision::Denied { reason } => {
                assert!(reason.contains("--enable-handlers"));
            }
            other => panic!("expected Denied (full caps still prompts), got: {other:?}"),
        }
    }

    #[test]
    fn pure_handler_falls_back_to_prompt_when_sandbox_unavailable() {
        // The path PR 12-plumbing ships with NullSandbox: pure
        // handler, but no enforced sandbox, so behaviour matches
        // β/γ — CLI without the flag is denied, LSP would prompt.
        let (mut gate, _dir) = gate_with_surface(
            Surface::CliOneShot,
            false,
            TrustDecision::Denied {
                reason: "n/a".into(),
            },
        );
        gate.set_sandbox(Box::new(FixedAvailabilitySandbox(false)));
        let d = gate.evaluate(
            &Source::LexTomlNamespace {
                name: "acme".into(),
            },
            Transport::Subprocess,
            Capability::Pure,
            "acme",
            "acme-handler",
        );
        match d {
            TrustDecision::Denied { reason } => {
                assert!(reason.contains("--enable-handlers"));
            }
            other => panic!("expected Denied without enforced sandbox, got: {other:?}"),
        }
    }
}
