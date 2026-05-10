//! LSP custom request for the extension trust prompt.
//!
//! When the extension boot path encounters a subprocess handler whose
//! trust hasn't been pinned in `<workspace>/.lex/trust.json`, the
//! [`LspPromptHandler`] forwards a `lex/trustRequest` to the LSP
//! client. The client (vscode / nvim / lexed) renders an editor-native
//! prompt and replies with the user's decision; the response is fed
//! back into the trust gate which pins the decision for subsequent
//! sessions.
//!
//! ## Why a request, not a notification
//!
//! `lex/trustRequest` needs a response to drive the gate's decision,
//! so it's an LSP request (server → client → response) rather than a
//! one-way notification. The wire shape is documented in
//! `comms/specs/proposals/extending-lex.lex` §γ.
//!
//! ## Sync / async bridge
//!
//! `TrustPromptHandler::prompt` is sync, but tower-lsp's
//! [`Client::send_request`] is async. The boot path in
//! [`crate::server::LexLanguageServer::extension_state`] wraps the
//! whole boot in `tokio::task::spawn_blocking`, so the prompt runs on
//! a tokio blocking-pool thread — which means we can call
//! [`tokio::runtime::Handle::block_on`] without blocking the runtime.
//! The handle is captured at boot time and held by the prompt handler.

use lex_extension_host::{
    Capability as TrustCapability, Source as TrustSource, Transport as TrustTransport,
    TrustDecision, TrustPromptContext, TrustPromptHandler,
};
use serde::{Deserialize, Serialize};
use tower_lsp::async_trait;
use tower_lsp::jsonrpc::Result as JsonRpcResult;
use tower_lsp::lsp_types::request::Request;

/// `lex/trustRequest` — server asks the client to render a trust prompt
/// for a subprocess handler that hasn't been pinned in the workspace
/// trust store.
pub enum LexTrustRequest {}
impl Request for LexTrustRequest {
    type Params = TrustRequestParams;
    type Result = TrustResponse;
    const METHOD: &'static str = "lex/trustRequest";
}

/// Parameters of a `lex/trustRequest`.
///
/// Mirrors [`TrustPromptContext`] one-to-one but with serializable wire
/// types — `source`, `capability`, and `transport` map onto string
/// constants (`"lex_toml"`, `"local_file"`, `"subprocess"`, …) so editor
/// implementations don't need to mirror the Rust enum hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustRequestParams {
    /// Namespace name (e.g. `"acme"`).
    pub namespace: String,
    /// Joined `handler.command` from the schema YAML — exactly the
    /// string the gate keys on for trust pinning.
    pub command_string: String,
    /// Where the schema came from. One of:
    /// - `{ "kind": "lex_toml", "name": "<namespace>" }` — declared in
    ///   `lex.toml`'s `[labels]` block.
    /// - `{ "kind": "local_file", "path": "<path>" }` — a local schema
    ///   directory the host opted into.
    /// - `{ "kind": "cache_only", "uri": "<uri>" }` — schema fetched
    ///   from a marketplace / registry / cache without an explicit
    ///   user gesture. Higher trust bar.
    pub source: TrustRequestSource,
    /// Declared capability set the handler asked for. Forward-compatible
    /// string-shaped enum:
    /// - `"pure"` — handler declared `fs: false, net: false`. Will be
    ///   eligible for sandbox-enforced auto-trust once PR 12 lands.
    /// - `"full"` — handler asked for `fs` or `net` access. Always
    ///   prompts (no sandbox can enforce this in v1).
    ///
    /// Future values (`"fs_read"`, `"fs_write"`, etc.) are
    /// non-breaking; editors should render unknown values as
    /// "unknown capability" and treat them at least as cautiously as
    /// `"full"`.
    pub capability: String,
    /// Transport: always `"subprocess"` in v1 (native handlers don't
    /// prompt; WASM is deferred to PR 12).
    pub transport: String,
}

/// `source` field shape on the wire. Internally tagged on `kind`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrustRequestSource {
    LexToml { name: String },
    LocalFile { path: String },
    CacheOnly { uri: String },
}

/// Response shape for `lex/trustRequest`.
///
/// `decision` is `"trusted"` or `"denied"` — string-shaped so future
/// values (e.g. `"trusted_once"`, `"trusted_for_session"`) are
/// non-breaking. Unknown values fall back to `"denied"` on the host
/// side. `reason` is optional and is surfaced as the diagnostic message
/// when `decision == "denied"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TrustResponse {
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Trait for sending `lex/trustRequest` to an LSP client. Mockable in
/// tests; the real impl forwards through tower-lsp's
/// [`Client::send_request`].
#[async_trait]
pub trait LspTrustRequester: Send + Sync + 'static {
    async fn send_trust_request(&self, params: TrustRequestParams) -> JsonRpcResult<TrustResponse>;
}

#[async_trait]
impl LspTrustRequester for tower_lsp::Client {
    async fn send_trust_request(&self, params: TrustRequestParams) -> JsonRpcResult<TrustResponse> {
        self.send_request::<LexTrustRequest>(params).await
    }
}

/// Convert a [`TrustPromptContext`] into the request payload. The
/// gate's enums map onto the string-shaped wire constants documented
/// above.
fn params_from_ctx(ctx: &TrustPromptContext) -> TrustRequestParams {
    let source = match &ctx.source {
        TrustSource::LexTomlNamespace { name } => {
            TrustRequestSource::LexToml { name: name.clone() }
        }
        TrustSource::LocalFile { path } => TrustRequestSource::LocalFile {
            path: path.display().to_string(),
        },
        TrustSource::CacheOnly { uri } => TrustRequestSource::CacheOnly { uri: uri.clone() },
    };
    let capability = match ctx.capability {
        TrustCapability::Pure => "pure",
        TrustCapability::Full => "full",
    }
    .to_string();
    let transport = "subprocess".to_string();
    TrustRequestParams {
        namespace: ctx.namespace.clone(),
        command_string: ctx.command_string.clone(),
        source,
        capability,
        transport,
    }
}

/// Maximum time we wait for the editor to reply to a `lex/trustRequest`.
///
/// Tradeoff: long enough that a distracted user has time to read the
/// prompt and decide; short enough that a buggy or quietly-dismissed
/// extension can't pin the boot indefinitely. 60s is the "send a
/// message in Slack and come back" budget — past this, treating it as
/// denied is safer than hanging the boot.
const PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// [`TrustPromptHandler`] implementation that forwards to an LSP
/// client. Sync→async bridge runs on the boot's blocking thread via
/// [`tokio::runtime::Handle::block_on`].
pub struct LspPromptHandler<R: LspTrustRequester> {
    requester: std::sync::Arc<R>,
    runtime_handle: tokio::runtime::Handle,
    timeout: std::time::Duration,
}

impl<R: LspTrustRequester> LspPromptHandler<R> {
    pub fn new(requester: std::sync::Arc<R>, runtime_handle: tokio::runtime::Handle) -> Self {
        Self {
            requester,
            runtime_handle,
            timeout: PROMPT_TIMEOUT,
        }
    }

    /// Construct with a custom timeout. Tests use this to drive the
    /// timeout-as-denied path without waiting 60 seconds.
    #[cfg(test)]
    pub fn with_timeout(
        requester: std::sync::Arc<R>,
        runtime_handle: tokio::runtime::Handle,
        timeout: std::time::Duration,
    ) -> Self {
        Self {
            requester,
            runtime_handle,
            timeout,
        }
    }
}

impl<R: LspTrustRequester> TrustPromptHandler for LspPromptHandler<R> {
    fn prompt(&self, ctx: &TrustPromptContext) -> TrustDecision {
        let params = params_from_ctx(ctx);
        let requester = std::sync::Arc::clone(&self.requester);
        let timeout = self.timeout;
        // We're called from a `spawn_blocking` thread (the boot path
        // wraps `boot_registry`), so `block_on` is safe — it parks
        // this blocking-pool thread, not the async runtime's worker
        // threads.
        //
        // Wrap the request in `tokio::time::timeout` so a buggy
        // extension or a prompt the user dismisses without a reply
        // can't hang the boot indefinitely. On timeout we deny with a
        // diagnostic, matching the other failure modes.
        let response = self.runtime_handle.block_on(async move {
            tokio::time::timeout(timeout, requester.send_trust_request(params)).await
        });
        match response {
            Ok(Ok(resp)) => match resp.decision.as_str() {
                "trusted" => TrustDecision::Trusted,
                _ => {
                    // Anything other than "trusted" — including unknown
                    // future values — falls back to denied. The reason
                    // surfaces as the boot diagnostic.
                    TrustDecision::Denied {
                        reason: resp.reason.unwrap_or_else(|| {
                            format!(
                                "subprocess handler `{}` was not trusted by the editor",
                                ctx.namespace
                            )
                        }),
                    }
                }
            },
            Ok(Err(e)) => TrustDecision::Denied {
                reason: format!(
                    "subprocess handler `{}` denied: trust request to editor failed ({e})",
                    ctx.namespace
                ),
            },
            Err(_elapsed) => TrustDecision::Denied {
                reason: format!(
                    "subprocess handler `{}` denied: trust request to editor timed out after {}s",
                    ctx.namespace,
                    timeout.as_secs()
                ),
            },
        }
    }
}

/// `Transport` wire-string mapping. Kept as a free function so the
/// const can be reused by tests / future request-shaped transports
/// (WASM, etc.).
#[allow(dead_code)]
pub(crate) fn transport_string(t: TrustTransport) -> &'static str {
    match t {
        TrustTransport::Native => "native",
        TrustTransport::Subprocess => "subprocess",
        TrustTransport::Wasm => "wasm",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex;

    /// Test requester that captures the params it received and returns
    /// a canned response.
    struct MockRequester {
        captured: Mutex<Vec<TrustRequestParams>>,
        response: Mutex<JsonRpcResult<TrustResponse>>,
        call_count: AtomicUsize,
    }

    impl MockRequester {
        fn new(response: TrustResponse) -> Self {
            Self {
                captured: Mutex::new(Vec::new()),
                response: Mutex::new(Ok(response)),
                call_count: AtomicUsize::new(0),
            }
        }

        fn with_error() -> Self {
            Self {
                captured: Mutex::new(Vec::new()),
                response: Mutex::new(Err(tower_lsp::jsonrpc::Error::internal_error())),
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl LspTrustRequester for MockRequester {
        async fn send_trust_request(
            &self,
            params: TrustRequestParams,
        ) -> JsonRpcResult<TrustResponse> {
            self.captured.lock().await.push(params);
            self.call_count.fetch_add(1, Ordering::SeqCst);
            // Clone-or-clone-error-shape via match.
            let r = self.response.lock().await;
            match &*r {
                Ok(resp) => Ok(resp.clone()),
                Err(e) => Err(e.clone()),
            }
        }
    }

    fn ctx() -> TrustPromptContext {
        TrustPromptContext {
            namespace: "acme".into(),
            command_string: "/usr/local/bin/acme-handler".into(),
            source: TrustSource::LexTomlNamespace {
                name: "acme".into(),
            },
            capability: TrustCapability::Full,
        }
    }

    #[test]
    fn params_round_trip_through_serde() {
        let p = TrustRequestParams {
            namespace: "acme".into(),
            command_string: "acme-bin".into(),
            source: TrustRequestSource::LexToml {
                name: "acme".into(),
            },
            capability: "full".into(),
            transport: "subprocess".into(),
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: TrustRequestParams = serde_json::from_str(&s).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn params_from_ctx_local_file_source() {
        let mut c = ctx();
        c.source = TrustSource::LocalFile {
            path: PathBuf::from("/tmp/schemas/acme"),
        };
        let p = params_from_ctx(&c);
        match p.source {
            TrustRequestSource::LocalFile { path } => {
                assert!(path.contains("acme"));
            }
            _ => panic!("expected LocalFile"),
        }
    }

    /// Trusted response from the editor → the prompt returns
    /// `TrustDecision::Trusted` and the editor saw a single request.
    #[test]
    fn prompt_handler_translates_trusted_response() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let requester = std::sync::Arc::new(MockRequester::new(TrustResponse {
            decision: "trusted".into(),
            reason: None,
        }));
        let handler = LspPromptHandler::new(std::sync::Arc::clone(&requester), rt.handle().clone());

        // The prompt method calls block_on internally — drive it from
        // the runtime's spawn_blocking pool (matches production flow).
        let decision = rt.block_on(async {
            tokio::task::spawn_blocking(move || handler.prompt(&ctx()))
                .await
                .unwrap()
        });

        assert!(matches!(decision, TrustDecision::Trusted));
        assert_eq!(requester.call_count.load(Ordering::SeqCst), 1);
    }

    /// Denied response with a reason → reason surfaces in the
    /// `TrustDecision::Denied` diagnostic.
    #[test]
    fn prompt_handler_surfaces_denied_reason() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let requester = std::sync::Arc::new(MockRequester::new(TrustResponse {
            decision: "denied".into(),
            reason: Some("user-clicked-deny".into()),
        }));
        let handler = LspPromptHandler::new(std::sync::Arc::clone(&requester), rt.handle().clone());
        let decision = rt.block_on(async {
            tokio::task::spawn_blocking(move || handler.prompt(&ctx()))
                .await
                .unwrap()
        });
        match decision {
            TrustDecision::Denied { reason } => {
                assert!(reason.contains("user-clicked-deny"), "got: {reason}");
            }
            other => panic!("expected Denied, got {other:?}"),
        }
    }

    /// Unknown decision string → fall back to denied (forward
    /// compatibility — future values are non-breaking).
    #[test]
    fn prompt_handler_treats_unknown_decision_as_denied() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let requester = std::sync::Arc::new(MockRequester::new(TrustResponse {
            decision: "trusted_once".into(),
            reason: None,
        }));
        let handler = LspPromptHandler::new(std::sync::Arc::clone(&requester), rt.handle().clone());
        let decision = rt.block_on(async {
            tokio::task::spawn_blocking(move || handler.prompt(&ctx()))
                .await
                .unwrap()
        });
        assert!(matches!(decision, TrustDecision::Denied { .. }));
    }

    /// Requester that never resolves — simulates a buggy editor that
    /// receives the request and never replies. The handler must
    /// time out and treat that as denied so the boot doesn't hang.
    struct StuckRequester;

    #[async_trait]
    impl LspTrustRequester for StuckRequester {
        async fn send_trust_request(&self, _: TrustRequestParams) -> JsonRpcResult<TrustResponse> {
            // Never resolves on its own; the prompt's tokio::time::timeout
            // is the only way out.
            std::future::pending().await
        }
    }

    /// Buggy / non-responsive editor → timeout fires → denied with a
    /// "timed out" diagnostic. Critical: a stuck prompt must NOT pin
    /// the boot indefinitely.
    #[test]
    fn prompt_handler_times_out_when_editor_never_responds() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let requester = std::sync::Arc::new(StuckRequester);
        let handler = LspPromptHandler::with_timeout(
            std::sync::Arc::clone(&requester),
            rt.handle().clone(),
            std::time::Duration::from_millis(50),
        );
        let decision = rt.block_on(async {
            tokio::task::spawn_blocking(move || handler.prompt(&ctx()))
                .await
                .unwrap()
        });
        match decision {
            TrustDecision::Denied { reason } => {
                assert!(
                    reason.contains("timed out"),
                    "expected 'timed out' diagnostic, got: {reason}"
                );
            }
            other => panic!("expected Denied (timeout), got {other:?}"),
        }
    }

    /// Editor-side error (transport, disconnect, etc.) → denied with a
    /// "trust request failed" diagnostic. Doesn't crash the boot.
    #[test]
    fn prompt_handler_surfaces_request_error_as_denied() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let requester = std::sync::Arc::new(MockRequester::with_error());
        let handler = LspPromptHandler::new(std::sync::Arc::clone(&requester), rt.handle().clone());
        let decision = rt.block_on(async {
            tokio::task::spawn_blocking(move || handler.prompt(&ctx()))
                .await
                .unwrap()
        });
        match decision {
            TrustDecision::Denied { reason } => {
                assert!(reason.contains("trust request"), "got: {reason}");
            }
            other => panic!("expected Denied, got {other:?}"),
        }
    }
}
