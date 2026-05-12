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
//! 2. The host passes the impl into
//!    [`SubprocessHandler::spawn_with_sandbox`] (or installs it on
//!    a [`crate::TrustGate`] via [`crate::TrustGate::set_sandbox`]
//!    for the auto-trust decision); the worker thread moves it in
//!    by value.
//! 3. Before spawning the child process, the worker calls
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
//! [`SubprocessHandler::spawn_with_sandbox`]: crate::transport::SubprocessHandler::spawn_with_sandbox

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use lex_extension::schema::Capabilities;

mod null;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
mod macos;

pub use null::NullSandbox;

#[cfg(target_os = "linux")]
pub use linux::LinuxSandbox;

#[cfg(target_os = "macos")]
pub use macos::MacosSandbox;

/// Construct the OS-appropriate default [`Sandbox`] for the running
/// platform. The δ-phase ([lex#528]) trust-matrix flip uses this to
/// switch the engine's default from [`NullSandbox`] (post-plumbing
/// stand-in, never enforces) to the per-OS impl on supported
/// platforms.
///
/// Asymmetry by design:
///
/// - **Linux**: returns [`LinuxSandbox`]. `supports(pure)` returns
///   `true`, so the trust gate auto-trusts declared-pure handlers
///   under this default.
/// - **macOS**: returns [`MacosSandbox`]. `supports()` returns
///   `false` for every capability shape until a hardened
///   `(deny default)` SBPL profile lands, so the trust gate
///   continues to route pure handlers to the prompt path —
///   identical UX to Windows. `apply_to` still installs the limited
///   profile so handlers that do run after a user prompt still get
///   the partial denies.
/// - **Other (Windows etc.)**: returns [`NullSandbox`]. No
///   enforcement, no auto-trust — the trust gate prompts on every
///   subprocess handler, matching β/γ behaviour for now.
///
/// Returned as `Arc<dyn Sandbox>` so the host can install one
/// instance and share it with both [`crate::TrustGate::set_sandbox`]
/// and [`crate::transport::SubprocessHandler::spawn_with_sandbox`].
/// That sharing is load-bearing: the auto-trust decision must be
/// anchored on the *same* sandbox that actually enforces policy at
/// spawn time.
///
/// [lex#528]: https://github.com/lex-fmt/lex/issues/528
pub fn os_default() -> Arc<dyn Sandbox> {
    #[cfg(target_os = "linux")]
    {
        Arc::new(LinuxSandbox)
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacosSandbox)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Arc::new(NullSandbox)
    }
}

/// OS-level sandbox enforcement for subprocess handlers.
///
/// Implementations modify a `std::process::Command` so that the child
/// process can only perform the operations the [`Capabilities`]
/// argument permits. See the [module docs](self) for the lifecycle
/// and platform-specific notes.
pub trait Sandbox: Send + Sync {
    /// Apply the sandbox policy to the command. Called by the
    /// subprocess transport just before `spawn()`. Returns
    /// [`SandboxError`] if a kernel-level call fails during policy
    /// installation.
    ///
    /// ## Contract
    ///
    /// - **Do not mutate stdin / stdout / stderr.** The subprocess
    ///   transport configures those as piped streams owned by tokio's
    ///   reactor for the JSON-RPC bridge; replacing the descriptors
    ///   breaks the transport. Implementations may attach Unix
    ///   pre-exec hooks, set environment variables, modify Windows
    ///   restricted-token attributes, etc. — anything that doesn't
    ///   touch the standard I/O streams.
    /// - **Don't return an error for "this capability isn't
    ///   supported on this OS".** Communicate that via
    ///   [`Sandbox::supports`] so the trust gate can route the
    ///   handler to the prompt path *before* spawn. By the time
    ///   `apply_to` is called, the trust gate has already returned
    ///   its final decision; surfacing an unsupported-capability
    ///   error here is a hard spawn failure, not a fall-back.
    /// - **Only return [`SandboxError`] for genuine kernel
    ///   failures** (seccomp install fault, sandbox profile compile
    ///   error, etc.).
    fn apply_to(
        &self,
        cmd: &mut std::process::Command,
        caps: Capabilities,
    ) -> Result<(), SandboxError>;

    /// True when this impl can enforce the supplied capability set
    /// on the running platform. The trust gate consults this when
    /// deciding whether a handler is eligible for auto-trust: only
    /// `true` shifts a `Pending` decision to `Trusted` without a
    /// prompt.
    ///
    /// Takes the [`Capabilities`] rather than the gate's coarser
    /// `Capability` (Pure / Full) classifier so the trait stays
    /// forward-compatible: future granular variants (e.g.
    /// `fs_read: true, fs_write: false`) can ask the sandbox
    /// directly whether the running OS implementation covers them.
    ///
    /// [`NullSandbox::supports`] always returns `false`. Per-OS
    /// impls return `false` for capability sets they can't enforce
    /// even if they support others.
    fn supports(&self, caps: Capabilities) -> bool;
}

/// Errors a [`Sandbox`] implementation can surface when installing a
/// policy at spawn time. Reserved for genuine kernel failures —
/// platform-capability mismatches are communicated through
/// [`Sandbox::supports`] *before* the trust gate decides, not here.
#[derive(Debug)]
pub struct SandboxError {
    message: String,
    source: Option<Box<dyn Error + Send + Sync + 'static>>,
}

impl SandboxError {
    /// Construct a new sandbox error without a wrapped source. Use
    /// when the OS-specific failure is already captured in
    /// `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }

    /// Construct from a message and a wrapped source error (for
    /// preserving the original `std::io::Error` from a kernel call,
    /// etc.).
    pub fn with_source(
        message: impl Into<String>,
        source: Box<dyn Error + Send + Sync + 'static>,
    ) -> Self {
        Self {
            message: message.into(),
            source: Some(source),
        }
    }
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sandbox apply failed: {}", self.message)
    }
}

impl Error for SandboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| s.as_ref() as &dyn Error)
    }
}
