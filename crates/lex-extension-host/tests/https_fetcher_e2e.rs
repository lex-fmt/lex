//! End-to-end network test for [`HttpsFetcher`] against a real
//! public tarball.
//!
//! Gated behind `#[ignore]` so CI doesn't depend on upstream
//! availability. Run with `cargo test --test https_fetcher_e2e --
//! --ignored` (or `cargo nextest run --test https_fetcher_e2e
//! --run-ignored=only`).
//!
//! The target is GitHub's `octocat/Hello-World` tarball — an
//! officially-stable test repo used in GitHub's own documentation.
//! Small, public, no auth, predictable filenames after extraction.

use std::path::Path;

use lex_extension_host::{default_fetcher_registry, resolve_namespace_with, ResolverCache};

#[test]
#[ignore = "network: hits api.github.com — run with --ignored when you want to exercise the real https path"]
fn fetches_real_github_tarball_via_template_dispatch() {
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    let cache_dir = tempfile::tempdir().expect("cache tempdir");
    let cache = ResolverCache::new(cache_dir.path()).expect("init cache");
    let registry = default_fetcher_registry();

    let resolved = resolve_namespace_with(
        "github:octocat/Hello-World",
        workspace.path(),
        &registry,
        &cache,
    )
    .expect("github tarball should resolve via the template + https fetcher");

    // The github tarball ships a single wrapper directory
    // (`octocat-Hello-World-<sha>/`) at the archive root containing
    // the repo contents. Without a `subdir = "..."` configured, our
    // extractor preserves that structure as-is — so the wrapper
    // directory should exist under the resolved schema_dir, and a
    // README inside it.
    let entries: Vec<_> = std::fs::read_dir(&resolved.schema_dir)
        .expect("schema dir readable")
        .filter_map(|e| e.ok())
        .collect();
    assert!(
        !entries.is_empty(),
        "extracted github tarball should have at least one top-level entry"
    );
    let wrapper = entries
        .iter()
        .find(|e| e.path().is_dir())
        .expect("github tarball wraps content in a single dir at root");
    assert!(
        wrapper
            .file_name()
            .to_string_lossy()
            .starts_with("octocat-Hello-World-"),
        "expected wrapper dir prefix `octocat-Hello-World-`, got: {:?}",
        wrapper.file_name()
    );
    assert!(
        find_file(&wrapper.path(), "README").is_some(),
        "Hello-World repo has a README at its root"
    );
}

fn find_file(root: &Path, name: &str) -> Option<std::path::PathBuf> {
    let lower = name.to_ascii_lowercase();
    for entry in std::fs::read_dir(root).ok()? {
        let entry = entry.ok()?;
        if entry
            .file_name()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with(&lower)
        {
            return Some(entry.path());
        }
    }
    None
}
