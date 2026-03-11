use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;

#[test]
fn test_inspect_nodemap_basic() {
    let content = "# Session\n\nPara";
    let file = "test_nodemap.lex";
    fs::write(file, content).unwrap();

    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("inspect").arg(file).arg("ast-nodemap");

    cmd.assert()
        .success()
        // Output should not be empty
        .stdout(predicate::str::is_empty().not());

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();

    // Source has 3 lines. Output should have 3 lines.
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].chars().count(), 9); // # Session
    assert_eq!(lines[1], "");
    assert_eq!(lines[2].chars().count(), 4); // Para

    fs::remove_file(file).ok();
}

#[test]
fn test_inspect_nodemap_color() {
    let content = "# Session";
    let file = "test_nodemap_color.lex";
    fs::write(file, content).unwrap();

    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("inspect")
        .arg(file)
        .arg("ast-nodemap")
        .arg("--extra-color");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("\x1b[")); // ANSI code

    fs::remove_file(file).ok();
}

#[test]
fn test_inspect_nodemap_summary() {
    let content = "# Session\n\nPara";
    let file = "test_nodemap_summary.lex";
    fs::write(file, content).unwrap();

    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("inspect")
        .arg(file)
        .arg("ast-nodemap")
        .arg("--extra-nodesummary");

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();

    assert!(stdout.contains("Ast Nodes ="));
    assert!(stdout.contains("Median Node Size ="));
    assert!(stdout.contains("1 char ast node ="));

    fs::remove_file(file).ok();
}
