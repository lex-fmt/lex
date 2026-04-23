#[cfg(all(unix, feature = "native-export"))]
mod unix {
    use lex_babel::format::{Format, SerializedDocument};
    use lex_babel::formats::pdf::PdfFormat;
    use lex_core::lex::transforms::standard::STRING_TO_AST;
    use std::collections::HashMap;
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
if [ -z "$OUTPUT" ]; then
  echo "missing output" >&2
  exit 1
fi
printf '%%PDF-1.7\n%%%%EOF\n' > "$OUTPUT"
exit 0
"#;
        fs::write(&script_path, script).unwrap();
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();
        (dir, script_path)
    }

    #[test]
    fn pdf_renderer_uses_chrome_stub() {
        let (_dir, chrome_stub) = write_stub_chrome();
        let prev = std::env::var("LEX_CHROME_BIN").ok();
        std::env::set_var("LEX_CHROME_BIN", &chrome_stub);

        let lex_src = "Paragraph in pdf test.\n";
        let doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
        let format = PdfFormat::default();
        let mut options = HashMap::new();
        options.insert("size-mobile".to_string(), "true".to_string());

        let result = format.serialize_with_options(&doc, &options).unwrap();
        match result {
            SerializedDocument::Binary(bytes) => {
                assert!(bytes.starts_with(b"%PDF"));
            }
            _ => panic!("Expected binary PDF output"),
        }

        if let Some(prev) = prev {
            std::env::set_var("LEX_CHROME_BIN", prev);
        } else {
            std::env::remove_var("LEX_CHROME_BIN");
        }
    }
}

#[cfg(not(all(unix, feature = "native-export")))]
#[test]
fn pdf_stub_skipped() {
    eprintln!("Skipping PDF tests (native-export feature or Unix required)");
}
