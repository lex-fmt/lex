//! Integration tests for the `lexd check` subcommand.
//!
//! `check` lints documents over the include-expanded AST and reports
//! findings with a CI-friendly exit-code contract (0 clean / 1 findings
//! at-or-above the fail threshold / 2 operational error). These tests
//! drive the real binary against on-disk fixtures (`tempfile::TempDir`)
//! so include resolution and config loading are exercised end-to-end.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// Scaffolding (mirrors includes.rs)
// ============================================================================

fn fixture_dir(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    for (rel, contents) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir -p");
        }
        fs::write(&path, contents).expect("write fixture file");
    }
    dir
}

fn path_in(dir: &TempDir, rel: &str) -> std::path::PathBuf {
    dir.path().join(rel)
}

fn lexd() -> assert_cmd::Command {
    cargo_bin_cmd!("lexd")
}

// ============================================================================
// Clean document → exit 0, no output
// ============================================================================

#[test]
fn clean_document_exits_zero_with_no_output() {
    let dir = fixture_dir(&[("clean.lex", "1. Intro\n\n    Body of the intro.\n")]);
    lexd()
        .arg("check")
        .arg(path_in(&dir, "clean.lex"))
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

// ============================================================================
// Missing footnote definition → exit 1, one finding
// ============================================================================

#[test]
fn missing_footnote_exits_one_with_finding() {
    let dir = fixture_dir(&[("doc.lex", "Text with [1] reference.\n")]);
    lexd()
        .arg("check")
        .arg(path_in(&dir, "doc.lex"))
        .assert()
        .failure()
        .code(1);
}

#[test]
fn missing_footnote_human_output_names_code_and_severity() {
    let dir = fixture_dir(&[("doc.lex", "Text with [1] reference.\n")]);
    lexd()
        .args(["check"])
        .arg(path_in(&dir, "doc.lex"))
        .assert()
        .failure()
        .code(1)
        .stdout(
            predicate::str::contains("missing-footnote")
                .and(predicate::str::contains("error:"))
                .and(predicate::str::contains("1 finding")),
        );
}

// ============================================================================
// Broken include (src= not found) → exit 1 via include error, blamed on site
// ============================================================================

#[test]
fn broken_include_exits_one_blamed_on_site() {
    let dir = fixture_dir(&[("main.lex", ":: lex.include src=\"missing.lex\" ::\n")]);
    lexd()
        .arg("check")
        .arg(path_in(&dir, "main.lex"))
        .assert()
        .failure()
        .code(1)
        .stdout(
            predicate::str::contains("include-not-found")
                .and(predicate::str::contains("missing.lex")),
        );
}

// ============================================================================
// --no-includes skips expansion
// ============================================================================

#[test]
fn no_includes_skips_expansion() {
    // With expansion ON, a missing include target is a finding. With
    // --no-includes, the resolver never runs, so the include-not-found
    // finding does not appear.
    let dir = fixture_dir(&[("main.lex", ":: lex.include src=\"missing.lex\" ::\n")]);

    lexd()
        .arg("check")
        .arg(path_in(&dir, "main.lex"))
        .arg("--no-includes")
        .assert()
        .stdout(predicate::str::contains("include-not-found").not());
}

// ============================================================================
// --format json valid + stable
// ============================================================================

#[test]
fn json_format_is_valid_and_shaped() {
    let dir = fixture_dir(&[("doc.lex", "Text with [1] reference.\n")]);
    let output = lexd()
        .arg("check")
        .arg(path_in(&dir, "doc.lex"))
        .args(["--format", "json"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("check --format json emits valid JSON");
    let arr = json.as_array().expect("top level is an array");
    assert_eq!(arr.len(), 1, "exactly one finding: {arr:?}");
    let f = &arr[0];
    assert_eq!(f["code"], "missing-footnote");
    assert_eq!(f["severity"], "error");
    assert!(f["path"].is_string());
    assert!(f["message"].is_string());
    assert!(f["range"]["start"]["line"].is_number());
    assert!(f["range"]["start"]["column"].is_number());
}

#[test]
fn json_clean_document_is_empty_array() {
    let dir = fixture_dir(&[("clean.lex", "Just a paragraph.\n")]);
    let output = lexd()
        .arg("check")
        .arg(path_in(&dir, "clean.lex"))
        .args(["--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(json.as_array().expect("array").len(), 0);
}

// ============================================================================
// A .lex.toml rule downgrade silences a finding
// ============================================================================

#[test]
fn lex_toml_rule_allow_silences_finding() {
    // Baseline: the missing footnote fires.
    let dir = fixture_dir(&[
        ("doc.lex", "Text with [1] reference.\n"),
        (
            ".lex.toml",
            "[diagnostics.rules]\nmissing_footnote = \"allow\"\n",
        ),
    ]);

    // `allow` drops the diagnostic entirely → clean exit 0.
    lexd()
        .arg("check")
        .arg(path_in(&dir, "doc.lex"))
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn fail_on_error_passes_when_only_warnings_present() {
    // An unclosed annotation is a warning. With the default threshold
    // (warning) it fails; raising the threshold to `error` keeps the
    // finding visible on stdout but the run exits 0 — proving the
    // `--fail-on` gate is what decides the exit code, not the presence
    // of findings.
    let dir = fixture_dir(&[("warn.lex", ":: note\nSome text.\n")]);

    // Default threshold: the warning fails the run.
    lexd()
        .arg("check")
        .arg(path_in(&dir, "warn.lex"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("unclosed-annotation"));

    // Raised threshold: same finding printed, but exit 0.
    lexd()
        .arg("check")
        .arg(path_in(&dir, "warn.lex"))
        .args(["--fail-on", "error"])
        .assert()
        .success()
        .stdout(predicate::str::contains("unclosed-annotation"));
}

#[test]
fn fail_on_error_still_fails_on_an_error_finding() {
    // missing-footnote is intrinsically error-severity, so raising the
    // threshold to `error` keeps the run failing. (The companion
    // `allow` test above proves a rule override silences a finding; this
    // pins the `--fail-on` threshold interaction.)
    let dir = fixture_dir(&[("doc.lex", "Text with [1] reference.\n")]);
    lexd()
        .arg("check")
        .arg(path_in(&dir, "doc.lex"))
        .args(["--fail-on", "error"])
        .assert()
        .failure()
        .code(1);
}

// ============================================================================
// Unreadable file / bad args → exit 2 (not 1)
// ============================================================================

#[test]
fn unreadable_file_exits_two() {
    lexd()
        .arg("check")
        .arg("/nonexistent/path/that/does/not/exist.lex")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("cannot read"));
}

#[test]
fn bad_fail_on_value_exits_two() {
    // clap rejects an out-of-set --fail-on value with usage exit code 2.
    let dir = fixture_dir(&[("doc.lex", "Body.\n")]);
    lexd()
        .arg("check")
        .arg(path_in(&dir, "doc.lex"))
        .args(["--fail-on", "bogus"])
        .assert()
        .failure()
        .code(2);
}

// ============================================================================
// A diagnostic originating inside an included file prints the include's path
// ============================================================================

#[test]
fn diagnostic_inside_include_reports_include_path() {
    // The forbidden `doc.*` label lives inside the included fragment.
    // After expansion the diagnostic originates in frag.lex, so the
    // reported source path must be the fragment, not the entry file.
    let dir = fixture_dir(&[
        ("book.lex", ":: lex.include src=\"frag.lex\" ::\n"),
        ("frag.lex", ":: doc.table ::\n\nBody.\n"),
    ]);
    lexd()
        .arg("check")
        .arg(path_in(&dir, "book.lex"))
        .assert()
        .failure()
        .code(1)
        // The schema diagnostic carries the stamped origin: frag.lex.
        .stdout(predicate::str::contains("frag.lex"));
}

// ============================================================================
// Multiple files → aggregate exit = max
// ============================================================================

#[test]
fn multiple_files_aggregate_exit_is_max() {
    // One clean file + one unreadable file → aggregate 2 (operational
    // beats both clean-0 and a hypothetical finding-1).
    let dir = fixture_dir(&[("clean.lex", "Body.\n")]);
    lexd()
        .arg("check")
        .arg(path_in(&dir, "clean.lex"))
        .arg("/nonexistent/missing.lex")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn multiple_files_findings_aggregate_to_one() {
    let dir = fixture_dir(&[
        ("a.lex", "Text with [1] reference.\n"),
        ("b.lex", "Clean body.\n"),
    ]);
    lexd()
        .arg("check")
        .arg(path_in(&dir, "a.lex"))
        .arg(path_in(&dir, "b.lex"))
        .assert()
        .failure()
        .code(1);
}

// ============================================================================
// check-labels subsumption: forbidden / unknown-canonical labels still flagged
// ============================================================================

#[test]
fn forbidden_label_prefix_still_flagged_by_plain_check() {
    let dir = fixture_dir(&[("doc.lex", ":: doc.table ::\n\nBody.\n")]);
    lexd()
        .arg("check")
        .arg(path_in(&dir, "doc.lex"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("forbidden-label-prefix"));
}

#[test]
fn unknown_lex_canonical_still_flagged_by_plain_check() {
    let dir = fixture_dir(&[("doc.lex", ":: lex.foobar :: x\n\nBody.\n")]);
    lexd()
        .arg("check")
        .arg(path_in(&dir, "doc.lex"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("unknown-lex-canonical"));
}
