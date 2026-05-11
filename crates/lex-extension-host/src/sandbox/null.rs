//! [`NullSandbox`] — the no-op fallback used when no OS-specific
//! sandbox is implemented (or available) for the running platform.
//!
//! Behavior: [`Sandbox::apply_to`] makes no changes to the command;
//! [`Sandbox::supports`] returns `false` for every capability set.
//! This means the trust gate (when consulted in PR 12d) will keep
//! prompting for every subprocess handler, exactly as it does today
//! in β/γ.
//!
//! The plumbing PR (this one) installs `NullSandbox` as the default
//! on every platform. Per-OS PRs (12a/b/c) ship concrete impls
//! behind feature flags and `#[cfg(target_os = ...)]`.

use lex_extension::schema::Capabilities;

use super::{Sandbox, SandboxError};

/// No-op sandbox. Always reports "not supported"; never modifies the
/// command. The default the existing [`SubprocessHandler::spawn`]
/// passes through to [`SubprocessHandler::spawn_with_sandbox`] when
/// the host doesn't supply a concrete impl, and the default the
/// [`crate::TrustGate`] installs via [`crate::TrustGate::new`].
///
/// [`SubprocessHandler::spawn`]: crate::transport::SubprocessHandler::spawn
/// [`SubprocessHandler::spawn_with_sandbox`]: crate::transport::SubprocessHandler::spawn_with_sandbox
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSandbox;

impl Sandbox for NullSandbox {
    fn apply_to(
        &self,
        _cmd: &mut std::process::Command,
        _caps: Capabilities,
    ) -> Result<(), SandboxError> {
        Ok(())
    }

    fn supports(&self, _caps: Capabilities) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_sandbox_reports_unsupported_for_every_capability_shape() {
        let s = NullSandbox;
        assert!(!s.supports(Capabilities::default()));
        assert!(!s.supports(Capabilities {
            fs: true,
            net: false
        }));
        assert!(!s.supports(Capabilities {
            fs: true,
            net: true
        }));
    }

    #[test]
    fn null_sandbox_apply_is_a_no_op() {
        let s = NullSandbox;
        let mut cmd = std::process::Command::new("true");
        // Should not error regardless of capability shape.
        s.apply_to(&mut cmd, Capabilities::default()).unwrap();
        s.apply_to(
            &mut cmd,
            Capabilities {
                fs: true,
                net: true,
            },
        )
        .unwrap();
    }
}
