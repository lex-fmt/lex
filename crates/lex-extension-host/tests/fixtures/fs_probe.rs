//! Filesystem-probe test fixture for the sandbox suite.
//!
//! Tries to read `/etc/passwd`. Exits 0 on success, 42 on EPERM /
//! EACCES / not-found. Used by sandbox tests to assert that a
//! `pure`-capability handler running under an enforced sandbox cannot
//! reach the filesystem, with `NullSandbox` providing the negative
//! control (exit 0 means the fixture itself works).
//!
//! /etc/passwd is chosen because it exists with mode 0644 on every
//! Linux distribution and macOS install we target, so the negative
//! control is reliable across CI runners.

use std::fs::File;

const EXIT_BLOCKED: i32 = 42;

fn main() {
    match File::open("/etc/passwd") {
        Ok(_) => std::process::exit(0),
        Err(_) => std::process::exit(EXIT_BLOCKED),
    }
}
