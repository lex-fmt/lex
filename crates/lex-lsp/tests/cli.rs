use std::process::{Command, Stdio};

#[test]
fn lex_lsp_binary_starts_and_stops() {
    let exe = env!("CARGO_BIN_EXE_lexd-lsp");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start lexd-lsp binary");

    // Immediately terminate the server; we only need to ensure it starts.
    child.kill().expect("failed to stop lexd-lsp binary");
    let _ = child.wait();
}
