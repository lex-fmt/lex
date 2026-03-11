use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use tempfile::tempdir;

#[test]
fn format_respects_indent_from_config() {
    let dir = tempdir().unwrap();
    let input_path = dir.path().join("doc.lex");
    fs::write(&input_path, "Session:\n    Body\n").unwrap();

    let config_path = dir.path().join("lex.toml");
    fs::write(
        &config_path,
        r#"[formatting.rules]
indent_string = "  "
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lex");
    cmd.arg("format")
        .arg(input_path.as_os_str())
        .arg("--config")
        .arg(config_path.as_os_str());

    let output = cmd.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8(output).unwrap();
    assert!(stdout.contains("\n  Body\n"));
    assert!(!stdout.contains("\n    Body\n"));
}
