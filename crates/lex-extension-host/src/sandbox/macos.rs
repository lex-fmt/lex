//! macOS sandbox enforcement for declared-pure handlers.
//!
//! macOS exposes its sandbox via the `sandbox_init(profile, flags,
//! errbuf)` libSystem function, which takes a Scheme-flavoured
//! "Sandbox Profile Language" (SBPL) string and applies it to the
//! current process. The same `pre_exec` pattern as the Linux impl
//! applies the profile to the child after `fork()` and before
//! `execve()`; macOS's sandbox policy survives `execve()`.
//!
//! ## Deprecation note
//!
//! `sandbox_init` has been deprecated since macOS 10.8 (2012), but
//! the function still ships in libSystem on every shipping macOS
//! version through Sequoia (15.x). Apple's replacement APIs
//! (`sandbox_extension_*`, the App Sandbox entitlement model) are
//! either SPI or require code signing + entitlements, neither of
//! which fits this use case. We rely on the deprecated function
//! exactly as `/usr/bin/sandbox-exec` does — Apple's own utility,
//! itself deprecated, remains the canonical "run an arbitrary binary
//! under a profile" tool on macOS.
//!
//! ## Profile shape
//!
//! The v1 profile is intentionally narrow: it denies network access
//! and reads of `/etc` (the workspace whose `/etc/passwd` the
//! `fs_probe` fixture targets), keeping everything else permissive
//! so a Rust binary can still load its system libraries and call
//! Mach services. A deny-default profile that runs Rust binaries
//! requires per-macOS-version tuning of dyld / mach-lookup
//! allowlists; that hardening is a follow-up once the schema's
//! `Capabilities` grows finer fields than `is_pure()`.
//!
//! ## Why `supports()` returns `false`
//!
//! The `(allow default)` shape above leaves a meaningful gap:
//! handlers running under it can still **write** anywhere on disk
//! and **read** anywhere outside `/etc` (notably `~/.ssh`, the
//! workspace, user documents). The probe tests catch the two
//! specific operations they target, but those denies are not the
//! complete protection a caller expects from a `pure` capability
//! declaration.
//!
//! To avoid silently auto-trusting handlers that aren't actually
//! isolated, [`MacosSandbox::supports`] **always returns `false`**.
//! `apply_to` still installs the profile (the limited denies remain
//! useful as a baseline and as regression tests for the SBPL
//! wiring), but the trust gate routes pure handlers to the prompt
//! path on macOS — same path Windows and no-landlock Linux use.
//!
//! When a hardened `(deny default)` profile lands, flipping
//! `supports` back to `caps.is_pure()` is a one-line change.
//!
//! The module itself is `#[cfg(target_os = "macos")]`-gated in
//! `super::mod`; no inner cfg is required here.

use std::ffi::{CStr, CString};
use std::io;
use std::os::unix::process::CommandExt;

use lex_extension::schema::Capabilities;

use super::{Sandbox, SandboxError};

/// macOS sandbox built on the libSystem `sandbox_init` API. Installed
/// via a `pre_exec` hook on the child so the policy applies after
/// `fork()` and survives `execve()`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacosSandbox;

/// SBPL profile for a pure handler: deny network + deny reads of
/// `/etc`. Everything else stays permissive so the system loader can
/// still bring up the Rust binary.
const PURE_PROFILE: &str = "(version 1)\n\
                            (allow default)\n\
                            (deny network*)\n\
                            (deny file-read* (subpath \"/etc\"))\n";

impl Sandbox for MacosSandbox {
    fn apply_to(
        &self,
        cmd: &mut std::process::Command,
        caps: Capabilities,
    ) -> Result<(), SandboxError> {
        if !caps.is_pure() {
            return Err(SandboxError::new(format!(
                "MacosSandbox only enforces pure capabilities (fs=false, net=false); got {caps:?}"
            )));
        }
        // SAFETY: the pre_exec closure only calls sandbox_init (a
        // libSystem syscall wrapper) plus a write(2) for the
        // diagnostic path. After fork() the child is single-threaded
        // so any allocator state held by other parent threads cannot
        // deadlock this path.
        unsafe {
            cmd.pre_exec(install_pure_policy);
        }
        Ok(())
    }

    /// **Intentionally returns `false` for every capability shape**
    /// until the SBPL profile is hardened to `(deny default)`. See
    /// the module docs for the rationale: the current `(allow
    /// default)` profile, while it denies the specific cases the
    /// probe tests cover (`/etc/passwd` reads, IP network), leaves
    /// every other filesystem operation permitted — including
    /// writes anywhere on disk and reads of `~/.ssh`, the
    /// workspace, etc. Reporting `supports = true` here would let
    /// the trust gate auto-trust a handler that's not actually
    /// isolated, which is a silent privilege escalation once PR 12d
    /// flips the matrix.
    ///
    /// Falling back to `false` puts macOS on the same path as
    /// Windows and any Linux kernel without landlock: the trust
    /// gate prompts the user (or requires `--enable-handlers` in
    /// CI). That is the correct behaviour given the current
    /// profile's coverage.
    fn supports(&self, _caps: Capabilities) -> bool {
        false
    }
}

// libSystem.dylib provides these. The functions are deprecated but
// still resolvable on every macOS version we target.
extern "C" {
    fn sandbox_init(
        profile: *const libc::c_char,
        flags: u64,
        errorbuf: *mut *mut libc::c_char,
    ) -> libc::c_int;

    fn sandbox_free_error(errorbuf: *mut libc::c_char);
}

fn install_pure_policy() -> io::Result<()> {
    let profile = CString::new(PURE_PROFILE).expect("profile has no interior nul");
    let mut errbuf: *mut libc::c_char = std::ptr::null_mut();
    // SAFETY: `profile.as_ptr()` is a valid nul-terminated C string
    // for the lifetime of this call; `&mut errbuf` is a writable
    // out-parameter. sandbox_init's contract is: returns 0 on
    // success; on failure returns non-zero and fills errbuf with a
    // malloc'd diagnostic that must be freed via sandbox_free_error.
    let ret = unsafe { sandbox_init(profile.as_ptr(), 0, &mut errbuf) };
    if ret == 0 {
        return Ok(());
    }
    let detail = if errbuf.is_null() {
        "sandbox_init failed without a diagnostic message".to_owned()
    } else {
        // SAFETY: errbuf points to a malloc'd nul-terminated string
        // owned by libSystem; we copy it into a Rust String, then
        // hand the pointer back to libSystem for freeing.
        let s = unsafe { CStr::from_ptr(errbuf) }
            .to_string_lossy()
            .into_owned();
        unsafe { sandbox_free_error(errbuf) };
        s
    };
    let err = io::Error::other(format!("sandbox_init: {detail}"));
    write_diag("sandbox_init", &err);
    Err(err)
}

/// Async-signal-safe diagnostic write to fd 2. Mirrors the Linux
/// impl's `write_diag` so operators get the same shape of
/// pre_exec-failure context on either OS.
fn write_diag(stage: &str, err: &io::Error) {
    let msg = format!("lex-extension-host sandbox: {stage} failed: {err}\n");
    let bytes = msg.as_bytes();
    // SAFETY: write(2) is async-signal-safe; fd 2 is inherited from
    // the parent and open until execve().
    unsafe {
        libc::write(2, bytes.as_ptr() as *const _, bytes.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_returns_false_for_every_capability_shape() {
        // Pinned to false until the SBPL profile is hardened to
        // `(deny default)`. See the module docs for the why; this
        // test guards against an inadvertent re-enable that would
        // ship a silent privilege escalation.
        let s = MacosSandbox;
        assert!(!s.supports(Capabilities::default()));
        assert!(!s.supports(Capabilities {
            fs: true,
            net: false,
        }));
        assert!(!s.supports(Capabilities {
            fs: false,
            net: true,
        }));
        assert!(!s.supports(Capabilities {
            fs: true,
            net: true,
        }));
    }

    #[test]
    fn apply_to_rejects_non_pure_capabilities() {
        let s = MacosSandbox;
        let mut cmd = std::process::Command::new("/usr/bin/true");
        let err = s
            .apply_to(
                &mut cmd,
                Capabilities {
                    fs: true,
                    net: false,
                },
            )
            .expect_err("non-pure caps must be rejected before spawn");
        assert!(
            err.to_string().contains("pure capabilities"),
            "unexpected error message: {err}"
        );
    }
}
