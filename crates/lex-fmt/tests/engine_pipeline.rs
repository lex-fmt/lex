//! Integration tests for [`lex_fmt::Engine`]'s end-to-end pipeline.
//!
//! Exercises parse → resolve → analyze → render with a native
//! fixture handler so the canonical embedder flow is covered as a
//! single round-trip rather than a sum of unit tests in each
//! underlying crate.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use lex_extension::schema::Schema;
use lex_extension::wire::{Diagnostic, DiagnosticSeverity, Format as WireFormat, RenderOut};
use lex_extension::{HandlerError, LabelCtx, LexHandler};
use lex_fmt::Engine;

/// Native handler that:
/// - Validates by emitting one info diagnostic per invocation.
/// - Renders an HTML snippet quoting the label name.
/// - Counts `on_label` *and* `on_render` invocations so tests can
///   confirm each hook fired without re-instrumenting per-test.
struct FixtureHandler {
    label_count: Arc<AtomicUsize>,
    render_count: Arc<AtomicUsize>,
}

impl FixtureHandler {
    fn new() -> Self {
        Self {
            label_count: Arc::new(AtomicUsize::new(0)),
            render_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl LexHandler for FixtureHandler {
    fn on_label(&self, _ctx: &LabelCtx) {
        self.label_count.fetch_add(1, Ordering::SeqCst);
    }

    fn on_validate(&self, ctx: &LabelCtx) -> Result<Vec<Diagnostic>, HandlerError> {
        Ok(vec![Diagnostic {
            severity: DiagnosticSeverity::Info,
            message: format!("fixture handler reached for `{}`", ctx.label),
            range: ctx.node.range,
            code: None,
            related: Vec::new(),
        }])
    }

    fn on_render(
        &self,
        ctx: &LabelCtx,
        fmt: WireFormat,
    ) -> Result<Option<RenderOut>, HandlerError> {
        self.render_count.fetch_add(1, Ordering::SeqCst);
        if matches!(fmt, WireFormat::Html) {
            Ok(Some(RenderOut::String {
                string: format!("<div class=\"acme-task\">{}</div>", ctx.label),
            }))
        } else {
            Ok(None)
        }
    }
}

fn fixture_schema() -> Schema {
    // `label: true` enables on_label notifications; `validate: true`
    // enables on_validate; `render: [html]` enables on_render for the
    // html target.
    let yaml = "schema_version: 1\n\
                label: acme.task\n\
                attaches_to: [document, paragraph, session]\n\
                hooks: { label: true, validate: true, render: [html] }\n";
    serde_yaml::from_str(yaml).expect("fixture schema parses")
}

#[test]
fn builder_default_yields_engine_with_lex_builtins_only() {
    let engine = Engine::builder()
        .workspace_root(std::env::temp_dir())
        .build()
        .expect("default builder builds");
    let registered: Vec<_> = engine
        .registered_namespaces()
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        registered.contains(&"lex"),
        "lex.* built-ins always present"
    );
    // Default formats include lex + html + markdown at minimum.
    let formats = engine.formats();
    assert!(formats.iter().any(|f| f == "lex"));
    assert!(formats.iter().any(|f| f == "html"));
    assert!(formats.iter().any(|f| f == "markdown"));
}

#[test]
fn parse_returns_document_for_well_formed_lex() {
    let engine = Engine::builder()
        .workspace_root(std::env::temp_dir())
        .build()
        .expect("build");
    let doc = engine.parse("A paragraph.\n").expect("parse succeeds");
    assert_eq!(doc.root.children.len(), 1);
}

#[test]
fn native_namespace_registers_and_dispatches_through_engine() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let engine = Engine::builder()
        .workspace_root(workspace.path())
        .with_native_namespace(
            "acme",
            vec![fixture_schema()],
            Box::new(FixtureHandler::new()),
        )
        .build()
        .expect("native namespace registers");

    let names: Vec<_> = engine
        .registered_namespaces()
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert!(names.contains(&"acme".to_string()), "got: {names:?}");
}

#[test]
fn native_namespace_source_kind_is_native_not_builtin() {
    // `with_native_namespace` records the entry as
    // `NamespaceSourceKind::Native` so embedders can tell their own
    // in-process handlers apart from the bundled `lex.*` built-ins
    // when inspecting `registered_namespaces()`.
    use lex_fmt::NamespaceSourceKind;

    let workspace = tempfile::tempdir().expect("tempdir");
    let engine = Engine::builder()
        .workspace_root(workspace.path())
        .with_native_namespace(
            "acme",
            vec![fixture_schema()],
            Box::new(FixtureHandler::new()),
        )
        .build()
        .expect("build");

    let acme = engine
        .registered_namespaces()
        .iter()
        .find(|r| r.name == "acme")
        .expect("acme registered");
    assert_eq!(acme.source, NamespaceSourceKind::Native);

    let lex = engine
        .registered_namespaces()
        .iter()
        .find(|r| r.name == "lex")
        .expect("lex.* registered");
    assert_eq!(lex.source, NamespaceSourceKind::Builtin);
}

#[test]
fn analyze_dispatches_native_handler_validate_and_label_hooks() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let handler = FixtureHandler::new();
    let label_count = handler.label_count.clone();
    let engine = Engine::builder()
        .workspace_root(workspace.path())
        .with_native_namespace("acme", vec![fixture_schema()], Box::new(handler))
        .build()
        .expect("build");

    let source = ":: acme.task ::\n";
    let doc = engine.parse(source).expect("parse");
    let diagnostics = engine.analyze(&doc);

    let from_handler = diagnostics
        .iter()
        .filter(|d| d.message.contains("fixture handler reached"))
        .count();
    assert_eq!(
        from_handler, 1,
        "expected one handler-sourced diagnostic, got {diagnostics:?}",
    );

    assert!(
        label_count.load(Ordering::SeqCst) >= 1,
        "on_label should fire alongside on_validate",
    );
}

#[test]
fn render_html_produces_output_today_without_on_render_splice() {
    // Per Engine::render's doc string: `on_render` hook splicing
    // through the FormatRegistry path awaits the integration tracked
    // at lex-fmt/lex#546. Today, `Engine::render` produces HTML via
    // the no-registry serializer — the handler's `on_render` is not
    // invoked, and its output is not spliced in.
    //
    // This test pins the current behaviour: output is non-empty and
    // contains the standard scaffolding, render_count stays at 0.
    // When #546 lands, flip the render_count assertion to >= 1 and
    // add an HTML-content assertion that proves the splice.
    let workspace = tempfile::tempdir().expect("tempdir");
    let handler = FixtureHandler::new();
    let render_count = handler.render_count.clone();
    let engine = Engine::builder()
        .workspace_root(workspace.path())
        .with_native_namespace("acme", vec![fixture_schema()], Box::new(handler))
        .build()
        .expect("build");

    let doc = engine.parse(":: acme.task ::\n").expect("parse");
    let html = engine.render(&doc, "html").expect("render html");
    assert!(!html.is_empty(), "html output non-empty");
    assert!(
        html.contains("lex-document"),
        "html contains default scaffolding",
    );
    assert_eq!(
        render_count.load(Ordering::SeqCst),
        0,
        "on_render is NOT invoked through Engine::render today (tracked at lex#546)",
    );
}

#[test]
fn on_render_fires_via_registry_aware_serializer_workaround() {
    // The current workaround for #546: embedders that need on_render
    // dispatch use `Engine::registry()` with the per-format
    // registry-aware serializer directly. This test demonstrates the
    // workaround works end-to-end and proves the engine carries
    // everything the embedder needs to opt into hook dispatch today.
    use lex_babel::formats::html::{serialize_to_html_with_registry, HtmlOptions, HtmlTheme};

    let workspace = tempfile::tempdir().expect("tempdir");
    let handler = FixtureHandler::new();
    let render_count = handler.render_count.clone();
    let engine = Engine::builder()
        .workspace_root(workspace.path())
        .with_native_namespace("acme", vec![fixture_schema()], Box::new(handler))
        .build()
        .expect("build");

    let doc = engine.parse(":: acme.task ::\n").expect("parse");
    let outcome = serialize_to_html_with_registry(
        &doc,
        HtmlOptions::new(HtmlTheme::Modern),
        engine.registry(),
    )
    .expect("registry-aware html");
    assert!(!outcome.html.is_empty(), "html output non-empty");
    assert!(
        render_count.load(Ordering::SeqCst) >= 1,
        "on_render fires when the registry-aware serializer is used directly",
    );
}

#[test]
fn engine_is_cloneable_and_clones_share_state() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let engine = Engine::builder()
        .workspace_root(workspace.path())
        .build()
        .expect("build");
    let cloned = engine.clone();

    // Clones see the same registered namespaces because they share
    // the same Arc<Inner>.
    assert_eq!(
        engine.registered_namespaces().len(),
        cloned.registered_namespaces().len(),
    );
}

#[test]
fn native_namespace_collision_with_builtin_is_build_error() {
    let workspace = tempfile::tempdir().expect("tempdir");
    // Try to register a native namespace called `lex` — collides
    // with the built-in.
    let yaml = "schema_version: 1\n\
                label: lex.bogus\n";
    let schema: Schema = serde_yaml::from_str(yaml).expect("schema");
    let result = Engine::builder()
        .workspace_root(workspace.path())
        .with_native_namespace("lex", vec![schema], Box::new(FixtureHandler::new()))
        .build();
    match result {
        Err(lex_fmt::BuildError::NamespaceCollision { namespace, .. }) => {
            assert_eq!(namespace, "lex");
        }
        Err(other) => panic!("expected NamespaceCollision, got {other:?}"),
        Ok(_) => panic!("expected NamespaceCollision, got Ok(Engine)"),
    }
}

#[test]
fn missing_lex_toml_does_not_error() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let missing = workspace.path().join("nonexistent-lex.toml");
    let engine = Engine::builder()
        .workspace_root(workspace.path())
        .load_lex_toml(&missing)
        .expect("missing file is not an error")
        .build()
        .expect("build");
    // Builds with just the built-in lex.* namespace.
    let names: Vec<_> = engine
        .registered_namespaces()
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(names, vec!["lex"]);
}

#[test]
fn auto_trust_prompt_is_installable() {
    use lex_fmt::AutoTrustPrompt;
    let workspace = tempfile::tempdir().expect("tempdir");
    let engine = Engine::builder()
        .workspace_root(workspace.path())
        .trust_prompt(Box::new(AutoTrustPrompt))
        .build()
        .expect("build");
    // Smoke-test only: the prompt isn't consulted without a
    // subprocess-shaped namespace. Just confirm the builder accepts
    // a non-default prompt.
    assert!(!engine.registered_namespaces().is_empty());
}
