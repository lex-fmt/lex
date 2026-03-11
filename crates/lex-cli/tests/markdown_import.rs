use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("lex-babel")
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn convert_markdown_to_tag_via_cli() {
    let fixture = fixture_path("markdown-reference-commonmark.md");
    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("convert").arg(&fixture).arg("--to").arg("tag");

    // Verify comprehensive output structure from markdown import
    let output_pred = predicate::str::contains("<document>")
        .and(predicate::str::contains("<session>CommonMark"))
        .and(predicate::str::contains("<paragraph>"))
        .and(predicate::str::contains("<list>"))
        .and(predicate::str::contains("Running tests against the spec"));

    cmd.assert().success().stdout(output_pred);
}
