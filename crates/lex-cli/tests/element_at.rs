use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;

#[allow(deprecated)]
#[test]
fn test_element_at_basic() {
    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("element-at")
        .arg("../../comms/specs/benchmark/010-kitchensink.lex")
        .arg("17")
        .arg("5");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Session:"));
}

#[test]
fn test_element_at_with_all_flag() {
    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("element-at")
        .arg("../../comms/specs/benchmark/010-kitchensink.lex")
        .arg("17")
        .arg("5")
        .arg("--all");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Document:"))
        .stdout(predicate::str::contains("Session:"));
}

#[test]
fn test_element_at_no_element_found() {
    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("element-at")
        .arg("../../comms/specs/benchmark/010-kitchensink.lex")
        .arg("10000")
        .arg("10000");

    // When no element is found, the command exits with 0 but prints to stderr
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("No element found"));
}

#[test]
fn test_element_at_missing_arguments() {
    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("element-at")
        .arg("comms/specs/benchmark/010-kitchensink.lex");

    cmd.assert().failure();
}

#[test]
fn test_element_at_file_not_found() {
    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("element-at")
        .arg("nonexistent.lex")
        .arg("1")
        .arg("0");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Error reading file"));
}

#[test]
fn test_element_at_on_paragraph() {
    // Create a simple test file
    let test_content = "This is a paragraph.";
    let test_file = "test_element_at_paragraph.lex";
    fs::write(test_file, test_content).unwrap();

    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("element-at").arg(test_file).arg("1").arg("5");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("TextLine:"));

    // Cleanup
    fs::remove_file(test_file).ok();
}

#[test]
fn test_element_at_shows_deepest_element() {
    // Create a test file with nested structure
    let test_content = "Session Title\n\n    Nested paragraph.";
    let test_file = "test_element_at_nested.lex";
    fs::write(test_file, test_content).unwrap();

    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("element-at").arg(test_file).arg("3").arg("5");

    // Without --all, should show only the deepest element
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("TextLine:"));

    // Cleanup
    fs::remove_file(test_file).ok();
}

#[test]
fn test_element_at_all_shows_ancestors() {
    // Create a test file with nested structure
    let test_content = "Session Title\n\n    Nested paragraph.";
    let test_file = "test_element_at_all.lex";
    fs::write(test_file, test_content).unwrap();

    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("element-at")
        .arg(test_file)
        .arg("3")
        .arg("5")
        .arg("--all");

    // With --all, should show all ancestors
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Document:"))
        .stdout(predicate::str::contains("Session:"))
        .stdout(predicate::str::contains("Paragraph:"))
        .stdout(predicate::str::contains("TextLine:"));

    // Cleanup
    fs::remove_file(test_file).ok();
}
