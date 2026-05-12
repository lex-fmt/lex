//! Stock [`TrustPromptHandler`] implementations for embedders.
//!
//! Hosts that ship as part of the lex toolchain (the `lexd` CLI, the
//! `lexd-lsp` server) install their own [`TrustPromptHandler`]s with
//! host-specific UX. Third-party Rust embedders building docs
//! pipelines / publishing servers / batch tools usually don't need
//! that level of customisation — they need a way to say "deny every
//! subprocess handler" (the safe default for non-interactive use) or
//! "trust every subprocess handler" (acceptable in tightly-controlled
//! test environments). This module supplies both.
//!
//! See [`crate::EngineBuilder::trust_prompt`] for installation; if
//! the embedder doesn't call `trust_prompt`, the builder defaults to
//! [`AutoDenyPrompt`].

use lex_extension_host::{TrustDecision, TrustPromptContext, TrustPromptHandler};

/// Denies every prompt. Recommended default for batch / non-interactive
/// embedders (docs pipelines, publishing servers, CI tasks).
///
/// Subprocess handlers declared in `lex.toml`'s `[labels]` block or
/// passed via `EngineBuilder::ext_schema_dir` register schema-only —
/// pre-validation in `Engine::analyze` still catches typos / wrong
/// param types, but the handlers themselves don't run. Native
/// handlers registered via [`crate::EngineBuilder::with_native_namespace`]
/// are unaffected (they're trusted by linkage).
pub struct AutoDenyPrompt;

impl TrustPromptHandler for AutoDenyPrompt {
    fn prompt(&self, ctx: &TrustPromptContext) -> TrustDecision {
        TrustDecision::Denied {
            reason: format!(
                "subprocess handler `{}` denied by AutoDenyPrompt; install a custom \
                 TrustPromptHandler via EngineBuilder::trust_prompt to allow it",
                ctx.namespace
            ),
        }
    }
}

/// Trusts every prompt. **Intended for tests only.** Emits one
/// stderr warning per invocation so production misuse leaves a paper
/// trail in CI logs.
///
/// The β/γ enforcement policy is "subprocess handlers always prompt"
/// — declared `capabilities: { fs: false, net: false }` is not
/// kernel-enforced until PR 12's OS-level sandboxing lands. The
/// proposal §8 trust matrix (with auto-trust for declared-pure
/// handlers) represents the post-δ target. Until then,
/// auto-trusting via this prompt gives every subprocess handler
/// ambient host privilege — which is fine for fixture-driven tests,
/// not for anything else.
pub struct AutoTrustPrompt;

impl TrustPromptHandler for AutoTrustPrompt {
    fn prompt(&self, ctx: &TrustPromptContext) -> TrustDecision {
        // Repeated stderr line per prompt is intentional — makes the
        // warning visible across the per-namespace boot diagnostics
        // that hosts already log.
        eprintln!(
            "[lex-fmt] AutoTrustPrompt unconditionally trusting subprocess handler `{}` \
             (command: `{}`) — DO NOT USE IN PRODUCTION",
            ctx.namespace, ctx.command_string
        );
        TrustDecision::Trusted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_extension_host::{Capability, Source};

    fn ctx(namespace: &str) -> TrustPromptContext {
        TrustPromptContext {
            namespace: namespace.into(),
            command_string: "test-binary".into(),
            source: Source::LocalFile {
                path: std::path::PathBuf::from("/test"),
            },
            capability: Capability::Pure,
        }
    }

    #[test]
    fn auto_deny_returns_denied_with_namespace_in_reason() {
        let decision = AutoDenyPrompt.prompt(&ctx("acme"));
        match decision {
            TrustDecision::Denied { reason } => {
                assert!(reason.contains("acme"));
                assert!(reason.contains("AutoDenyPrompt"));
            }
            other => panic!("expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn auto_trust_returns_trusted() {
        let decision = AutoTrustPrompt.prompt(&ctx("acme"));
        assert_eq!(decision, TrustDecision::Trusted);
    }
}
