//! Verifies [`GitFetcher`] surfaces a clean error when `git` is not
//! on `PATH`. The test mutates the process's `PATH` env var, so it
//! lives in its own integration-test file (`tests/*.rs` files compile
//! to separate test binaries — within one binary tests run in
//! parallel by default, and a global env mutation would race with any
//! other test that shells out to a binary).
//!
//! Spec §6.3 requires the diagnostic surface to be clear about the
//! constraint (`git` must be in `PATH`); the test pins the message
//! shape so future refactors don't quietly drop the hint about
//! falling back to `path:` / `--ext-schema`.

use lex_extension_host::resolve::fetcher::GitFetcher;
use lex_extension_host::{FetchError, Fetcher, ParsedUri};

#[test]
fn git_not_in_path_surfaces_actionable_other_error() {
    // SAFETY: mutating PATH affects this single-test binary only.
    // No parallelism concern: this file has one test.
    let original = std::env::var_os("PATH");
    // `safety: set_var is unsafe in newer Rust editions for
    // multithreaded contexts. We're single-test, single-thread.`
    // Note: tests do run on multiple threads via the test harness's
    // thread pool, but only this one test binary; no other test in
    // this binary touches PATH or Command::new("git"). Even so, we
    // could wrap in a Mutex if the harness configuration changes.
    std::env::set_var("PATH", "");

    let dest = tempfile::tempdir().unwrap();
    let uri = ParsedUri::parse("git:file:///nonexistent/path").unwrap();
    let result = GitFetcher.fetch(&uri, dest.path());

    // Restore PATH before any assertion (so a panic doesn't strand
    // subsequent tests in a stripped-PATH state). The variable was
    // captured before mutation; if it was unset, leave it unset.
    if let Some(orig) = original {
        std::env::set_var("PATH", orig);
    } else {
        std::env::remove_var("PATH");
    }

    let err = result.expect_err("git fetch should fail when git is not in PATH");
    match err {
        FetchError::Other { message } => {
            assert!(
                message.contains("git binary not in PATH"),
                "error should name the missing binary: {message}"
            );
            assert!(
                message.contains("path:") || message.contains("--ext-schema"),
                "error should hint at the local-schema escape hatch: {message}"
            );
        }
        other => panic!("expected Other(git binary not in PATH), got: {other:?}"),
    }
}
