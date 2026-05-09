//! Multi-mode test fixture binary for the subprocess transport.
//!
//! Spawned by the integration tests via the schema's `command` array.
//! One binary covers every behaviour the test suite needs by selecting
//! a mode through argv. Keeping the modes in one binary avoids 5
//! separate `[[bin]]` entries — a build-cost and review-surface saving
//! that doesn't lose any coverage.
//!
//! Usage:
//!   lex-extension-host-fixture-handler <mode> [args...]
//!
//! Modes:
//!   echo                    — round-trip every hook with a deterministic
//!                             response based on the request payload
//!   slow --delay-ms <N>     — sleep N ms before responding to every
//!                             non-initialize request (used for timeout tests)
//!   crash --on <method>     — exit non-zero immediately when the named
//!                             method is invoked
//!   malformed               — respond to on_validate with non-JSON garbage
//!                             on stdout, then close
//!   version-mismatch --version <N>
//!                           — initialize with wire_version: N (default 99)
//!
//! All modes write LSP-framed JSON-RPC on stdout and read it from stdin,
//! matching the `lex-extension-host::transport::subprocess` framing.

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use serde_json::{json, Value};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("echo");

    match mode {
        "echo" => run(Mode::Echo),
        "slow" => {
            let ms = parse_arg_u64(&args, "--delay-ms").unwrap_or(10_000);
            run(Mode::Slow {
                delay: Duration::from_millis(ms),
            })
        }
        "crash" => {
            let method = parse_arg_str(&args, "--on").unwrap_or_else(|| "on_validate".into());
            run(Mode::Crash { on_method: method })
        }
        "malformed" => run(Mode::Malformed),
        "version-mismatch" => {
            let v = parse_arg_u64(&args, "--version").unwrap_or(99) as u32;
            run(Mode::VersionMismatch { version: v })
        }
        other => {
            eprintln!("unknown fixture mode: {other}");
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, Clone)]
enum Mode {
    Echo,
    Slow { delay: Duration },
    Crash { on_method: String },
    Malformed,
    VersionMismatch { version: u32 },
}

fn run(mode: Mode) -> ExitCode {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    loop {
        let frame = match read_frame(&mut stdin) {
            Ok(Some(v)) => v,
            Ok(None) => return ExitCode::SUCCESS, // EOF on stdin: parent closed.
            Err(e) => {
                eprintln!("fixture: read error: {e}");
                return ExitCode::from(1);
            }
        };

        let id = frame.get("id").and_then(|v| v.as_u64());
        let method = frame
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Notifications (no `id`) are accepted but produce no reply.
        let Some(id) = id else {
            // shutdown notification arrives without id; we just exit.
            if method == "shutdown" {
                return ExitCode::SUCCESS;
            }
            continue;
        };

        if let Mode::Crash { on_method } = &mode {
            if &method == on_method {
                eprintln!("fixture: crashing on `{method}` as instructed");
                return ExitCode::from(7);
            }
        }

        if let Mode::Slow { delay } = &mode {
            if method != "initialize" {
                std::thread::sleep(*delay);
            }
        }

        let result = match method.as_str() {
            "initialize" => initialize_result(&mode),
            "on_label" => continue, // notification, no reply (defensive)
            "on_validate" => {
                if matches!(mode, Mode::Malformed) {
                    // Write garbage and close — the host should fail
                    // on parse error.
                    let garbage = b"Content-Length: 5\r\n\r\nNOTJSON";
                    let _ = stdout.write_all(garbage);
                    let _ = stdout.flush();
                    return ExitCode::SUCCESS;
                }
                json!({ "diagnostics": diagnostics_for(&frame) })
            }
            "on_resolve" => json!({ "replacement": resolve_for(&frame) }),
            "on_render" => json!({ "output": render_for(&frame) }),
            "on_hover" => json!({ "hover": hover_for(&frame) }),
            "on_completion" => json!({ "items": completion_for(&frame) }),
            "on_code_action" => json!({ "actions": code_actions_for(&frame) }),
            other => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("method `{other}` not implemented by fixture"),
                    }
                });
                if write_frame(&mut stdout, &response).is_err() {
                    return ExitCode::from(1);
                }
                continue;
            }
        };

        let response = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        if write_frame(&mut stdout, &response).is_err() {
            return ExitCode::from(1);
        }
    }
}

fn initialize_result(mode: &Mode) -> Value {
    let wire_version = match mode {
        Mode::VersionMismatch { version } => *version,
        _ => 1,
    };
    let implements = match mode {
        Mode::Malformed => vec!["on_validate"],
        _ => vec![
            "on_label",
            "on_validate",
            "on_resolve",
            "on_render",
            "on_hover",
            "on_completion",
            "on_code_action",
        ],
    };
    json!({
        "wire_version": wire_version,
        "implements": implements,
    })
}

/// Echo a single warning that quotes the inbound label, so the test can
/// verify the round-trip carried the LabelCtx through correctly.
fn diagnostics_for(frame: &Value) -> Value {
    let label = frame
        .pointer("/params/label")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let range = frame
        .pointer("/params/node/range")
        .cloned()
        .unwrap_or_else(|| json!({ "start": [0, 0], "end": [0, 0] }));
    json!([{
        "severity": "warning",
        "message": format!("from {label}"),
        "range": range,
    }])
}

fn resolve_for(_frame: &Value) -> Value {
    json!({
        "kind": "paragraph",
        "range": { "start": [0, 0], "end": [0, 0] },
        "inlines": [{ "kind": "text", "text": "spliced" }],
    })
}

fn render_for(frame: &Value) -> Value {
    let label = frame
        .pointer("/params/label")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let format = frame
        .pointer("/params/format")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    json!({
        "kind": "string",
        "string": format!("<rendered {label} as {format}>"),
    })
}

fn hover_for(_frame: &Value) -> Value {
    json!({
        "contents": "fixture hover",
        "format": "plaintext",
    })
}

fn completion_for(_frame: &Value) -> Value {
    json!([{
        "label": "fixture-completion",
        "insert": "fc",
        "kind": "value",
    }])
}

fn code_actions_for(_frame: &Value) -> Value {
    json!([{
        "title": "fixture action",
        "kind": "quickfix",
    }])
}

// ───────────────── framing helpers (LSP-style Content-Length) ─────────────────

fn read_frame(stdin: &mut io::StdinLock) -> io::Result<Option<Value>> {
    // Read header lines until blank.
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = stdin.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().ok();
            }
        }
    }
    let n = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut body = vec![0u8; n];
    stdin.read_exact(&mut body)?;
    let v: Value =
        serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(v))
}

fn write_frame(stdout: &mut io::StdoutLock, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value).expect("serialise");
    write!(stdout, "Content-Length: {}\r\n\r\n", body.len())?;
    stdout.write_all(&body)?;
    stdout.flush()
}

fn parse_arg_u64(args: &[String], name: &str) -> Option<u64> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1)?.parse().ok()
}

fn parse_arg_str(args: &[String], name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).cloned()
}
