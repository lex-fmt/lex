#[cfg(unix)]
mod unix {
    use assert_cmd::cargo::cargo_bin_cmd;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn write_stub_chrome() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("fake-chrome.sh");
        let script = r#"#!/bin/sh
OUTPUT=""
for arg in "$@"; do
  case $arg in
    --print-to-pdf=*)
      OUTPUT="${arg#*=}"
      ;;
  esac
done
printf '%%PDF-1.7\n%%%%EOF\n' > "$OUTPUT"
"#;
        fs::write(&script_path, script).unwrap();
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();
        (dir, script_path)
    }

    #[test]
    fn cli_converts_to_pdf_with_stub() {
        let (_dir, chrome_stub) = write_stub_chrome();
        let output_dir = tempdir().unwrap();
        let output_pdf = output_dir.path().join("out.pdf");

        let mut cmd = cargo_bin_cmd!("lex");
        cmd.env("LEX_CHROME_BIN", &chrome_stub)
            .arg("../../comms/specs/benchmark/010-kitchensink.lex")
            .arg("--to")
            .arg("pdf")
            .arg("-o")
            .arg(&output_pdf)
            .arg("--pdf-size")
            .arg("mobile");

        cmd.assert().success();

        let pdf = fs::read(&output_pdf).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn cli_pdf_requires_output_path() {
        let (_dir, chrome_stub) = write_stub_chrome();
        let mut cmd = cargo_bin_cmd!("lex");
        cmd.env("LEX_CHROME_BIN", &chrome_stub)
            .arg("../../comms/specs/benchmark/010-kitchensink.lex")
            .arg("--to")
            .arg("pdf");

        cmd.assert().failure();
    }
}

#[cfg(not(unix))]
#[test]
fn pdf_cli_tests_skipped() {
    eprintln!("Skipping PDF CLI tests on non-Unix platforms");
}
