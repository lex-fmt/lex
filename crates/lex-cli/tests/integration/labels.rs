//! End-to-end tests for `lexd labels` and the `--ext-schema` /
//! `--enable-handlers` global flags. Runs the actual `lexd` binary
//! against fixture workspaces.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Build a minimal workspace: `.lex.toml` (optionally with a
/// `[labels]` block), and an `acme/` directory of YAML schemas the
/// CLI can register via `--ext-schema`.
fn make_workspace_with_acme_schemas(labels_toml: Option<&str>) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let lex_toml = labels_toml.unwrap_or("# empty\n");
    fs::write(dir.path().join(".lex.toml"), lex_toml).unwrap();

    let acme_dir = dir.path().join("acme");
    fs::create_dir(&acme_dir).unwrap();
    fs::write(
        acme_dir.join("task.yaml"),
        "schema_version: 1\nlabel: acme.task\nattaches_to: [annotation, paragraph, document]\n",
    )
    .unwrap();
    fs::write(
        acme_dir.join("note.yaml"),
        "schema_version: 1\nlabel: acme.note\nattaches_to: [annotation]\n",
    )
    .unwrap();
    dir
}

#[test]
fn labels_list_prints_builtin_when_no_namespaces_declared() {
    let dir = make_workspace_with_acme_schemas(None);
    Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args(["labels", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("lex"));
}

#[test]
fn labels_list_with_path_uri_in_lex_toml() {
    let dir = make_workspace_with_acme_schemas(Some(
        r#"
[labels]
acme = "path:acme"
"#,
    ));
    Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args(["labels", "list"])
        .assert()
        .success()
        .stdout(
            predicates::str::contains("acme")
                .and(predicates::str::contains("2 schemas"))
                .and(predicates::str::contains("path:acme")),
        );
}

#[test]
fn labels_list_with_ext_schema_flag() {
    let dir = make_workspace_with_acme_schemas(None);
    Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args(["labels", "list", "--ext-schema", "acme"])
        .assert()
        .success()
        .stdout(predicates::str::contains("acme").and(predicates::str::contains("--ext-schema")));
}

#[test]
fn labels_list_with_remote_uri_emits_unimplemented_diagnostic() {
    let dir = make_workspace_with_acme_schemas(Some(
        r#"
[labels]
remote = { tap = "remote" }
"#,
    ));
    Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args(["labels", "list"])
        .assert()
        .success() // listing succeeds; the diagnostic is in stdout
        .stdout(predicates::str::contains("not yet implemented"));
}

#[test]
fn labels_validate_returns_zero_for_clean_document() {
    let dir = make_workspace_with_acme_schemas(None);
    let doc = dir.path().join("hello.lex");
    fs::write(&doc, "Hello, world.\n").unwrap();
    Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args(["labels", "validate", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn labels_validate_with_ext_schema_finds_unknown_label() {
    // Document uses acme.unknown which is NOT in the schema set
    // (acme.task, acme.note are registered). Walker emits an
    // UnknownLabel diagnostic and `validate` exits 1.
    let dir = make_workspace_with_acme_schemas(None);
    let doc = dir.path().join("bad.lex");
    fs::write(&doc, ":: acme.unknown ::\n").unwrap();
    Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args([
            "labels",
            "validate",
            "--ext-schema",
            "acme",
            doc.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicates::str::contains("acme.unknown").and(predicates::str::contains("error")));
}

#[test]
fn labels_with_reserved_lex_namespace_in_toml_fails_load() {
    let dir = make_workspace_with_acme_schemas(Some(
        r#"
[labels]
lex = "github:fake/lex-labels"
"#,
    ));
    Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args(["labels", "list"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("reserved"));
}

// `lexd check-labels` — PR 5 of #584.

#[test]
fn check_labels_exits_zero_on_clean_document() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("clean.lex");
    fs::write(&doc, ":: author :: Alice\n\n1. Intro\n\n    Body.\n").unwrap();
    Command::cargo_bin("lexd")
        .unwrap()
        .args(["check-labels", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn check_labels_exits_one_on_forbidden_doc_prefix() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("bad.lex");
    fs::write(&doc, ":: doc.table ::\n\nBody.\n").unwrap();
    Command::cargo_bin("lexd")
        .unwrap()
        .args(["check-labels", doc.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(
            predicates::str::contains("doc.table")
                .and(predicates::str::contains("forbidden-label-prefix"))
                .and(predicates::str::contains("1 label-policy violation")),
        );
}

#[test]
fn check_labels_exits_one_on_unknown_lex_canonical() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("bad.lex");
    fs::write(&doc, ":: lex.foobar :: x\n\nBody.\n").unwrap();
    Command::cargo_bin("lexd")
        .unwrap()
        .args(["check-labels", doc.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(
            predicates::str::contains("lex.foobar")
                .and(predicates::str::contains("unknown-lex-canonical")),
        );
}

#[test]
fn check_labels_reports_every_violation_in_one_run() {
    // Permissive parse means every violation surfaces in a single
    // invocation — useful for batch CI runs.
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("multi.lex");
    fs::write(
        &doc,
        ":: doc.table :: x\n:: doc.image :: y\n:: lex.foobar :: z\n\nBody.\n",
    )
    .unwrap();
    Command::cargo_bin("lexd")
        .unwrap()
        .args(["check-labels", doc.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("3 label-policy violation"));
}

#[test]
fn check_labels_exits_two_on_missing_file() {
    Command::cargo_bin("lexd")
        .unwrap()
        .args(["check-labels", "/nonexistent/path/that/does/not/exist.lex"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("failed to read"));
}
