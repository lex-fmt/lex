//! Linux sandbox enforcement for declared-pure handlers.
//!
//! Combines two kernel mechanisms:
//!
//! - **seccomp** (via the `seccompiler` crate, Apache-2.0 / BSD-3) —
//!   blocks network-stack syscalls (`socket`, `connect`, `bind`,
//!   `listen`, `accept`, `sendto`, etc.). Returns `EPERM` so the
//!   child errors gracefully rather than dying via `SIGSYS`.
//! - **landlock** (via the `landlock` crate, MIT / Apache-2.0) —
//!   restricts filesystem reads to a minimal allowlist: the
//!   dynamic-loader paths plus the handler binary itself. Anything
//!   not in the allowlist (notably `/etc/passwd` and the workspace
//!   the host is running over) returns `EACCES`.
//!
//! Both policies are installed inside a `pre_exec` hook so they apply
//! to the child after `fork()` and survive across `execve()`. The
//! parent host process is unaffected.
//!
//! ## Capability shapes
//!
//! [`LinuxSandbox::supports`] returns `true` only for the
//! [`Capabilities::is_pure`] shape (`fs: false, net: false`). Any
//! finer shape (when [`Capabilities`] grows fields like scoped fs
//! reads) reports unsupported until we wire the corresponding
//! allowlist; the trust gate then routes those handlers to the
//! prompt path and `apply_to` is never called for them.
//!
//! ## Fail-closed semantics
//!
//! If the kernel reports that landlock is not enforced (older kernel
//! without the LSM, or build without `CONFIG_SECURITY_LANDLOCK`), we
//! treat it as a hard error from the `pre_exec` hook so the spawn
//! fails. A handler advertised as sandbox-isolated must actually be
//! isolated — silent fallback would defeat the trust gate.
//!
//! The module itself is `#[cfg(target_os = "linux")]`-gated in
//! `super::mod`; no inner cfg is required here.

use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

use landlock::{
    AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, ABI,
};
use lex_extension::schema::Capabilities;
use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, TargetArch};

use super::{Sandbox, SandboxError};

/// Linux sandbox built on seccomp (network deny) + landlock
/// (filesystem allowlist). Installed via a `pre_exec` hook on the
/// child process so the policy survives `execve()`.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxSandbox;

impl Sandbox for LinuxSandbox {
    fn apply_to(
        &self,
        cmd: &mut std::process::Command,
        caps: Capabilities,
    ) -> Result<(), SandboxError> {
        if !caps.is_pure() {
            return Err(SandboxError::new(format!(
                "LinuxSandbox only enforces pure capabilities (fs=false, net=false); got {caps:?}"
            )));
        }
        let program: OsString = cmd.get_program().to_owned();
        // SAFETY: the closure only performs syscalls (seccomp filter
        // install, landlock ruleset create + restrict_self) plus
        // allocations needed by the seccompiler/landlock crates.
        // After fork() the child is single-threaded so allocator
        // mutexes left by other parent threads cannot deadlock this
        // path. No locks owned by the parent are touched.
        unsafe {
            cmd.pre_exec(move || install_pure_policy(&program));
        }
        Ok(())
    }

    fn supports(&self, caps: Capabilities) -> bool {
        caps.is_pure()
    }
}

/// Apply the seccomp + landlock policies for a pure-capability
/// handler in the just-forked child, right before `execve()`. Errors
/// here cause `Command::spawn` to fail; the parent host surfaces them
/// through the existing `SpawnError::Sandbox` variant.
///
/// `pre_exec` swallows the textual `io::Error` message on the way
/// back to the parent (only the errno is preserved), so we route
/// diagnostic context through a stderr write before returning. The
/// child still has the parent's stderr at this point.
fn install_pure_policy(program: &std::ffi::OsStr) -> io::Result<()> {
    set_no_new_privs().inspect_err(|e| write_diag("PR_SET_NO_NEW_PRIVS", e))?;
    install_landlock_fs_allowlist(program).inspect_err(|e| write_diag("landlock", e))?;
    install_seccomp_network_deny().inspect_err(|e| write_diag("seccomp", e))?;
    Ok(())
}

/// Async-signal-safe diagnostic write to stderr from inside
/// `pre_exec`. Used so the operator gets a specific failure point
/// instead of a bare `EINVAL` when the kernel lacks landlock support
/// or the policy install fails at runtime. `eprintln!` is unsafe
/// post-fork because it acquires the stdio lock; `libc::write` is
/// not.
fn write_diag(stage: &str, err: &io::Error) {
    let msg = format!("lex-extension-host sandbox: {stage} failed: {err}\n");
    let bytes = msg.as_bytes();
    // SAFETY: write(2) is async-signal-safe; fd 2 is inherited from
    // the parent and remains open until execve().
    unsafe {
        libc::write(2, bytes.as_ptr() as *const _, bytes.len());
    }
}

/// Unprivileged seccomp filter installation requires
/// `PR_SET_NO_NEW_PRIVS` to be set on the process — without it the
/// kernel rejects `PR_SET_SECCOMP` with `EPERM`. Landlock also
/// requires this for unprivileged rulesets. Setting it before either
/// policy install is the canonical pattern.
fn set_no_new_privs() -> io::Result<()> {
    // SAFETY: prctl is a syscall wrapper; no Rust invariants in scope.
    let ret = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Filesystem allowlist: read access to the dynamic-loader paths +
/// the handler binary. Everything else is denied (notably the
/// workspace and `/etc/passwd`).
///
/// The path set is intentionally broad enough to cover Debian/Ubuntu,
/// RHEL/Fedora, Arch, and Alpine layouts. `PathFd::new` silently
/// drops paths that don't exist on the current distro so the
/// allowlist is robust to per-distro differences.
fn install_landlock_fs_allowlist(program: &std::ffi::OsStr) -> io::Result<()> {
    let abi = ABI::V1;
    let read_access = AccessFs::from_read(abi);

    // System paths a typical handler binary needs to read in order to
    // boot:
    //
    // - `/lib`, `/lib64`, `/usr/lib`, `/usr/lib64` — dynamic-loader
    //   library paths.
    // - `/etc/ld.so.cache`, `/etc/ld.so.preload` — dynamic-loader
    //   config.
    // - `/proc/self` — Rust stdlib reads `/proc/self/exe` and a few
    //   neighbours during startup.
    // - `/usr/bin`, `/bin` — common interpreters (`node`,
    //   `python3`, `bash`, `env`) and basic system utilities. Without
    //   these, schemas using `command: ["node", "handler.js"]` or a
    //   shebang script fail to `execve` even when the script itself
    //   is allowed. The script path adjacent to an interpreter is a
    //   separate concern tracked at lex#559.
    let mut allowed: Vec<PathBuf> = [
        "/lib",
        "/lib64",
        "/usr/lib",
        "/usr/lib64",
        "/usr/bin",
        "/bin",
        "/etc/ld.so.cache",
        "/etc/ld.so.preload",
        "/proc/self",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    allowed.push(PathBuf::from(program));

    let ruleset = Ruleset::default()
        .handle_access(read_access)
        .map_err(landlock_err)?
        .create()
        .map_err(landlock_err)?
        .add_rules(allowed.iter().filter_map(|p| {
            PathFd::new(p)
                .ok()
                .map(|fd| Ok::<_, landlock::RulesetError>(PathBeneath::new(fd, read_access)))
        }))
        .map_err(landlock_err)?;

    let status = ruleset.restrict_self().map_err(landlock_err)?;
    // FullyEnforced — all requested AccessFs bits applied. Common on
    // modern kernels.
    // PartiallyEnforced — landlock is active and our rules are
    // installed; some bits in the ABI we requested are silently
    // unsupported by this kernel. The bits we care about for the
    // pure-handler shape (file read on /etc/passwd) are covered by
    // the supported subset, so this is acceptable.
    // NotEnforced — landlock isn't applying our rules at all (kernel
    // built without `CONFIG_SECURITY_LANDLOCK`, or LSM not loaded).
    // Fail closed.
    if status.ruleset == RulesetStatus::NotEnforced {
        return Err(io::Error::other(format!(
            "landlock not enforced (status: {:?}); kernel missing landlock support",
            status.ruleset
        )));
    }
    Ok(())
}

/// Network deny: block the syscalls that open or accept IP sockets.
/// Everything else passes through. Unmatched syscalls get the
/// default `Allow` action — the seccomp filter is additive to
/// landlock's fs enforcement, not a full whitelist.
fn install_seccomp_network_deny() -> io::Result<()> {
    use std::collections::BTreeMap;

    // Deny EPERM rather than KILL — we want graceful Err returns in
    // the child, not SIGSYS crash messages, so the negative-control
    // tests can distinguish "blocked" from "process died unexpectedly".
    let deny = SeccompAction::Errno(libc::EPERM as u32);

    // `SYS_socket` is the primary gate — a process that can't create
    // an IP socket can't do networking regardless of which other
    // syscalls are reachable. The rest are defense-in-depth for the
    // case where a socket fd is inherited from the parent (not part
    // of our spawn path today, but cheap to defend against).
    // `sendmmsg`/`recvmmsg` are the multi-message variants of
    // `sendmsg`/`recvmsg` and need to be listed explicitly — seccomp
    // filters on syscall number, not on name.
    let denied_syscalls = [
        libc::SYS_socket,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_sendto,
        libc::SYS_recvfrom,
        libc::SYS_sendmsg,
        libc::SYS_recvmsg,
        libc::SYS_sendmmsg,
        libc::SYS_recvmmsg,
    ];

    let mut rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = BTreeMap::new();
    for syscall in denied_syscalls {
        rules.insert(syscall, Vec::new());
    }

    let arch: TargetArch = std::env::consts::ARCH
        .try_into()
        .map_err(|e| io::Error::other(format!("seccomp arch: {e}")))?;

    let filter = SeccompFilter::new(rules, SeccompAction::Allow, deny, arch)
        .map_err(|e| io::Error::other(format!("seccomp filter build: {e}")))?;

    let program: BpfProgram = filter
        .try_into()
        .map_err(|e| io::Error::other(format!("seccomp compile: {e}")))?;

    seccompiler::apply_filter(&program)
        .map_err(|e| io::Error::other(format!("seccomp apply: {e}")))?;

    Ok(())
}

fn landlock_err<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::other(format!("landlock: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_returns_true_only_for_pure_capabilities() {
        let s = LinuxSandbox;
        assert!(s.supports(Capabilities::default()));
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
        let s = LinuxSandbox;
        let mut cmd = std::process::Command::new("/bin/true");
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
