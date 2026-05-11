//! [`NullSandbox`] — the no-op fallback used when no OS-specific
//! sandbox is implemented (or available) for the running platform.
//!
//! Behavior: [`Sandbox::apply_to`] makes no changes to the command;
//! [`Sandbox::available`] returns `false`. This means the trust gate
//! (when consulted in PR 12d) will keep prompting for every
//! subprocess handler, exactly as it does today in β/γ.
//!
//! The plumbing PR (this one) installs `NullSandbox` as the default
//! on every platform. Per-OS PRs (12a/b/c) ship concrete impls
//! behind feature flags and `#[cfg(target_os = ...)]`.

use lex_extension::schema::Capabilities;

use super::{Sandbox, SandboxError};

/// No-op sandbox. Always returns "not available"; never modifies the
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

    fn available(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_sandbox_reports_unavailable() {
        let s = NullSandbox;
        assert!(!s.available());
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
