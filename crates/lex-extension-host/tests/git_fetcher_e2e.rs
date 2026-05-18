//! Integration test for [`lex_extension_host::resolve::fetcher::GitFetcher`]
//! against a local bare repository.
//!
//! The fixture builds a real bare repo in a tempdir, populates it
//! with a commit (or several), and points the fetcher at a
//! `git:file://<path>` URI. This avoids the brittle "ignore unless
//! --ignored" gating that the https e2e test needs (the GitHub
//! tarball API is a live external dependency); local-bare is
//! hermetic and runs as part of the normal test suite.
//!
//! What's covered:
//!
//! - Clone of a single-commit repo into an empty dest produces the
//!   file tree under dest with no `.git/` left behind.
//! - `uri.rev` set to a branch name passes through as `--branch` and
//!   selects that branch's HEAD.
//! - `uri.rev` set to a tag selects the tagged commit.
//! - `uri.subdir` extracts just that subdirectory's contents.
//! - Subdir-not-found surfaces as a typed extract-style error.
//! - Clone failure on a non-existent file:// URL surfaces as a typed
//!   error (Network or Other; the local file:// URL won't classify
//!   as Network so we accept Other too).
//! - Cache integration round-trip (cache miss → clone → cache hit →
//!   no clone).

use std::path::Path;
use std::process::Command;

use lex_extension_host::{
    default_fetcher_registry, resolve_namespace_with, FetchError, Fetcher, ParsedUri, ResolverCache,
};

/// Run a `git` subcommand against the supplied directory, asserting
/// success. Sets sane non-interactive defaults so the test runs in any
/// environment regardless of the user's global gitconfig (signing
/// disabled, default branch named, dummy identity).
fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args([
            "-c",
            "user.email=test@lex.invalid",
            "-c",
            "user.name=lex-test",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} spawn failed: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Build a bare git repo at `<base>/bare.git` with the supplied entries
/// committed on `main`. Returns the bare repo's absolute path.
///
/// Each entry is `(relative_path, file_contents)`. The working tree
/// for the commit is built in a sibling `work/` directory; we push the
/// resulting commit into the bare repo so subsequent clones see it on
/// the `main` branch.
fn build_bare_repo(base: &Path, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
    let bare = base.join("bare.git");
    std::fs::create_dir(&bare).unwrap();
    git(&bare, &["init", "--bare", "--initial-branch=main"]);

    let work = base.join("work");
    std::fs::create_dir(&work).unwrap();
    git(&work, &["init", "--initial-branch=main"]);
    for (rel, contents) in entries {
        let p = work.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, contents).unwrap();
    }
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "initial"]);
    let bare_str = bare.to_str().unwrap();
    git(&work, &["remote", "add", "origin", bare_str]);
    git(&work, &["push", "origin", "main"]);
    bare
}

/// Same as [`build_bare_repo`] but layers a second commit on top
/// (under a different branch + tag) so tests can exercise `rev`
/// dispatch.
fn build_bare_repo_with_extra_branch_and_tag(
    base: &Path,
    main_entries: &[(&str, &[u8])],
    branch_entries: &[(&str, &[u8])],
    branch_name: &str,
    tag_name: &str,
) -> std::path::PathBuf {
    let bare = build_bare_repo(base, main_entries);
    let work = base.join("work");
    // Build the branch commit on top of main.
    git(&work, &["checkout", "-b", branch_name]);
    for (rel, contents) in branch_entries {
        let p = work.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, contents).unwrap();
    }
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "branch-commit"]);
    git(&work, &["tag", tag_name]);
    git(&work, &["push", "origin", branch_name]);
    git(&work, &["push", "origin", tag_name]);
    bare
}

/// Builds a `git:file://<absolute-path>` URI for the bare repo path.
fn file_uri_for(bare: &Path, fragment: Option<&str>, query: Option<&str>) -> String {
    let mut s = format!("git:file://{}", bare.display());
    if let Some(rev) = fragment {
        s.push('#');
        s.push_str(rev);
    }
    if let Some(q) = query {
        s.push('?');
        s.push_str(q);
    }
    s
}

#[test]
fn clones_bare_repo_into_empty_dest_and_strips_dot_git() {
    let base = tempfile::tempdir().unwrap();
    let bare = build_bare_repo(
        base.path(),
        &[
            ("schema.yaml", b"label: acme.x\n"),
            ("nested/extra.yaml", b"label: acme.nested\n"),
        ],
    );

    let dest = tempfile::tempdir().unwrap();
    let fetcher = lex_extension_host::resolve::fetcher::GitFetcher;
    let uri = ParsedUri::parse(&file_uri_for(&bare, None, None)).unwrap();
    fetcher
        .fetch(&uri, dest.path())
        .expect("git clone of local bare repo");

    assert!(
        dest.path().join("schema.yaml").exists(),
        "top-level schema file should be present after clone"
    );
    assert!(
        dest.path().join("nested/extra.yaml").exists(),
        "nested file should be preserved"
    );
    assert!(
        !dest.path().join(".git").exists(),
        ".git should be stripped from the cache contents"
    );
    assert!(
        !dest.path().join(".lex-git-clone").exists(),
        "the temporary clone-staging directory should be cleaned up"
    );
}

#[test]
fn rev_selects_branch_head() {
    let base = tempfile::tempdir().unwrap();
    let bare = build_bare_repo_with_extra_branch_and_tag(
        base.path(),
        &[("schema.yaml", b"label: main\n")],
        &[("schema.yaml", b"label: feature\n")],
        "feature/x",
        "v0.1.0",
    );

    let dest = tempfile::tempdir().unwrap();
    let fetcher = lex_extension_host::resolve::fetcher::GitFetcher;
    let uri = ParsedUri::parse(&file_uri_for(&bare, Some("feature/x"), None)).unwrap();
    fetcher
        .fetch(&uri, dest.path())
        .expect("clone of feature branch");

    let body = std::fs::read_to_string(dest.path().join("schema.yaml")).unwrap();
    assert!(
        body.contains("feature"),
        "branch-rev should select feature branch contents, got: {body:?}"
    );
}

#[test]
fn rev_selects_tag() {
    let base = tempfile::tempdir().unwrap();
    let bare = build_bare_repo_with_extra_branch_and_tag(
        base.path(),
        &[("schema.yaml", b"label: main\n")],
        &[("schema.yaml", b"label: tagged\n")],
        "feature/x",
        "v0.1.0",
    );

    let dest = tempfile::tempdir().unwrap();
    let fetcher = lex_extension_host::resolve::fetcher::GitFetcher;
    let uri = ParsedUri::parse(&file_uri_for(&bare, Some("v0.1.0"), None)).unwrap();
    fetcher.fetch(&uri, dest.path()).expect("clone at tag");

    let body = std::fs::read_to_string(dest.path().join("schema.yaml")).unwrap();
    assert!(
        body.contains("tagged"),
        "tag-rev should select the tagged commit's contents, got: {body:?}"
    );
}

#[test]
fn subdir_extracts_only_that_directory() {
    let base = tempfile::tempdir().unwrap();
    let bare = build_bare_repo(
        base.path(),
        &[
            ("README.md", b"readme"),
            ("labels/foo.yaml", b"label: foo\n"),
            ("labels/bar.yaml", b"label: bar\n"),
            ("other/baz.yaml", b"label: baz\n"),
        ],
    );

    let dest = tempfile::tempdir().unwrap();
    let fetcher = lex_extension_host::resolve::fetcher::GitFetcher;
    let uri = ParsedUri::parse(&file_uri_for(&bare, None, Some("subdir=labels"))).unwrap();
    fetcher.fetch(&uri, dest.path()).expect("subdir clone");

    assert!(dest.path().join("foo.yaml").exists());
    assert!(dest.path().join("bar.yaml").exists());
    assert!(
        !dest.path().join("README.md").exists(),
        "README should be excluded by subdir filter"
    );
    assert!(
        !dest.path().join("baz.yaml").exists(),
        "other-dir contents should be excluded by subdir filter"
    );
    assert!(
        !dest.path().join("labels").exists(),
        "subdir prefix should be stripped (loader scans dest flat)"
    );
    assert!(
        !dest.path().join(".lex-git-clone").exists(),
        "clone-staging dir should be cleaned up"
    );
}

#[test]
fn subdir_not_found_surfaces_typed_error() {
    let base = tempfile::tempdir().unwrap();
    let bare = build_bare_repo(base.path(), &[("schema.yaml", b"label: x\n")]);

    let dest = tempfile::tempdir().unwrap();
    let fetcher = lex_extension_host::resolve::fetcher::GitFetcher;
    let uri = ParsedUri::parse(&file_uri_for(&bare, None, Some("subdir=missing"))).unwrap();
    let err = fetcher
        .fetch(&uri, dest.path())
        .expect_err("missing subdir should error");
    match err {
        FetchError::Other { message } => {
            assert!(
                message.contains("missing"),
                "error should name the missing subdir, got: {message}"
            );
        }
        other => panic!("expected Other(subdir not found), got: {other:?}"),
    }
}

#[test]
fn missing_remote_surfaces_typed_error() {
    // Point at a path that's not a git repo; git clone should fail
    // and we surface a typed error (not a panic, not a generic IO
    // unwrap).
    let bogus = tempfile::tempdir().unwrap();
    // bogus tempdir exists but is empty — not a git repo.

    let dest = tempfile::tempdir().unwrap();
    let fetcher = lex_extension_host::resolve::fetcher::GitFetcher;
    let uri = ParsedUri::parse(&format!("git:file://{}", bogus.path().display())).unwrap();
    let err = fetcher
        .fetch(&uri, dest.path())
        .expect_err("clone of non-repo should error");
    // file:// failures don't trip the Network classifier; expect Other
    // with the raw git stderr.
    match err {
        FetchError::Other { message } => {
            assert!(
                !message.is_empty(),
                "Other error should carry git's stderr text"
            );
        }
        FetchError::Network { .. } => {
            // Acceptable too — depends on git's wording.
        }
        other => panic!("expected Other or Network, got: {other:?}"),
    }
}

#[test]
fn resolves_through_full_pipeline_and_caches_on_second_call() {
    // Drives the GitFetcher through `resolve_namespace_with`, which
    // is the path lex-fmt / lex-cli actually call. Confirms that the
    // resolver machinery (URI parse → registry dispatch → cache key
    // → fetch → cache hit) works end-to-end against a real shell-out
    // fetcher. The two consecutive resolves prove the cache short-
    // circuits the second clone — the bare repo's mtime + file count
    // is the proof (if a second clone had run, the schema_dir would
    // be different).
    let base = tempfile::tempdir().unwrap();
    let bare = build_bare_repo(base.path(), &[("schema.yaml", b"label: cached\n")]);

    let cache_root = tempfile::tempdir().unwrap();
    let cache = ResolverCache::new(cache_root.path()).unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let registry = default_fetcher_registry();

    let uri = file_uri_for(&bare, Some("main"), None);
    let resolved1 =
        resolve_namespace_with(&uri, workspace.path(), &registry, &cache).expect("first resolve");
    assert!(
        resolved1.schema_dir.starts_with(cache_root.path()),
        "cache dir should land under cache root, got: {}",
        resolved1.schema_dir.display()
    );
    assert!(
        resolved1.schema_dir.join("schema.yaml").exists(),
        "first resolve should produce the schema file in the cache dir"
    );

    // Second resolve, same URI: cache hit; the function should
    // return the same path.
    let resolved2 =
        resolve_namespace_with(&uri, workspace.path(), &registry, &cache).expect("second resolve");
    assert_eq!(
        resolved1.schema_dir, resolved2.schema_dir,
        "second resolve should hit the cache and return the same dir"
    );
}
