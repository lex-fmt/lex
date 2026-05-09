//! Integration tests for the subprocess transport.
//!
//! Each test spawns the in-tree `lex-extension-host-fixture-handler`
//! binary in one of its modes (echo, slow, crash, malformed,
//! version-mismatch) and exercises one corner of the
//! `SubprocessHandler` contract through the public `LexHandler` trait.
//!
//! The fixture binary is declared as a `[[bin]]` of `lex-extension-host`
//! gated on the `subprocess` feature (which is on by default), so cargo
//! sets `CARGO_BIN_EXE_lex-extension-host-fixture-handler` automatically
//! when this test runs.

#![cfg(feature = "subprocess")]

use std::time::Duration;

use lex_extension::schema::{Capabilities, HandlerSpec, HandlerTransport};
use lex_extension::wire::{AnnotationBody, Format, LabelCtx, NodeRef, Position, Range};
use lex_extension::{HandlerError, LexHandler};
use lex_extension_host::transport::{SpawnEnv, SpawnError, SubprocessHandler};

/// Path to the fixture binary cargo built before invoking the test.
fn fixture_bin() -> String {
    env!("CARGO_BIN_EXE_lex-extension-host-fixture-handler").to_string()
}

fn ctx(label: &str) -> LabelCtx {
    LabelCtx {
        label: label.into(),
        params: serde_json::json!({}),
        body: AnnotationBody::None,
        node: NodeRef {
            kind: "annotation".into(),
            range: Range {
                start: Position(1, 0),
                end: Position(1, 10),
            },
            origin: None,
        },
    }
}

fn spec(extra_args: &[&str], timeout_ms: Option<u32>) -> HandlerSpec {
    let mut command = vec![fixture_bin()];
    command.extend(extra_args.iter().map(|s| s.to_string()));
    HandlerSpec {
        transport: HandlerTransport::Subprocess,
        command,
        timeout_ms,
    }
}

fn echo_handler() -> SubprocessHandler {
    SubprocessHandler::spawn(
        &spec(&["echo"], Some(2000)),
        "fixture",
        &["fixture.label".into()],
        Capabilities::default(),
        "test",
        &SpawnEnv::default(),
    )
    .expect("spawn echo handler")
}

#[test]
fn initialize_handshake_succeeds_and_records_implements() {
    let h = echo_handler();
    // Handler advertised the full set in its initialize response.
    for m in [
        "on_label",
        "on_validate",
        "on_resolve",
        "on_render",
        "on_hover",
        "on_completion",
        "on_code_action",
    ] {
        assert!(h.implements(m), "handler must advertise `{m}`");
    }
}

#[test]
fn version_mismatch_in_initialize_yields_clear_error() {
    let err = SubprocessHandler::spawn(
        &spec(&["version-mismatch", "--version", "99"], Some(2000)),
        "fixture",
        &["fixture.label".into()],
        Capabilities::default(),
        "test",
        &SpawnEnv::default(),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("wire_version mismatch"),
        "diagnostic must call out the version mismatch, got: {msg}"
    );
    assert!(
        msg.contains("99"),
        "diagnostic must include the offending version, got: {msg}"
    );
}

#[test]
fn on_validate_round_trips_diagnostics() {
    let h = echo_handler();
    let diags = h.on_validate(&ctx("fixture.label")).expect("ok");
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0].message.contains("fixture.label"),
        "diagnostic must echo the label: {:?}",
        diags[0].message
    );
}

#[test]
fn on_resolve_round_trips_replacement_subtree() {
    let h = echo_handler();
    let resolved = h.on_resolve(&ctx("fixture.label")).expect("ok");
    let node = resolved.expect("returned Some");
    match node {
        lex_extension::WireNode::Paragraph { .. } => {}
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn on_render_round_trips_for_html() {
    let h = echo_handler();
    let out = h
        .on_render(&ctx("fixture.label"), Format::Html)
        .expect("ok");
    let render = out.expect("returned Some");
    match render {
        lex_extension::RenderOut::String { string } => {
            assert!(
                string.contains("fixture.label"),
                "rendered string: {string}"
            );
            assert!(string.contains("html"), "rendered string: {string}");
        }
        other => panic!("expected string output, got {other:?}"),
    }
}

#[test]
fn on_hover_round_trips() {
    let h = echo_handler();
    let hover = h
        .on_hover(&ctx("fixture.label"))
        .expect("ok")
        .expect("Some");
    assert_eq!(hover.contents, "fixture hover");
}

#[test]
fn on_completion_round_trips() {
    let h = echo_handler();
    let items = h.on_completion(&ctx("fixture.label")).expect("ok");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "fixture-completion");
}

#[test]
fn on_code_action_round_trips() {
    let h = echo_handler();
    let actions = h.on_code_action(&ctx("fixture.label")).expect("ok");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].title, "fixture action");
}

#[test]
fn on_label_is_a_notification_with_no_panic() {
    // on_label has no return value to assert on; we just check it
    // runs without panicking and the handler stays usable afterward.
    let h = echo_handler();
    h.on_label(&ctx("fixture.label"));
    let diags = h.on_validate(&ctx("fixture.label")).expect("still works");
    assert_eq!(diags.len(), 1);
}

#[test]
fn slow_handler_hits_timeout() {
    // 50 ms timeout, fixture sleeps 5 s on every call.
    let h = SubprocessHandler::spawn(
        &spec(&["slow", "--delay-ms", "5000"], Some(50)),
        "fixture",
        &["fixture.label".into()],
        Capabilities::default(),
        "test",
        &SpawnEnv::default(),
    )
    .expect("spawn");
    let err = h.on_validate(&ctx("fixture.label")).unwrap_err();
    match err {
        HandlerError::Internal { message } => {
            assert!(
                message.contains("timed out"),
                "expected timeout message, got: {message}"
            );
        }
        other => panic!("expected Internal timeout, got {other:?}"),
    }
}

#[test]
fn crashing_handler_disables_after_first_call() {
    let h = SubprocessHandler::spawn(
        &spec(&["crash", "--on", "on_validate"], Some(2000)),
        "fixture",
        &["fixture.label".into()],
        Capabilities::default(),
        "test",
        &SpawnEnv::default(),
    )
    .expect("spawn");
    let err = h.on_validate(&ctx("fixture.label")).unwrap_err();
    assert!(matches!(err, HandlerError::Internal { .. }));

    // Subsequent calls also fail (handler is disabled, child gone).
    // Give the worker a moment to notice the EOF before the second
    // call, otherwise it might race and look like it succeeded.
    std::thread::sleep(Duration::from_millis(100));
    let err2 = h.on_validate(&ctx("fixture.label")).unwrap_err();
    assert!(matches!(err2, HandlerError::Internal { .. }));
}

#[test]
fn malformed_response_disables_handler() {
    let h = SubprocessHandler::spawn(
        &spec(&["malformed"], Some(2000)),
        "fixture",
        &["fixture.label".into()],
        Capabilities::default(),
        "test",
        &SpawnEnv::default(),
    )
    .expect("spawn");
    let err = h.on_validate(&ctx("fixture.label")).unwrap_err();
    assert!(matches!(err, HandlerError::Internal { .. }));
}

#[test]
fn missing_binary_is_a_spawn_error() {
    let err = SubprocessHandler::spawn(
        &HandlerSpec {
            transport: HandlerTransport::Subprocess,
            command: vec!["/this/binary/definitely/does/not/exist".into()],
            timeout_ms: Some(2000),
        },
        "fixture",
        &["fixture.label".into()],
        Capabilities::default(),
        "test",
        &SpawnEnv::default(),
    )
    .unwrap_err();
    assert!(matches!(err, SpawnError::Spawn(_)) || matches!(err, SpawnError::Initialize(_)));
}

#[test]
fn unknown_env_var_is_rejected_at_spawn() {
    let err = SubprocessHandler::spawn(
        &HandlerSpec {
            transport: HandlerTransport::Subprocess,
            command: vec![fixture_bin(), "${MYSTERY}".into()],
            timeout_ms: Some(2000),
        },
        "fixture",
        &["fixture.label".into()],
        Capabilities::default(),
        "test",
        &SpawnEnv::default(),
    )
    .unwrap_err();
    match err {
        SpawnError::UnknownVariable { name } => assert_eq!(name, "MYSTERY"),
        other => panic!("expected UnknownVariable, got {other}"),
    }
}

#[test]
fn known_env_vars_expand_in_command() {
    // The fixture treats argv[1] as `mode`; we use `${WORKSPACE_ROOT}`
    // expanded to "echo" to verify the substitution actually happens
    // before exec.
    let h = SubprocessHandler::spawn(
        &HandlerSpec {
            transport: HandlerTransport::Subprocess,
            command: vec![fixture_bin(), "${WORKSPACE_ROOT}".into()],
            timeout_ms: Some(2000),
        },
        "fixture",
        &["fixture.label".into()],
        Capabilities::default(),
        "test",
        &SpawnEnv {
            workspace_root: Some("echo".into()),
            ..Default::default()
        },
    )
    .expect("spawn with substituted argv");
    let diags = h.on_validate(&ctx("fixture.label")).expect("ok");
    assert_eq!(diags.len(), 1);
}

/// Regression test for the IPC pipe deadlock identified in review of
/// the original PR. If the worker writes inline inside its `select!`
/// loop, a handler that emits a response large enough to fill the
/// kernel stdout pipe buffer would block waiting for the host to
/// drain it — while the host blocked waiting for its own stdin write
/// to complete. Both processes would hang forever.
///
/// The fix decouples writes onto a dedicated tokio task, so the
/// reader branch of the select! is never starved. To exercise it,
/// the `bigecho` fixture mode pads each `on_validate` response with
/// 256 KiB of filler — well past Linux's typical 64 KiB pipe buffer.
/// We then issue 4 sequential calls in tight succession, which
/// without the fix would deadlock once a single in-flight response
/// stalls the writer.
#[test]
fn large_response_does_not_deadlock_writer() {
    let h = SubprocessHandler::spawn(
        &spec(&["bigecho", "--bytes", "262144"], Some(5000)),
        "fixture",
        &["fixture.label".into()],
        Capabilities::default(),
        "test",
        &SpawnEnv::default(),
    )
    .expect("spawn bigecho");
    for _ in 0..4 {
        let diags = h.on_validate(&ctx("fixture.label")).expect("ok");
        assert_eq!(diags.len(), 1);
    }
}

#[test]
fn drop_shuts_down_child_cleanly() {
    // Spawn → drop → child should be gone within a reasonable time.
    // This is the lazy version of the test: we trust kill_on_drop.
    // The stronger assertion (no zombies) is platform-specific to
    // verify automatically.
    {
        let _h = echo_handler();
        // _h drops here.
    }
    // If kill_on_drop is broken, we'd accumulate zombies running
    // this test repeatedly; cargo would still pass but a parallel
    // observer (e.g. `ps`) would notice. The minimal assertion is
    // that drop returns without hanging — the test framework times
    // out otherwise.
}
