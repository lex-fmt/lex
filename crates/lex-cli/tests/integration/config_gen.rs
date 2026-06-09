//! End-to-end tests for `lexd config gen`'s extension-diagnostic
//! "discovery channel" (#659 / #707): with a registered namespace that
//! declares diagnostic codes, `config gen` appends a commented-out
//! `[diagnostics.rules]` entry per declared code, annotated with its
//! description and default severity.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Build a workspace whose `[labels]` block registers an `acme`
/// namespace from a local `acme/` schema dir. The schema declares two
/// diagnostic codes so `config gen` has something to enumerate.
fn workspace_with_declared_diagnostics() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".lex.toml"),
        "[labels]\nacme = \"path:acme\"\n",
    )
    .unwrap();

    let acme_dir = dir.path().join("acme");
    fs::create_dir(&acme_dir).unwrap();
    fs::write(
        acme_dir.join("task.yaml"),
        "schema_version: 1\n\
         label: acme.task\n\
         attaches_to: [annotation, paragraph]\n\
         diagnostics:\n  \
         - code: task-due-date-missing\n    \
         description: A task is missing its due date.\n    \
         default_severity: warning\n  \
         - code: task-overdue\n    \
         description: A task's due date is in the past.\n    \
         default_severity: error\n",
    )
    .unwrap();
    dir
}

#[test]
fn config_gen_emits_commented_entry_per_declared_code() {
    let dir = workspace_with_declared_diagnostics();
    Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args(["config", "gen"])
        .assert()
        .success()
        .stdout(
            // The discovery section header + the rules table.
            predicates::str::contains("[diagnostics.rules]")
                // One commented entry per declared code, keyed by the
                // on-the-wire `<namespace>.<code>`.
                .and(predicates::str::contains(
                    "# \"acme.task-due-date-missing\" = \"warn\"",
                ))
                .and(predicates::str::contains(
                    "# \"acme.task-overdue\" = \"warn\"",
                ))
                // Each annotated with its declared description ...
                .and(predicates::str::contains(
                    "# A task is missing its due date.",
                ))
                .and(predicates::str::contains(
                    "# A task's due date is in the past.",
                ))
                // ... and its declared default severity.
                .and(predicates::str::contains("# default severity: warning"))
                .and(predicates::str::contains("# default severity: error")),
        );
}

#[test]
fn config_gen_output_is_valid_toml_with_multiline_descriptions() {
    // Regression (lex#750 review): a multi-line declared description must have
    // EVERY line commented, and the discovery section must NOT emit a second
    // `[diagnostics.rules]` table header (the base template already defines it) —
    // either would yield invalid TOML.
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".lex.toml"),
        "[labels]\nacme = \"path:acme\"\n",
    )
    .unwrap();
    let acme_dir = dir.path().join("acme");
    fs::create_dir(&acme_dir).unwrap();
    fs::write(
        acme_dir.join("task.yaml"),
        "schema_version: 1\n\
         label: acme.task\n\
         attaches_to: [annotation, paragraph]\n\
         diagnostics:\n  \
         - code: task-overdue\n    \
         description: |\n      \
         First line of the description.\n      \
         Second line that must also be commented.\n    \
         default_severity: error\n",
    )
    .unwrap();

    let out = Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args(["config", "gen"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();

    // every line of the multi-line description is commented
    assert!(stdout.contains("# First line of the description."));
    assert!(stdout.contains("# Second line that must also be commented."));
    // exactly one (base-template) `[diagnostics.rules]` table header — the
    // discovery section adds none, so the TOML never redefines the table.
    let headers = stdout
        .lines()
        .filter(|l| l.trim_start() == "[diagnostics.rules]")
        .count();
    assert_eq!(
        headers, 1,
        "expected exactly one [diagnostics.rules] header, got {headers}\n{stdout}"
    );
}

#[test]
fn config_gen_without_declared_diagnostics_omits_section() {
    // A bare workspace (no `[labels]`, no extension schemas) has no
    // declared diagnostic codes, so `config gen` emits the plain
    // schema template with no discovery section.
    let dir = TempDir::new().unwrap();
    Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args(["config", "gen"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Extension diagnostic rules").not());
}
