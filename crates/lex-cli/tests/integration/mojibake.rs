//! CLI integration tests for the mojibake-detection warning (#608).
//!
//! `lexd convert` and `lexd format` scan their input for UTF-8
//! double-encoding signatures and print a single warning to stderr when
//! the input looks corrupted. The warning is informational — it never
//! blocks the conversion, and `--no-warnings` or `LEX_QUIET=1` suppresses
//! it.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

/// A small lex paragraph with multiple cp1252 → UTF-8 mojibake glyphs,
/// enough to clear the ≥3-distinct-pattern threshold.
const MOJIBAKE_LEX: &str = "RÃ©sumÃ© of the MÃ¶bius cafÃ©: Ã¼nique Ã±otes â€\u{201D} indeed.\n";

const CLEAN_LEX: &str = "A perfectly clean paragraph with no mojibake.\n";

#[test]
fn convert_warns_on_mojibake_input() {
    let mut cmd = cargo_bin_cmd!("lexd");
    cmd.arg("convert")
        .arg("--from")
        .arg("lex")
        .arg("--to")
        .arg("markdown")
        .write_stdin(MOJIBAKE_LEX);

    cmd.assert().success().stderr(predicate::str::contains(
        "appears to be UTF-8-double-encoded",
    ));
}

#[test]
fn convert_stays_silent_on_clean_input() {
    let mut cmd = cargo_bin_cmd!("lexd");
    cmd.arg("convert")
        .arg("--from")
        .arg("lex")
        .arg("--to")
        .arg("markdown")
        .write_stdin(CLEAN_LEX);

    cmd.assert()
        .success()
        .stderr(predicate::str::contains("UTF-8-double-encoded").not());
}

#[test]
fn no_warnings_flag_suppresses_mojibake_warning() {
    let mut cmd = cargo_bin_cmd!("lexd");
    cmd.arg("--no-warnings")
        .arg("convert")
        .arg("--from")
        .arg("lex")
        .arg("--to")
        .arg("markdown")
        .write_stdin(MOJIBAKE_LEX);

    cmd.assert()
        .success()
        .stderr(predicate::str::contains("UTF-8-double-encoded").not());
}

#[test]
fn lex_quiet_env_suppresses_mojibake_warning() {
    let mut cmd = cargo_bin_cmd!("lexd");
    cmd.env("LEX_QUIET", "1")
        .arg("convert")
        .arg("--from")
        .arg("lex")
        .arg("--to")
        .arg("markdown")
        .write_stdin(MOJIBAKE_LEX);

    cmd.assert()
        .success()
        .stderr(predicate::str::contains("UTF-8-double-encoded").not());
}

#[test]
fn format_warns_on_mojibake_input() {
    let mut cmd = cargo_bin_cmd!("lexd");
    cmd.arg("format").write_stdin(MOJIBAKE_LEX);

    cmd.assert().success().stderr(predicate::str::contains(
        "appears to be UTF-8-double-encoded",
    ));
}
