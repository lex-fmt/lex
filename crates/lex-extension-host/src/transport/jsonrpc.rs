//! JSON-RPC 2.0 message types and LSP-style framing for the subprocess
//! transport.
//!
//! Transports under `lex-extension-host` use the same wire shape as the
//! Language Server Protocol: each frame is a `Content-Length: N\r\n\r\n`
//! header followed by an N-byte JSON body. This matches the *Lex
//! Extension Wire Format* §1.1 ("LSP-framed JSON-RPC over stdin/stdout").
//!
//! The types here are deliberately minimal — just enough to send and
//! receive request/response/notification frames. Hook payloads
//! (`LabelCtx`, `Diagnostic`, …) live in `lex-extension::wire`; this
//! module only knows about JSON-RPC envelopes.

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 request from host to handler.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OutgoingRequest<'a> {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: &'a str,
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 notification from host to handler. Same shape as
/// [`OutgoingRequest`] minus `id`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OutgoingNotification<'a> {
    pub jsonrpc: &'static str,
    pub method: &'a str,
    pub params: serde_json::Value,
}

/// Inbound frame from the handler. Either a response correlated with one
/// of our request `id`s, or an unsolicited notification (which we log).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum IncomingFrame {
    /// Response. JSON-RPC requires either `result` xor `error`; we
    /// accept whichever is present and report a parse error if both
    /// are missing in the surrounding correlation logic.
    Response {
        #[allow(dead_code)]
        jsonrpc: String,
        id: u64,
        #[serde(default)]
        result: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<JsonRpcError>,
    },
    /// Server-side notification (no `id`). Handlers can use this to
    /// stream log lines or progress; the host logs them and otherwise
    /// ignores them.
    Notification {
        #[allow(dead_code)]
        jsonrpc: String,
        method: String,
        #[serde(default)]
        #[allow(dead_code)]
        params: serde_json::Value,
    },
}

/// A JSON-RPC `error` object. Error codes follow the wire spec §5.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Reserved JSON-RPC error codes used by the host when it produces
/// errors locally. Wire spec §5.
pub(crate) mod codes {
    /// Method not found / unsupported (also used for "format unknown" on
    /// `on_render`).
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Internal error.
    pub const INTERNAL: i32 = -32603;
}

/// Maximum body size accepted from the handler. Defends against a
/// runaway handler that streams gigabytes back at us. 16 MiB is well
/// above any sane diagnostic / hover / render output for one label and
/// well below "we're definitely under attack."
pub(crate) const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// LSP-style header parsing. Consumes header bytes from `buf` and
/// returns the announced body length.
///
/// We accept any number of `Name: value` lines terminated by `\r\n`,
/// followed by a final `\r\n` separator. Only `Content-Length` is
/// required. Unknown headers are tolerated (forward-compat).
///
/// Errors:
/// - missing `Content-Length`
/// - non-integer or negative `Content-Length`
/// - body length exceeds [`MAX_BODY_BYTES`]
pub(crate) fn parse_headers(buf: &str) -> Result<usize, FrameError> {
    let mut content_length: Option<usize> = None;
    for line in buf.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or_else(|| {
            FrameError::MalformedHeader(format!("missing `:` in header line: {line:?}"))
        })?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            let n: usize = value
                .trim()
                .parse()
                .map_err(|_| FrameError::InvalidContentLength(value.trim().to_string()))?;
            content_length = Some(n);
        }
    }
    let n = content_length.ok_or(FrameError::MissingContentLength)?;
    if n > MAX_BODY_BYTES {
        return Err(FrameError::BodyTooLarge(n));
    }
    Ok(n)
}

/// Framing-layer errors. Mapped into `HandlerError::Internal` at the
/// transport boundary so they surface as diagnostics like any other
/// transport failure.
#[derive(Debug)]
pub(crate) enum FrameError {
    MissingContentLength,
    InvalidContentLength(String),
    MalformedHeader(String),
    BodyTooLarge(usize),
    /// Stdout closed mid-frame (handler crashed or shut down).
    UnexpectedEof,
    Io(std::io::Error),
    /// JSON body did not parse as a JSON-RPC frame.
    Json(serde_json::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::MissingContentLength => f.write_str("missing Content-Length header"),
            FrameError::InvalidContentLength(s) => write!(f, "invalid Content-Length value: {s}"),
            FrameError::MalformedHeader(m) => write!(f, "malformed header line: {m}"),
            FrameError::BodyTooLarge(n) => write!(f, "frame body too large: {n} bytes"),
            FrameError::UnexpectedEof => f.write_str("handler closed stdout mid-frame"),
            FrameError::Io(e) => write!(f, "frame io error: {e}"),
            FrameError::Json(e) => write!(f, "frame json error: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<std::io::Error> for FrameError {
    fn from(e: std::io::Error) -> Self {
        FrameError::Io(e)
    }
}

impl From<serde_json::Error> for FrameError {
    fn from(e: serde_json::Error) -> Self {
        FrameError::Json(e)
    }
}

/// Encode a JSON value as an LSP-framed message. Returned bytes are the
/// full frame: header + `\r\n\r\n` + body.
pub(crate) fn encode_frame(value: &serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(value).expect("serialise JSON value");
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = Vec::with_capacity(header.len() + body.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_headers_accepts_minimal_valid_frame() {
        let n = parse_headers("Content-Length: 42\r\n").unwrap();
        assert_eq!(n, 42);
    }

    #[test]
    fn parse_headers_is_case_insensitive_on_name() {
        let n = parse_headers("content-length: 7\r\n").unwrap();
        assert_eq!(n, 7);
    }

    #[test]
    fn parse_headers_tolerates_extra_unknown_headers() {
        let n = parse_headers("Content-Type: application/vscode-jsonrpc\r\nContent-Length: 3\r\n")
            .unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn parse_headers_rejects_missing_content_length() {
        let err = parse_headers("X-Custom: yes\r\n").unwrap_err();
        assert!(matches!(err, FrameError::MissingContentLength));
    }

    #[test]
    fn parse_headers_rejects_invalid_value() {
        let err = parse_headers("Content-Length: NaN\r\n").unwrap_err();
        assert!(matches!(err, FrameError::InvalidContentLength(_)));
    }

    #[test]
    fn parse_headers_rejects_too_large_body() {
        let big = MAX_BODY_BYTES + 1;
        let err = parse_headers(&format!("Content-Length: {big}\r\n")).unwrap_err();
        assert!(matches!(err, FrameError::BodyTooLarge(_)));
    }

    #[test]
    fn encode_frame_emits_header_and_body() {
        let v = serde_json::json!({"a": 1});
        let bytes = encode_frame(&v);
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.starts_with("Content-Length: 7\r\n\r\n"));
        assert!(s.ends_with(r#"{"a":1}"#));
    }

    #[test]
    fn incoming_frame_response_with_result() {
        let body = r#"{"jsonrpc":"2.0","id":3,"result":{"x":1}}"#;
        let f: IncomingFrame = serde_json::from_str(body).unwrap();
        match f {
            IncomingFrame::Response {
                id, result, error, ..
            } => {
                assert_eq!(id, 3);
                assert!(error.is_none());
                assert_eq!(result.unwrap()["x"], serde_json::json!(1));
            }
            _ => panic!("expected response"),
        }
    }

    #[test]
    fn incoming_frame_response_with_error() {
        let body = r#"{"jsonrpc":"2.0","id":4,"error":{"code":-32601,"message":"nope"}}"#;
        let f: IncomingFrame = serde_json::from_str(body).unwrap();
        match f {
            IncomingFrame::Response { id, error, .. } => {
                assert_eq!(id, 4);
                let err = error.unwrap();
                assert_eq!(err.code, -32601);
                assert_eq!(err.message, "nope");
            }
            _ => panic!("expected response"),
        }
    }

    #[test]
    fn incoming_frame_notification_no_id() {
        let body = r#"{"jsonrpc":"2.0","method":"log","params":"hi"}"#;
        let f: IncomingFrame = serde_json::from_str(body).unwrap();
        assert!(matches!(f, IncomingFrame::Notification { method, .. } if method == "log"));
    }
}
