use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

const SAMPLE_LEX: &str = "Hello:\n    World.\n";

#[test]
fn inspect_reads_from_stdin_with_default_transform() {
    let mut cmd = cargo_bin_cmd!("lexd");
    cmd.arg("inspect").write_stdin(SAMPLE_LEX);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Document"));
}

#[test]
fn inspect_reads_from_stdin_with_positional_transform() {
    let mut cmd = cargo_bin_cmd!("lexd");
    cmd.arg("inspect").arg("ast-tag").write_stdin(SAMPLE_LEX);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("<document>"))
        .stdout(predicate::str::contains("<definition"));
}

#[test]
fn convert_reads_from_stdin_when_from_is_set() {
    let mut cmd = cargo_bin_cmd!("lexd");
    cmd.arg("convert")
        .arg("--from")
        .arg("lex")
        .arg("--to")
        .arg("markdown")
        .write_stdin(SAMPLE_LEX);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("**Hello**"));
}

#[test]
fn convert_injected_reads_from_stdin_when_from_is_set() {
    let mut cmd = cargo_bin_cmd!("lexd");
    // No explicit `convert` subcommand - should be auto-injected from --to flag.
    cmd.arg("--from")
        .arg("lex")
        .arg("--to")
        .arg("markdown")
        .write_stdin(SAMPLE_LEX);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("**Hello**"));
}

#[test]
fn convert_without_from_flag_errors_on_stdin() {
    let mut cmd = cargo_bin_cmd!("lexd");
    cmd.arg("convert")
        .arg("--to")
        .arg("markdown")
        .write_stdin(SAMPLE_LEX);

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("--from is required"));
}

#[test]
fn format_reads_from_stdin() {
    let mut cmd = cargo_bin_cmd!("lexd");
    cmd.arg("format").write_stdin(SAMPLE_LEX);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Hello:"))
        .stdout(predicate::str::contains("World."));
}
