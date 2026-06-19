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
fn labels_list_with_unresolvable_uri_emits_diagnostic() {
    // Uses an `ftp:` URI — no fetcher in the default registry claims
    // that scheme, so resolve fails with UnknownScheme and the CLI
    // surfaces a per-namespace diagnostic. (Previously this test
    // pointed at `git+ssh:` to exercise FetchError::Unimplemented,
    // but `git:` / `git+ssh:` ship a real fetcher now; `ftp:` is
    // the stable proxy for "unresolvable remote URI in lex.toml".)
    let dir = make_workspace_with_acme_schemas(Some(
        r#"
[labels.remote]
uri = "ftp:server/path"
"#,
    ));
    Command::cargo_bin("lexd")
        .unwrap()
        .current_dir(dir.path())
        .args(["labels", "list"])
        .assert()
        .success() // listing succeeds; the diagnostic is in stdout
        .stdout(predicates::str::contains("ftp"));
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
