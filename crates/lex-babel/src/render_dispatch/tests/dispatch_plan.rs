//! Registry / format gating and the shape of the [`RenderPlan`] the
//! dispatch walk produces.

use super::*;
use crate::render_dispatch::dispatch_render;
use lex_extension::wire::{Format, WireNode};
use lex_extension::{AnnotationBody, HandlerError, LabelCtx, LexHandler, RenderOut};
use lex_extension_host::Registry;

#[test]
fn empty_registry_yields_empty_plan() {
    let doc = parse(":: acme.task ::\n");
    let registry = Registry::new();
    let plan = dispatch_render(&doc, &registry, "html");
    assert!(plan.nodes.is_empty());
}

#[test]
fn registered_label_with_html_render_hook_dispatches() {
    let doc = parse(":: acme.task ::\n");
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["html"])],
            Box::new(EchoRender),
        )
        .unwrap();
    let plan = dispatch_render(&doc, &registry, "html");
    assert_eq!(plan.nodes.len(), 1);
    assert_eq!(plan.nodes[0].label, "acme.task");
    assert_eq!(
        plan.nodes[0].output.as_deref(),
        Some(r#"<RENDERED label="acme.task"/>"#)
    );
}

#[test]
fn label_without_html_in_render_hooks_is_skipped() {
    let doc = parse(":: acme.task ::\n");
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["markdown"])],
            Box::new(EchoRender),
        )
        .unwrap();
    let plan = dispatch_render(&doc, &registry, "html");
    // Schema declares markdown only — html dispatch skipped.
    assert!(plan.nodes.is_empty());
}

/// Regression for the format-alias mismatch: `format_for_name`
/// normalises `"md"` → `Format::Markdown`, but the schema's
/// `hooks.render` list contains the canonical `"markdown"`.
/// Schema gating must compare against the canonical
/// `Format::as_str()`, not the raw caller input — otherwise an
/// alias caller would never match a canonical schema.
#[test]
fn alias_format_name_matches_canonical_schema_render_hook() {
    let doc = parse(":: acme.task ::\n");
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["markdown"])],
            Box::new(EchoRender),
        )
        .unwrap();
    // Caller passes the alias "md"; schema declares "markdown".
    let plan = dispatch_render(&doc, &registry, "md");
    assert_eq!(plan.nodes.len(), 1);
    assert_eq!(plan.nodes[0].label, "acme.task");
}

#[test]
fn handler_error_yields_diagnostic_and_no_html() {
    struct Boom;
    impl LexHandler for Boom {
        fn on_render(&self, _: &LabelCtx, _: Format) -> Result<Option<RenderOut>, HandlerError> {
            Err(HandlerError::internal("render failed"))
        }
    }
    let doc = parse(":: acme.task ::\n");
    let registry = Registry::new();
    registry
        .register_namespace("acme", vec![schema("acme.task", &["html"])], Box::new(Boom))
        .unwrap();
    let plan = dispatch_render(&doc, &registry, "html");
    assert_eq!(plan.nodes.len(), 1);
    assert!(plan.nodes[0].output.is_none());
    let diag = plan.nodes[0].diagnostic.as_deref().expect("diagnostic");
    assert!(diag.contains("render failed"));
}

#[test]
fn wire_ast_output_for_string_target_falls_back_with_diagnostic() {
    struct WireOut;
    impl LexHandler for WireOut {
        fn on_render(&self, _: &LabelCtx, _: Format) -> Result<Option<RenderOut>, HandlerError> {
            Ok(Some(RenderOut::WireAst {
                ast: WireNode::Document {
                    range: lex_extension::wire::Range::new(
                        lex_extension::wire::Position(0, 0),
                        lex_extension::wire::Position(0, 0),
                    ),
                    origin: None,
                    children: vec![],
                },
            }))
        }
    }
    let doc = parse(":: acme.task ::\n");
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["html"])],
            Box::new(WireOut),
        )
        .unwrap();
    let plan = dispatch_render(&doc, &registry, "html");
    assert_eq!(plan.nodes.len(), 1);
    assert!(plan.nodes[0].output.is_none());
    assert!(plan.nodes[0]
        .diagnostic
        .as_deref()
        .is_some_and(|d| d.contains("WireAst")));
}

/// Wire spec §2.1: `LabelCtx.node.kind` carries the host kind. For
/// a doc-scope annotation (`Document::document_annotations`) we
/// surface `"document"`.
#[test]
fn handler_sees_host_node_kind_in_label_ctx() {
    use std::sync::Mutex;
    struct CaptureKind {
        seen: std::sync::Arc<Mutex<Vec<String>>>,
    }
    impl LexHandler for CaptureKind {
        fn on_render(&self, ctx: &LabelCtx, _: Format) -> Result<Option<RenderOut>, HandlerError> {
            self.seen.lock().unwrap().push(ctx.node.kind.clone());
            Ok(None)
        }
    }
    let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["html"])],
            Box::new(CaptureKind { seen: seen.clone() }),
        )
        .unwrap();
    let doc = parse(":: acme.task ::\n");
    let _ = dispatch_render(&doc, &registry, "html");
    let kinds = seen.lock().unwrap().clone();
    assert_eq!(
        kinds.as_slice(),
        &["document"],
        "top-level annotation must surface as host kind \"document\"",
    );
}

/// Acceptance criterion for #616: a render handler must fire from
/// a markdown serializer path with the same signature it uses for
/// HTML. Sub D (#617) will wire splicing on the markdown side;
/// this test only proves format-agnosticism — the same
/// `dispatch_render` call, against the same IR, with a different
/// `format_name`, routes to the markdown-declared handler.
#[test]
fn dispatch_render_fires_from_markdown_path() {
    let doc = parse(":: acme.task ::\n");
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["markdown"])],
            Box::new(EchoRender),
        )
        .unwrap();
    let plan = dispatch_render(&doc, &registry, "markdown");
    assert_eq!(plan.nodes.len(), 1, "markdown dispatch should fire");
    assert_eq!(plan.nodes[0].label, "acme.task");
    assert_eq!(
        plan.nodes[0].output.as_deref(),
        Some(r#"<RENDERED label="acme.task"/>"#),
        "markdown handler output should surface in the plan"
    );
}

/// Annotation body is passed to handlers as
/// `AnnotationBody::Lex { children }` when the IR annotation has
/// content. Locks the IR→Wire bridge so handlers see the body
/// they expect.
#[test]
fn annotation_body_surfaces_lex_children_to_handler() {
    use std::sync::Mutex;
    struct CaptureBody {
        seen: std::sync::Arc<Mutex<Option<AnnotationBody>>>,
    }
    impl LexHandler for CaptureBody {
        fn on_render(&self, ctx: &LabelCtx, _: Format) -> Result<Option<RenderOut>, HandlerError> {
            *self.seen.lock().unwrap() = Some(ctx.body.clone());
            Ok(None)
        }
    }
    let seen = std::sync::Arc::new(Mutex::new(None));
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["html"])],
            Box::new(CaptureBody { seen: seen.clone() }),
        )
        .unwrap();
    // Annotation with a body paragraph — the IR carries the body
    // in `Annotation::content`, and the IR→Wire bridge surfaces
    // it as `AnnotationBody::Lex { children: [Paragraph] }`.
    let doc = parse(":: acme.task ::\n    inside the body.\n");
    let _ = dispatch_render(&doc, &registry, "html");
    let body = seen.lock().unwrap().clone().expect("handler ran");
    match body {
        AnnotationBody::Lex { children } => {
            assert!(
                !children.is_empty(),
                "lex body must carry annotation children"
            );
        }
        other => panic!("expected AnnotationBody::Lex, got {other:?}"),
    }
}
