//! Subprocess transport: spawn a handler binary, talk to it over
//! LSP-framed JSON-RPC on stdin/stdout, present the same
//! [`LexHandler`](lex_extension::LexHandler) trait as native handlers.
//!
//! # Architecture
//!
//! `SubprocessHandler` looks synchronous to its caller — `LexHandler` is
//! a sync trait — but everything inside is driven by a dedicated worker
//! thread that owns a single-threaded tokio runtime. The split is what
//! lets us keep the public dispatch surface sync (no async traits leaking
//! through `Registry::dispatch_*`) while the I/O layer gets tokio's
//! timers, async I/O, and structured cancellation for free.
//!
//! ```text
//!     sync caller                     worker thread
//!         |                                |
//!     on_validate(ctx) ---send-cmd-->  cmd queue (tokio::mpsc)
//!         |                                |  select! on:
//!         |                                |    - new cmd
//!     blocking_recv()                      |    - child stdout frames
//!         ^                                |    - shutdown
//!         |                                |
//!         +------ reply (std mpsc) <-------+
//! ```
//!
//! - One outstanding request per `SubprocessHandler` is plenty: the
//!   analysis pass dispatches sequentially per label. We don't gain
//!   throughput by allowing concurrent calls into the same handler; we
//!   would gain race conditions.
//! - Replies flow through `std::sync::mpsc` so the sync caller can
//!   `recv_timeout` without wiring tokio into its frame.
//!
//! # What this PR does *not* gate
//!
//! Per the master tracking issue (correction #1), the trust gate decides
//! whether subprocess handlers run at all. This module provides the
//! mechanism; the gate (PR 6) decides when to construct one.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use lex_extension::{
    schema::HandlerSpec, CodeAction, Completion, Diagnostic, Format, HandlerError, Hover, LabelCtx,
    LexHandler, RenderOut, WireNode,
};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use super::jsonrpc::{
    codes, encode_frame, parse_headers, FrameError, IncomingFrame, JsonRpcError,
    OutgoingNotification, OutgoingRequest,
};

/// Default per-request timeout when the schema doesn't override it.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(2000);

/// Initialize-handshake timeout: a handler that can't say hello inside
/// 5 s is broken. Independent of per-request timeout because handshake
/// is a one-shot at spawn time.
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(5);

/// Grace period to let a handler exit after we send the `shutdown`
/// notification. After this elapses we let the writer task finish,
/// drop the runtime, and rely on `kill_on_drop(true)` on the
/// `tokio::process::Child` to SIGKILL the process. (The previous doc
/// claimed an explicit SIGTERM step; the implementation has always
/// relied on `kill_on_drop` for the kill phase.)
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

/// Workspace + cache context used to expand `${WORKSPACE_ROOT}`,
/// `${LEX_CACHE}`, and `${HANDLER_CONFIG}` inside the schema's `command`
/// array.
///
/// `handler_config` is a per-namespace string the host can pass through
/// (typically a path to a config file the handler knows how to read).
/// Hosts construct this once and clone it into each spawn call.
#[derive(Debug, Clone, Default)]
pub struct SpawnEnv {
    pub workspace_root: Option<String>,
    pub lex_cache: Option<String>,
    pub handler_config: Option<String>,
}

impl SpawnEnv {
    /// Substitute `${NAME}` references in one argv entry. Unknown names
    /// produce a [`SpawnError::UnknownVariable`] error so a typo in a
    /// schema gets caught at spawn time, not as silent empty-string
    /// substitution at runtime.
    fn expand(&self, raw: &str) -> Result<String, SpawnError> {
        // We accept `${NAME}`; bare `$NAME` is intentionally not expanded
        // (matches the wire spec's literal-string convention).
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after_brace = &rest[start + 2..];
            let close = after_brace
                .find('}')
                .ok_or_else(|| SpawnError::UnclosedVariable {
                    fragment: raw.to_string(),
                })?;
            let name = &after_brace[..close];
            let value = match name {
                "WORKSPACE_ROOT" => self.workspace_root.as_deref(),
                "LEX_CACHE" => self.lex_cache.as_deref(),
                "HANDLER_CONFIG" => self.handler_config.as_deref(),
                other => {
                    return Err(SpawnError::UnknownVariable {
                        name: other.to_string(),
                    });
                }
            };
            // A known variable that wasn't supplied substitutes empty;
            // matches POSIX `${VAR}` for unset-but-declared variables.
            // The `UnknownVariable` error is reserved for *names the
            // host doesn't recognise*.
            out.push_str(value.unwrap_or(""));
            rest = &after_brace[close + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }
}

/// Errors raised at handler spawn time, before any JSON-RPC traffic.
/// Once a handler is alive, transport-level failures fold into
/// `HandlerError::Internal` instead.
#[derive(Debug)]
pub enum SpawnError {
    /// `command` array was empty (caller bypassed the schema validator).
    EmptyCommand,
    /// Schema referenced `${NAME}` for a name the host doesn't supply.
    UnknownVariable { name: String },
    /// Schema referenced `${NAME` (no closing brace).
    UnclosedVariable { fragment: String },
    /// `Command::spawn` failed (binary not found, EACCES, …).
    Spawn(std::io::Error),
    /// The child closed stdin/stdout before completing the
    /// `initialize` handshake.
    ChildStreamMissing,
    /// The OS-level [`Sandbox`] couldn't install its policy on the
    /// command (e.g., the requested capability isn't enforceable on
    /// this platform, or a kernel call failed). The child is never
    /// spawned in this case.
    ///
    /// Current handling in [`lex_engine::setup::boot_registry`]
    /// (and any other caller treating spawn failure as terminal):
    /// the namespace registers schema-only — pre-validation still
    /// catches typos but no handler runs — and a `BootDiagnostic`
    /// surfaces the reason. Same shape as other [`SpawnError`]
    /// variants today. Future revisions (likely landing alongside
    /// PR 12d's matrix flip) may add a retry-with-prompt path so a
    /// sandbox install failure on a pure handler can downgrade to
    /// the prompt-then-pin track instead of registering schema-
    /// only on the first run; until then, sandbox failures behave
    /// identically to other spawn failures.
    ///
    /// [`Sandbox`]: crate::sandbox::Sandbox
    /// [`lex_engine::setup::boot_registry`]: https://docs.rs/lex-engine/latest/lex_engine/setup/fn.boot_registry.html
    Sandbox(String),
    /// Initialize timed out, errored, or returned an incompatible
    /// `wire_version`.
    Initialize(InitializeError),
}

#[derive(Debug)]
pub enum InitializeError {
    Timeout,
    Transport(String),
    /// Handler responded to `initialize` with a JSON-RPC `error` object.
    /// `code` and `message` are the JSON-RPC fields, surfaced as a
    /// flattened pair to keep `JsonRpcError` private to the crate.
    HandlerError {
        code: i32,
        message: String,
    },
    /// Handler reported a `wire_version` we don't understand. Currently
    /// the host speaks exactly `WIRE_VERSION = 1`.
    VersionMismatch {
        handler: u32,
        host: u32,
    },
    BadResponse(String),
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpawnError::EmptyCommand => f.write_str("subprocess handler: empty command array"),
            SpawnError::UnknownVariable { name } => write!(
                f,
                "subprocess handler: unknown environment variable `${{{name}}}` in command array"
            ),
            SpawnError::UnclosedVariable { fragment } => write!(
                f,
                "subprocess handler: unclosed `${{...}}` in command array entry: {fragment:?}"
            ),
            SpawnError::Spawn(e) => write!(f, "subprocess handler: failed to spawn child: {e}"),
            SpawnError::ChildStreamMissing => {
                f.write_str("subprocess handler: child closed stdio before initialize")
            }
            SpawnError::Sandbox(message) => {
                write!(
                    f,
                    "subprocess handler: sandbox policy install failed: {message}"
                )
            }
            SpawnError::Initialize(InitializeError::Timeout) => {
                f.write_str("subprocess handler: initialize handshake timed out")
            }
            SpawnError::Initialize(InitializeError::Transport(m)) => {
                write!(
                    f,
                    "subprocess handler: transport error during initialize: {m}"
                )
            }
            SpawnError::Initialize(InitializeError::HandlerError { code, message }) => write!(
                f,
                "subprocess handler: initialize returned error {code}: {message}",
            ),
            SpawnError::Initialize(InitializeError::VersionMismatch { handler, host }) => write!(
                f,
                "subprocess handler: wire_version mismatch (handler={handler}, host={host})"
            ),
            SpawnError::Initialize(InitializeError::BadResponse(m)) => {
                write!(f, "subprocess handler: malformed initialize response: {m}")
            }
        }
    }
}

impl std::error::Error for SpawnError {}

/// Initialize handshake parameters sent to the handler.
#[derive(serde::Serialize)]
struct InitializeParams<'a> {
    wire_version: u32,
    lex_version: &'a str,
    namespace: &'a str,
    labels: &'a [String],
    capabilities: lex_extension::schema::Capabilities,
    workspace: Option<&'a str>,
}

/// Initialize handshake result returned by the handler.
#[derive(Deserialize, Debug)]
struct InitializeResult {
    wire_version: u32,
    #[serde(default)]
    implements: Vec<String>,
}

/// One outstanding request waiting on the worker thread.
struct PendingReply {
    tx: std::sync::mpsc::Sender<Result<serde_json::Value, JsonRpcError>>,
}

/// Commands the sync side sends to the worker thread.
enum WorkerCmd {
    Call {
        /// Pre-allocated by the sync side so the timeout path can
        /// follow up with [`WorkerCmd::CancelPending`] for the same id.
        id: u64,
        method: &'static str,
        params: serde_json::Value,
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, JsonRpcError>>,
    },
    /// Fire-and-forget JSON-RPC notification. No `id`, no waiter; the
    /// worker writes the frame and moves on.
    Notify {
        method: &'static str,
        params: serde_json::Value,
    },
    /// The sync side timed out on a pending request. Tell the worker
    /// to drop the entry preemptively so `pending` doesn't accumulate
    /// when a handler is permanently slow.
    CancelPending {
        id: u64,
    },
    Shutdown,
}

/// A `LexHandler` backed by an external process.
#[derive(Debug)]
pub struct SubprocessHandler {
    cmd_tx: mpsc::UnboundedSender<WorkerCmd>,
    timeout: Duration,
    /// JSON-RPC id allocator. Reserves [1..100) for handshake / future
    /// housekeeping; live calls start at 100.
    next_id: AtomicU64,
    /// `implements` array reported by the handler at initialize. Used
    /// to short-circuit dispatch for hooks the handler doesn't claim
    /// to support.
    implements: std::collections::HashSet<String>,
    /// Sticky failure flag. Set on transport-fatal errors (channel
    /// closed, framing error). Once set, all further calls return
    /// `HandlerError::Internal` immediately.
    disabled: Arc<AtomicBool>,
    /// Holds the worker thread handle so [`Drop`] can join it after
    /// telling it to shut down.
    worker: Option<thread::JoinHandle<()>>,
}

impl SubprocessHandler {
    /// Spawn a handler subprocess and run the `initialize` handshake.
    ///
    /// `spec` is the schema's `handler` block; `namespace` and `labels`
    /// are passed verbatim into the initialize params; `lex_version` is
    /// the host's `lex` crate version (used by handlers that want to
    /// report which host they're talking to). `env` provides values
    /// for the `${WORKSPACE_ROOT}` / `${LEX_CACHE}` / `${HANDLER_CONFIG}`
    /// expansions inside `spec.command`.
    ///
    /// The child process runs without OS-level sandboxing. To enforce
    /// declared capabilities at the kernel level (declared-pure
    /// handlers under post-δ trust-matrix auto-trust), use
    /// [`Self::spawn_with_sandbox`].
    pub fn spawn(
        spec: &HandlerSpec,
        namespace: &str,
        labels: &[String],
        capabilities: lex_extension::schema::Capabilities,
        lex_version: &str,
        env: &SpawnEnv,
    ) -> Result<Self, SpawnError> {
        Self::spawn_with_sandbox(
            spec,
            namespace,
            labels,
            capabilities,
            lex_version,
            env,
            Box::new(crate::sandbox::NullSandbox),
        )
    }

    /// Spawn a handler subprocess under the supplied OS-level
    /// [`Sandbox`], then run the `initialize` handshake. The sandbox's
    /// [`Sandbox::apply_to`] is called against the [`Command`] before
    /// `spawn()`, so the kernel enforces the declared capability set
    /// on the child from the first instruction.
    ///
    /// `sandbox` carries the per-OS enforcement mechanism (Linux
    /// seccomp+landlock via `birdcage`, macOS sandbox-exec, Windows
    /// Job Objects + restricted tokens). For the plumbing PR
    /// ([lex-fmt/lex#528](https://github.com/lex-fmt/lex/issues/528))
    /// the only available impl is [`crate::sandbox::NullSandbox`],
    /// which behaves identically to [`Self::spawn`].
    ///
    /// [`Sandbox`]: crate::sandbox::Sandbox
    /// [`Sandbox::apply_to`]: crate::sandbox::Sandbox::apply_to
    pub fn spawn_with_sandbox(
        spec: &HandlerSpec,
        namespace: &str,
        labels: &[String],
        capabilities: lex_extension::schema::Capabilities,
        lex_version: &str,
        env: &SpawnEnv,
        sandbox: Box<dyn crate::sandbox::Sandbox>,
    ) -> Result<Self, SpawnError> {
        if spec.command.is_empty() {
            return Err(SpawnError::EmptyCommand);
        }
        // Expand variables once on the host side. Resulting argv goes
        // straight to the kernel; the child process does not see the
        // unexpanded form.
        let expanded: Vec<String> = spec
            .command
            .iter()
            .map(|raw| env.expand(raw))
            .collect::<Result<_, _>>()?;

        let timeout = spec
            .timeout_ms
            .map(|ms| Duration::from_millis(ms as u64))
            .unwrap_or(DEFAULT_TIMEOUT);

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<WorkerCmd>();
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<InitializeResult, SpawnError>>();
        let disabled = Arc::new(AtomicBool::new(false));
        let disabled_for_worker = disabled.clone();

        // Snapshot the handshake inputs so they can move into the
        // worker thread without lifetime ties to `&self`.
        let init_params = serde_json::to_value(InitializeParams {
            wire_version: lex_extension::WIRE_VERSION,
            lex_version,
            namespace,
            labels,
            capabilities,
            workspace: env.workspace_root.as_deref(),
        })
        .expect("InitializeParams serialises");

        let worker_sandbox = sandbox;
        let worker_caps = capabilities;
        let worker = thread::spawn(move || {
            run_worker(
                expanded,
                init_params,
                init_tx,
                cmd_rx,
                disabled_for_worker,
                worker_sandbox,
                worker_caps,
            );
        });

        let init = init_rx
            .recv_timeout(INITIALIZE_TIMEOUT)
            .map_err(|_| SpawnError::Initialize(InitializeError::Timeout))??;

        if init.wire_version != lex_extension::WIRE_VERSION {
            // Tell the worker to shut down so the child doesn't linger.
            let _ = cmd_tx.send(WorkerCmd::Shutdown);
            let _ = worker.join();
            return Err(SpawnError::Initialize(InitializeError::VersionMismatch {
                handler: init.wire_version,
                host: lex_extension::WIRE_VERSION,
            }));
        }

        Ok(Self {
            cmd_tx,
            timeout,
            next_id: AtomicU64::new(100),
            implements: init.implements.into_iter().collect(),
            disabled,
            worker: Some(worker),
        })
    }

    /// True if the handler advertised this method in its `implements`
    /// array. Hosts can use this to skip dispatch for hooks the handler
    /// doesn't claim to support, avoiding a round-trip-then-`-32601`.
    pub fn implements(&self, method: &str) -> bool {
        self.implements.contains(method)
    }

    /// Send a JSON-RPC request and block (up to `self.timeout`) on the
    /// reply.
    ///
    /// Callers are responsible for checking `self.implements` first when
    /// the trait method has a "no result" default. `call()` itself does
    /// not second-guess: short-circuiting here would surface as a
    /// spurious `Unsupported` diagnostic for hooks like `on_hover` whose
    /// natural default is `Ok(None)`.
    fn call(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, HandlerError> {
        if self.disabled.load(Ordering::SeqCst) {
            return Err(HandlerError::internal("subprocess handler disabled"));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::channel();
        self.cmd_tx
            .send(WorkerCmd::Call {
                id,
                method,
                params,
                reply: tx,
            })
            .map_err(|_| {
                self.disabled.store(true, Ordering::SeqCst);
                HandlerError::internal("subprocess handler worker has stopped")
            })?;
        match rx.recv_timeout(self.timeout) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(err)) => Err(handler_error_from_jsonrpc(err)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Tell the worker to drop the pending entry so a
                // permanently-slow handler can't accumulate stale
                // pending requests in `pending`. Send is best-effort —
                // if the worker is gone, our shutdown ladder will
                // collect the leak.
                let _ = self.cmd_tx.send(WorkerCmd::CancelPending { id });
                Err(HandlerError::internal(format!(
                    "subprocess handler timed out after {} ms on `{method}`",
                    self.timeout.as_millis()
                )))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.disabled.store(true, Ordering::SeqCst);
                Err(HandlerError::internal(
                    "subprocess handler worker dropped reply channel",
                ))
            }
        }
    }
}

impl Drop for SubprocessHandler {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(WorkerCmd::Shutdown);
        if let Some(handle) = self.worker.take() {
            // Bounded join: a healthy worker exits within milliseconds
            // of receiving Shutdown (it sends the JSON-RPC `shutdown`
            // notification, waits SHUTDOWN_GRACE for the child, then
            // returns; `kill_on_drop(true)` on the child handles the
            // SIGKILL ladder). If it's wedged in IO we don't want to
            // block Drop forever, so we poll `is_finished` for a short
            // window and detach the handle if it doesn't return.
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !handle.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
            // else: detach. The OS will reap the thread when its
            // resources finally release; we'd rather leak a thread
            // than block our caller.
        }
    }
}

/// Map a JSON-RPC error response onto a `HandlerError`. Reserved codes
/// surface as the typed `Unsupported`/`Internal` variants so the
/// registry's diagnostic-mapping layer sees the same shape it does for
/// native handlers.
fn handler_error_from_jsonrpc(err: JsonRpcError) -> HandlerError {
    match err.code {
        codes::METHOD_NOT_FOUND => HandlerError::Unsupported {
            detail: err.message,
        },
        codes::INTERNAL => HandlerError::Internal {
            message: err.message,
        },
        code if (-32099..=-32000).contains(&code) => HandlerError::Custom {
            code,
            message: err.message,
            data: err.data,
        },
        // Any other reserved code (parse error, invalid request, …)
        // is upstream's bug; surface as Internal so the user gets a
        // diagnostic instead of silent drop.
        _ => HandlerError::Internal {
            message: format!(
                "handler returned reserved error code {}: {}",
                err.code, err.message
            ),
        },
    }
}

// ────────────────────────── LexHandler bridge ──────────────────────────

impl SubprocessHandler {
    /// True when the handler advertised this method in its
    /// `implements` array, OR it advertised an empty array (treat as
    /// "didn't tell us — try and see"). False short-circuits the
    /// trait method to its identity default — `Ok(None)` /
    /// `Ok(Vec::new())` / `()` — so unimplemented hooks don't
    /// surface as `Unsupported` diagnostics, matching the trait's
    /// own default semantics for native handlers.
    fn advertised(&self, method: &str) -> bool {
        self.implements.is_empty() || self.implements.contains(method)
    }
}

impl LexHandler for SubprocessHandler {
    fn on_label(&self, ctx: &LabelCtx) {
        // Wire spec §4.1: on_label is a JSON-RPC notification — no `id`,
        // no response. Send via the Notify command so the worker writes
        // the frame and moves on without registering a pending entry.
        if self.disabled.load(Ordering::SeqCst) || !self.advertised("on_label") {
            return;
        }
        let _ = self.cmd_tx.send(WorkerCmd::Notify {
            method: "on_label",
            params: serde_json::to_value(ctx).expect("LabelCtx"),
        });
    }

    fn on_validate(&self, ctx: &LabelCtx) -> Result<Vec<Diagnostic>, HandlerError> {
        if !self.advertised("on_validate") {
            return Ok(Vec::new());
        }
        #[derive(Deserialize)]
        struct ValidateResult {
            #[serde(default)]
            diagnostics: Vec<Diagnostic>,
        }
        let v = self.call("on_validate", serde_json::to_value(ctx).expect("LabelCtx"))?;
        let result: ValidateResult = serde_json::from_value(v)
            .map_err(|e| HandlerError::internal(format!("on_validate response decode: {e}")))?;
        Ok(result.diagnostics)
    }

    fn on_resolve(&self, ctx: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
        if !self.advertised("on_resolve") {
            return Ok(None);
        }
        #[derive(Deserialize)]
        struct ResolveResult {
            #[serde(default)]
            replacement: Option<WireNode>,
        }
        let v = self.call("on_resolve", serde_json::to_value(ctx).expect("LabelCtx"))?;
        let result: ResolveResult = serde_json::from_value(v)
            .map_err(|e| HandlerError::internal(format!("on_resolve response decode: {e}")))?;
        Ok(result.replacement)
    }

    fn on_render(&self, ctx: &LabelCtx, format: Format) -> Result<Option<RenderOut>, HandlerError> {
        if !self.advertised("on_render") {
            return Ok(None);
        }
        #[derive(Deserialize)]
        struct RenderResult {
            #[serde(default)]
            output: Option<RenderOut>,
        }
        // Wire spec §4.4: render params extend LabelCtx with `format` +
        // `format_options`.
        let mut params = serde_json::to_value(ctx).expect("LabelCtx");
        let obj = params
            .as_object_mut()
            .expect("LabelCtx serialises as object");
        obj.insert(
            "format".into(),
            serde_json::to_value(&format).expect("Format"),
        );
        obj.insert(
            "format_options".into(),
            serde_json::Value::Object(Default::default()),
        );
        let v = self.call("on_render", params)?;
        let result: RenderResult = serde_json::from_value(v)
            .map_err(|e| HandlerError::internal(format!("on_render response decode: {e}")))?;
        Ok(result.output)
    }

    fn on_hover(&self, ctx: &LabelCtx) -> Result<Option<Hover>, HandlerError> {
        if !self.advertised("on_hover") {
            return Ok(None);
        }
        #[derive(Deserialize)]
        struct HoverResult {
            #[serde(default)]
            hover: Option<Hover>,
        }
        let v = self.call("on_hover", serde_json::to_value(ctx).expect("LabelCtx"))?;
        let result: HoverResult = serde_json::from_value(v)
            .map_err(|e| HandlerError::internal(format!("on_hover response decode: {e}")))?;
        Ok(result.hover)
    }

    fn on_completion(&self, ctx: &LabelCtx) -> Result<Vec<Completion>, HandlerError> {
        if !self.advertised("on_completion") {
            return Ok(Vec::new());
        }
        #[derive(Deserialize)]
        struct CompletionResult {
            #[serde(default)]
            items: Vec<Completion>,
        }
        let v = self.call(
            "on_completion",
            serde_json::to_value(ctx).expect("LabelCtx"),
        )?;
        let result: CompletionResult = serde_json::from_value(v)
            .map_err(|e| HandlerError::internal(format!("on_completion response decode: {e}")))?;
        Ok(result.items)
    }

    fn on_code_action(&self, ctx: &LabelCtx) -> Result<Vec<CodeAction>, HandlerError> {
        if !self.advertised("on_code_action") {
            return Ok(Vec::new());
        }
        #[derive(Deserialize)]
        struct CodeActionResult {
            #[serde(default)]
            actions: Vec<CodeAction>,
        }
        let v = self.call(
            "on_code_action",
            serde_json::to_value(ctx).expect("LabelCtx"),
        )?;
        let result: CodeActionResult = serde_json::from_value(v)
            .map_err(|e| HandlerError::internal(format!("on_code_action response decode: {e}")))?;
        Ok(result.actions)
    }
}

// ─────────────────────────── Worker thread ────────────────────────────

/// Worker entry point. Owns a single-threaded tokio runtime, spawns the
/// child, runs the initialize handshake, then loops dispatching
/// commands until shutdown.
#[allow(clippy::too_many_arguments)]
fn run_worker(
    argv: Vec<String>,
    init_params: serde_json::Value,
    init_tx: std::sync::mpsc::Sender<Result<InitializeResult, SpawnError>>,
    cmd_rx: mpsc::UnboundedReceiver<WorkerCmd>,
    disabled: Arc<AtomicBool>,
    sandbox: Box<dyn crate::sandbox::Sandbox>,
    capabilities: lex_extension::schema::Capabilities,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build current_thread runtime");
    runtime.block_on(async move {
        worker_main(
            argv,
            init_params,
            init_tx,
            cmd_rx,
            disabled,
            sandbox,
            capabilities,
        )
        .await;
    });
}

#[allow(clippy::too_many_arguments)]
async fn worker_main(
    argv: Vec<String>,
    init_params: serde_json::Value,
    init_tx: std::sync::mpsc::Sender<Result<InitializeResult, SpawnError>>,
    mut cmd_rx: mpsc::UnboundedReceiver<WorkerCmd>,
    disabled: Arc<AtomicBool>,
    sandbox: Box<dyn crate::sandbox::Sandbox>,
    capabilities: lex_extension::schema::Capabilities,
) {
    // Build the Command first, hand it to the sandbox for in-place
    // policy installation (env vars, Unix pre-exec hooks, restricted
    // tokens on Windows), then spawn. Doing the apply_to before
    // spawn() is the contract — once the child is alive it may have
    // already issued syscalls we'd want to block, and most kernel
    // sandboxes can only attach pre-exec.
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Err(e) = sandbox.apply_to(cmd.as_std_mut(), capabilities) {
        let _ = init_tx.send(Err(SpawnError::Sandbox(format!("{e}"))));
        return;
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = init_tx.send(Err(SpawnError::Spawn(e)));
            return;
        }
    };

    let stdin = match child.stdin.take() {
        Some(s) => s,
        None => {
            let _ = init_tx.send(Err(SpawnError::ChildStreamMissing));
            return;
        }
    };
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = init_tx.send(Err(SpawnError::ChildStreamMissing));
            return;
        }
    };
    if let Some(stderr) = child.stderr.take() {
        // Stderr is logged but not parsed — wire spec §1.1.
        tokio::spawn(forward_stderr(stderr));
    }

    let mut io = HandlerIo {
        stdin,
        reader: FrameReader::new(stdout),
    };

    // Initialize handshake. Runs synchronously on this task because
    // it's a single small request/response with a known bounded
    // payload — the deadlock concern only applies once the dispatch
    // loop is intermixing reads and writes.
    let init_result = match do_initialize(&mut io, &init_params).await {
        Ok(r) => r,
        Err(e) => {
            disabled.store(true, Ordering::SeqCst);
            let _ = init_tx.send(Err(e));
            // Try to reap the child; we don't care about the status.
            let _ = child.kill().await;
            return;
        }
    };
    let _ = init_tx.send(Ok(init_result));

    // Decouple stdin writes from the select! loop so a full pipe
    // can never block our stdout reader. This is the deadlock fix
    // pointed out in review of #539: writing inline inside the
    // select!'s `cmd` branch made it possible for a handler that
    // emits a large response (filling its stdout pipe) to block
    // forever, because the host was simultaneously blocking on its
    // own stdin write and not reading the handler's stdout. Both
    // paths now run concurrently in their own tasks.
    let HandlerIo {
        mut stdin,
        mut reader,
    } = io;
    let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let mut writer_handle = tokio::spawn(async move {
        while let Some(bytes) = write_rx.recv().await {
            if stdin.write_all(&bytes).await.is_err() {
                break;
            }
        }
        let _ = stdin.shutdown().await;
    });

    // Main dispatch loop. Ids are allocated by the sync side so the
    // sync-side timeout path can match its CancelPending command to a
    // specific entry. Outgoing frames are handed to the writer task
    // via `write_tx` — this branch never `await`s an actual write.
    let mut pending: HashMap<u64, PendingReply> = HashMap::new();

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(WorkerCmd::Call { id, method, params, reply }) => {
                        let req = OutgoingRequest {
                            jsonrpc: "2.0",
                            id,
                            method,
                            params,
                        };
                        let bytes = encode_frame(&serde_json::to_value(&req).expect("OutgoingRequest"));
                        if write_tx.send(bytes).is_err() {
                            // Writer task is gone — its stdin write
                            // failed (child died or pipe closed).
                            // Surface the failure and bail; the
                            // reader will catch the matching EOF.
                            let _ = reply.send(Err(JsonRpcError {
                                code: codes::INTERNAL,
                                message: "subprocess writer task ended".into(),
                                data: None,
                            }));
                            disabled.store(true, Ordering::SeqCst);
                            break;
                        }
                        pending.insert(id, PendingReply { tx: reply });
                    }
                    Some(WorkerCmd::Notify { method, params }) => {
                        let note = OutgoingNotification {
                            jsonrpc: "2.0",
                            method,
                            params,
                        };
                        let bytes = encode_frame(
                            &serde_json::to_value(&note).expect("OutgoingNotification"),
                        );
                        if write_tx.send(bytes).is_err() {
                            disabled.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                    Some(WorkerCmd::CancelPending { id }) => {
                        // Sync caller timed out on this id. Drop the
                        // pending entry so it doesn't accumulate; if
                        // the response eventually arrives we'll treat
                        // it as an unknown id and ignore it (same as
                        // a notification for an out-of-flight id).
                        pending.remove(&id);
                    }
                    Some(WorkerCmd::Shutdown) | None => {
                        break;
                    }
                }
            }
            frame = reader.read_frame() => {
                match frame {
                    Ok(IncomingFrame::Response { id, result, error, .. }) => {
                        if let Some(pending_reply) = pending.remove(&id) {
                            let payload = match (result, error) {
                                (Some(v), None) => Ok(v),
                                (None, Some(err)) => Err(err),
                                (Some(_), Some(err)) => {
                                    // Spec violation — treat as error.
                                    Err(err)
                                }
                                (None, None) => Err(JsonRpcError {
                                    code: codes::INTERNAL,
                                    message: "handler response carried neither result nor error".into(),
                                    data: None,
                                }),
                            };
                            let _ = pending_reply.tx.send(payload);
                        }
                        // Unknown id = no waiter; drop silently.
                    }
                    Ok(IncomingFrame::Notification { method, .. }) => {
                        eprintln!("[lex-extension-host] subprocess notification: {method}");
                    }
                    Err(FrameError::UnexpectedEof) => {
                        // Child closed stdout. Fail every pending
                        // request and bail.
                        disabled.store(true, Ordering::SeqCst);
                        fail_all_pending(&mut pending, "subprocess handler closed stdout");
                        break;
                    }
                    Err(e) => {
                        // Malformed frame — same handling: fail
                        // pending, disable.
                        disabled.store(true, Ordering::SeqCst);
                        fail_all_pending(&mut pending, &format!("framing error: {e}"));
                        break;
                    }
                }
            }
        }
    }

    // Graceful shutdown ladder: send `shutdown` notification through
    // the writer task, then drop `write_tx` so the writer drains and
    // closes stdin. If the writer is wedged (handler stopped reading
    // stdin), abort it explicitly so it can't keep the runtime alive
    // past `SHUTDOWN_GRACE`. SIGKILL via `kill_on_drop(true)` when
    // `child` falls out of scope.
    let shutdown = OutgoingNotification {
        jsonrpc: "2.0",
        method: "shutdown",
        params: serde_json::Value::Null,
    };
    let _ = write_tx.send(encode_frame(
        &serde_json::to_value(&shutdown).expect("OutgoingNotification"),
    ));
    drop(write_tx);
    if tokio::time::timeout(SHUTDOWN_GRACE, &mut writer_handle)
        .await
        .is_err()
    {
        writer_handle.abort();
    }
    let _ = tokio::time::timeout(SHUTDOWN_GRACE, child.wait()).await;

    fail_all_pending(&mut pending, "subprocess handler shutting down");
}

fn fail_all_pending(pending: &mut HashMap<u64, PendingReply>, message: &str) {
    for (_, reply) in pending.drain() {
        let _ = reply.tx.send(Err(JsonRpcError {
            code: codes::INTERNAL,
            message: message.to_string(),
            data: None,
        }));
    }
}

async fn do_initialize(
    io: &mut HandlerIo,
    params: &serde_json::Value,
) -> Result<InitializeResult, SpawnError> {
    let req = OutgoingRequest {
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: params.clone(),
    };
    let bytes = encode_frame(&serde_json::to_value(&req).expect("OutgoingRequest"));
    io.stdin
        .write_all(&bytes)
        .await
        .map_err(|e| SpawnError::Initialize(InitializeError::Transport(e.to_string())))?;

    // Read frames until we get the response with id=1. Per the wire
    // spec, handlers may legitimately emit notifications before the
    // initialize response (e.g., log lines or progress); we log and
    // skip those rather than treat them as protocol errors.
    let deadline = tokio::time::Instant::now() + INITIALIZE_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(SpawnError::Initialize(InitializeError::Timeout));
        }
        let frame = tokio::time::timeout(remaining, io.reader.read_frame())
            .await
            .map_err(|_| SpawnError::Initialize(InitializeError::Timeout))?
            .map_err(|e| SpawnError::Initialize(InitializeError::Transport(e.to_string())))?;
        match frame {
            IncomingFrame::Response {
                id: 1,
                result: Some(r),
                error: None,
                ..
            } => {
                return serde_json::from_value::<InitializeResult>(r).map_err(|e| {
                    SpawnError::Initialize(InitializeError::BadResponse(e.to_string()))
                });
            }
            IncomingFrame::Response {
                id: 1,
                error: Some(err),
                ..
            } => {
                return Err(SpawnError::Initialize(InitializeError::HandlerError {
                    code: err.code,
                    message: err.message,
                }));
            }
            IncomingFrame::Response { id, .. } => {
                return Err(SpawnError::Initialize(InitializeError::BadResponse(
                    format!("initialize response id={id}, expected 1"),
                )));
            }
            IncomingFrame::Notification { method, .. } => {
                eprintln!(
                    "[lex-extension-host] subprocess notification during initialize: {method}"
                );
                continue;
            }
        }
    }
}

struct HandlerIo {
    stdin: ChildStdin,
    reader: FrameReader,
}

/// Buffered LSP-frame reader over the child's stdout.
struct FrameReader {
    stdout: ChildStdout,
    /// Carry-over bytes read past one frame's body — typically empty
    /// because frames are streamed, but a fast handler may push
    /// multiple frames inside one syscall.
    buf: Vec<u8>,
}

impl FrameReader {
    fn new(stdout: ChildStdout) -> Self {
        Self {
            stdout,
            buf: Vec::with_capacity(4096),
        }
    }

    /// Read one full frame from the child. Returns the parsed
    /// [`IncomingFrame`] or a [`FrameError`] on EOF / malformed input.
    async fn read_frame(&mut self) -> Result<IncomingFrame, FrameError> {
        // Find the header/body separator in the buffer, refilling from
        // stdout as needed. Cap the unbounded fill: a misbehaving
        // handler that streams bytes without `\r\n\r\n` could otherwise
        // grow `self.buf` until the host runs out of memory. 8 KiB is
        // generous for a few `Name: value` lines (LSP rarely uses more
        // than `Content-Length` and `Content-Type`).
        const MAX_HEADER_BYTES: usize = 8 * 1024;
        let header_end = loop {
            if let Some(pos) = find_header_end(&self.buf) {
                break pos;
            }
            if self.buf.len() >= MAX_HEADER_BYTES {
                return Err(FrameError::MalformedHeader(format!(
                    "no header separator after {} bytes",
                    self.buf.len()
                )));
            }
            let mut chunk = [0u8; 4096];
            let n = self.stdout.read(&mut chunk).await?;
            if n == 0 {
                return Err(FrameError::UnexpectedEof);
            }
            self.buf.extend_from_slice(&chunk[..n]);
        };

        let header_bytes = &self.buf[..header_end];
        let header_str = std::str::from_utf8(header_bytes)
            .map_err(|_| FrameError::MalformedHeader("non-UTF-8 bytes in headers".to_string()))?;
        // header_end points at the start of `\r\n\r\n`; +4 skips it.
        let body_start = header_end + 4;
        let body_len = parse_headers(header_str)?;

        // Refill until we have body_len bytes after body_start.
        while self.buf.len() < body_start + body_len {
            let mut chunk = [0u8; 4096];
            let n = self.stdout.read(&mut chunk).await?;
            if n == 0 {
                return Err(FrameError::UnexpectedEof);
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }

        let body = self.buf[body_start..body_start + body_len].to_vec();
        // Drop the consumed bytes; preserve any trailing pipeline.
        self.buf.drain(..body_start + body_len);

        let frame: IncomingFrame = serde_json::from_slice(&body)?;
        Ok(frame)
    }
}

/// Locate the start of the first `\r\n\r\n` separator. Returned index
/// is the position of the *first* `\r`; the body starts 4 bytes later.
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

async fn forward_stderr(mut stderr: ChildStderr) {
    let mut buf = [0u8; 4096];
    loop {
        match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                let chunk = String::from_utf8_lossy(&buf[..n]);
                for line in chunk.lines() {
                    if !line.is_empty() {
                        eprintln!("[lex-extension-host:handler] {line}");
                    }
                }
            }
        }
    }
}

// ──────────────────────────── unit tests ────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_env_expands_known_variables() {
        let env = SpawnEnv {
            workspace_root: Some("/ws".into()),
            lex_cache: Some("/cache".into()),
            handler_config: Some("/conf.toml".into()),
        };
        assert_eq!(env.expand("--ws=${WORKSPACE_ROOT}").unwrap(), "--ws=/ws");
        assert_eq!(env.expand("${LEX_CACHE}/x").unwrap(), "/cache/x");
        assert_eq!(env.expand("${HANDLER_CONFIG}").unwrap(), "/conf.toml");
        // Multiple in one entry.
        assert_eq!(
            env.expand("${WORKSPACE_ROOT}:${LEX_CACHE}").unwrap(),
            "/ws:/cache"
        );
        // No variables: identity.
        assert_eq!(env.expand("plain").unwrap(), "plain");
    }

    #[test]
    fn spawn_env_unknown_variable_rejected() {
        let env = SpawnEnv::default();
        let err = env.expand("--x=${MYSTERY}").unwrap_err();
        match err {
            SpawnError::UnknownVariable { name } => assert_eq!(name, "MYSTERY"),
            other => panic!("expected UnknownVariable, got {other}"),
        }
    }

    #[test]
    fn spawn_env_unclosed_variable_rejected() {
        let env = SpawnEnv::default();
        let err = env.expand("--x=${WORKSPACE_ROOT").unwrap_err();
        assert!(matches!(err, SpawnError::UnclosedVariable { .. }));
    }

    #[test]
    fn spawn_env_known_but_unset_substitutes_empty() {
        // None on the host side is "known but unset" — substitutes
        // empty rather than erroring. This matches POSIX `${VAR}`
        // semantics for declared-but-empty variables and lets a host
        // omit `LEX_CACHE` without breaking handlers that reference it.
        let env = SpawnEnv::default();
        assert_eq!(env.expand("--cache=${LEX_CACHE}").unwrap(), "--cache=");
    }

    #[test]
    fn handler_error_from_jsonrpc_maps_reserved_codes() {
        let err = handler_error_from_jsonrpc(JsonRpcError {
            code: codes::METHOD_NOT_FOUND,
            message: "no such".into(),
            data: None,
        });
        assert!(matches!(err, HandlerError::Unsupported { .. }));

        let err = handler_error_from_jsonrpc(JsonRpcError {
            code: codes::INTERNAL,
            message: "oops".into(),
            data: None,
        });
        assert!(matches!(err, HandlerError::Internal { .. }));

        let err = handler_error_from_jsonrpc(JsonRpcError {
            code: -32050,
            message: "custom".into(),
            data: Some(serde_json::json!(1)),
        });
        match err {
            HandlerError::Custom { code, .. } => assert_eq!(code, -32050),
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn find_header_end_empty_buffer() {
        assert!(find_header_end(b"").is_none());
        assert!(find_header_end(b"Content-Length: 1\r\n").is_none());
        // "Content-Length: 1" is 17 bytes; the \r\n\r\n separator
        // therefore starts at byte 17.
        let pos = find_header_end(b"Content-Length: 1\r\n\r\nx").unwrap();
        assert_eq!(pos, 17);
    }
}
