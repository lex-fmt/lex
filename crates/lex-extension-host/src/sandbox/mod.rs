//! OS-level sandboxing for subprocess handlers.
//!
//! The [`Sandbox`] trait is the facade over per-OS enforcement
//! mechanisms (Linux seccomp+landlock, macOS sandbox-exec, Windows
//! Job Objects + restricted tokens). Concrete impls live in sibling
//! files behind `#[cfg(target_os = "...")]` and a feature flag per
//! platform; the [`NullSandbox`] no-op fallback ships on every
//! platform and is the default.
//!
//! ## Why a facade
//!
//! Each OS exposes a sharply different abstraction:
//!
//! - Linux: per-syscall filtering (seccomp) + per-path fs rules
//!   (landlock). Fine-grained, programmatic.
//! - macOS: Scheme-based profile (`deny file-read* (subpath ...)`).
//!   Coarse but expressive. Backed by `sandbox-exec`.
//! - Windows: Job Objects + restricted tokens + AppContainer +
//!   Windows Filtering Platform. Per-resource-type, not
//!   per-syscall.
//!
//! Our use case ("spawn this handler subprocess with declared-pure
//! capabilities") is uniform; the facade isolates the per-OS
//! asymmetry so the rest of `lex-extension-host` has one API.
//!
//! ## Lifecycle
//!
//! 1. The host constructs a [`Sandbox`] impl appropriate for the
//!    running OS (or [`NullSandbox`] when no implementation exists).
//! 2. [`SubprocessHandler::with_sandbox`] stores the impl on the
//!    handler.
//! 3. Before spawning the child process, the handler calls
//!    [`Sandbox::apply_to`] with the declared
//!    [`lex_extension::schema::Capabilities`]. The impl modifies the
//!    [`std::process::Command`] in place (env vars, pre-exec hooks
//!    on Unix, restricted-token hand-off on Windows) so the kernel
//!    enforces the declared restrictions on the child.
//! 4. The trust gate consults [`Sandbox::available`] to decide
//!    whether a pure handler can auto-trust (post-δ matrix flip in
//!    PR 12d). Today's β/γ behavior — every subprocess prompts — is
//!    preserved as long as [`NullSandbox::available`] returns
//!    `false`.
//!
//! [`SubprocessHandler::with_sandbox`]: crate::transport::SubprocessHandler::with_sandbox

use std::error::Error;
use std::fmt;

use lex_extension::schema::Capabilities;

mod null;

pub use null::NullSandbox;

/// OS-level sandbox enforcement for subprocess handlers.
///
/// Implementations modify a `std::process::Command` so that the child
/// process can only perform the operations the [`Capabilities`]
/// argument permits. See the [module docs](self) for the lifecycle
/// and platform-specific notes.
pub trait Sandbox: Send + Sync {
    /// Apply the sandbox policy to the command. Called by the
    /// subprocess transport just before `spawn()`. Returns
    /// [`SandboxError`] if the policy can't be installed (e.g., the
    /// requested capability isn't enforceable on this platform).
    fn apply_to(
        &self,
        cmd: &mut std::process::Command,
        caps: Capabilities,
    ) -> Result<(), SandboxError>;

    /// True when this impl can enforce the declared capabilities on
    /// the running platform. The trust gate consults this when
    /// deciding whether a `pure` handler is eligible for auto-trust:
    /// only `true` shifts a `Pending` decision to `Trusted` without a
    /// prompt.
    ///
    /// [`NullSandbox::available`] always returns `false`. A real
    /// impl may also return `false` for capability sets it can't
    /// enforce (e.g., a future stricter capability not yet covered
    /// on a particular OS).
    fn available(&self) -> bool;
}

/// Errors a [`Sandbox`] implementation can surface when applying a
/// policy.
#[derive(Debug)]
pub enum SandboxError {
    /// The requested capability cannot be enforced on this platform.
    /// E.g., on Windows where syscall-level restrictions don't have
    /// a 1:1 mechanism. The caller (the trust gate) should treat
    /// this as "fall back to prompt" rather than auto-trust.
    Unsupported { detail: String },
    /// An OS-level call failed during policy installation. The inner
    /// error captures the OS-specific failure (e.g., seccomp install
    /// failure, sandbox profile compile error).
    Os {
        message: String,
        source: Option<Box<dyn Error + Send + Sync + 'static>>,
    },
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { detail } => {
                write!(
                    f,
                    "sandbox capability not supported on this platform: {detail}"
                )
            }
            Self::Os { message, .. } => write!(f, "sandbox apply failed: {message}"),
        }
    }
}

impl Error for SandboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unsupported { .. } => None,
            Self::Os { source, .. } => source.as_ref().map(|s| s.as_ref() as &dyn Error),
        }
    }
}

impl SandboxError {
    /// Convenience constructor for an [`SandboxError::Unsupported`]
    /// error.
    pub fn unsupported(detail: impl Into<String>) -> Self {
        Self::Unsupported {
            detail: detail.into(),
        }
    }

    /// Convenience constructor for an [`SandboxError::Os`] error
    /// without a source.
    pub fn os(message: impl Into<String>) -> Self {
        Self::Os {
            message: message.into(),
            source: None,
        }
    }
}
