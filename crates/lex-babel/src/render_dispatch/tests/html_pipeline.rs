//! End-to-end HTML serialization and the splice behaviour that routes
//! handler output (or falls back to default rendering) for the HTML
//! pipeline.

use super::*;
use crate::render_dispatch::dispatch_render;
use lex_core::lex::loader::DocumentLoader;
use lex_extension::wire::Format;
use lex_extension::{HandlerError, LabelCtx, LexHandler, RenderOut};
use lex_extension_host::Registry;

/// End-to-end: registry-aware HTML serialization runs the dispatch
/// pass, surfaces handler diagnostics in the outcome, and produces
/// the default-rendered HTML.
#[test]
fn end_to_end_html_pipeline_surfaces_diagnostics() {
    use crate::formats::html::{serialize_to_html_with_registry, HtmlOptions, HtmlTheme};
    struct Boom;
    impl LexHandler for Boom {
        fn on_render(&self, _: &LabelCtx, _: Format) -> Result<Option<RenderOut>, HandlerError> {
            Err(HandlerError::internal("rendering failed"))
        }
    }
    let ast = DocumentLoader::from_string(":: acme.task ::\n    Body content.\n")
        .parse()
        .expect("parse");
    let registry = Registry::new();
    registry
        .register_namespace("acme", vec![schema("acme.task", &["html"])], Box::new(Boom))
        .unwrap();
    let outcome =
        serialize_to_html_with_registry(&ast, HtmlOptions::new(HtmlTheme::default()), &registry)
            .expect("serialise");
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|d| d.contains("rendering failed")),
        "expected handler diagnostic in outcome, got: {:?}",
        outcome.diagnostics
    );
    assert!(!outcome.html.is_empty());
}

#[test]
fn end_to_end_html_pipeline_is_passthrough_when_no_hooks_match() {
    use crate::formats::html::{
        serialize_to_html, serialize_to_html_with_registry, HtmlOptions, HtmlTheme,
    };
    let ast = DocumentLoader::from_string(":: acme.task ::\n    Body.\n")
        .parse()
        .expect("parse");
    let registry = Registry::new();
    // Schema declares only markdown — html dispatch skipped.
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["markdown"])],
            Box::new(EchoRender),
        )
        .unwrap();
    let outcome =
        serialize_to_html_with_registry(&ast, HtmlOptions::new(HtmlTheme::default()), &registry)
            .expect("serialise");
    let baseline = serialize_to_html(&ast, HtmlTheme::default()).expect("baseline");
    assert_eq!(outcome.html, baseline);
    assert!(outcome.diagnostics.is_empty());
}

#[test]
fn splice_replaces_default_annotation_rendering_with_handler_html() {
    use crate::formats::html::{serialize_to_html_with_registry, HtmlOptions, HtmlTheme};
    let ast = DocumentLoader::from_string(DOC_WITH_SCOPED_ANNOTATION)
        .parse()
        .expect("parse");
    let registry = registry_with_string_handler(
        "acme.task",
        "<div class=\"acme-task\">handler-rendered</div>",
    );
    let outcome =
        serialize_to_html_with_registry(&ast, HtmlOptions::new(HtmlTheme::default()), &registry)
            .expect("serialise");
    assert!(
        outcome
            .html
            .contains("<div class=\"acme-task\">handler-rendered</div>"),
        "handler HTML should be spliced into the output. got:\n{}",
        outcome.html
    );
    assert!(
        !outcome.html.contains("<!-- lex:acme.task"),
        "default start comment should be replaced by splice. got:\n{}",
        outcome.html
    );
    assert!(
        !outcome.html.contains("<!-- /lex:acme.task"),
        "default end comment should be replaced by splice. got:\n{}",
        outcome.html
    );
}

#[test]
fn splice_consumes_annotation_body_so_default_content_does_not_render() {
    use crate::formats::html::{serialize_to_html_with_registry, HtmlOptions, HtmlTheme};
    let ast = DocumentLoader::from_string(DOC_WITH_SCOPED_ANNOTATION)
        .parse()
        .expect("parse");
    let registry = registry_with_string_handler("acme.task", "<div>HANDLER</div>");
    let outcome =
        serialize_to_html_with_registry(&ast, HtmlOptions::new(HtmlTheme::default()), &registry)
            .expect("serialise");
    assert!(outcome.html.contains("<div>HANDLER</div>"));
    assert!(
        !outcome.html.contains("Body that should be replaced."),
        "annotation body must be suppressed inside the handler-owned region. got:\n{}",
        outcome.html
    );
}

#[test]
fn no_splice_when_handler_returns_none_falls_through_to_default() {
    use crate::formats::html::{serialize_to_html_with_registry, HtmlOptions, HtmlTheme};
    struct AlwaysNone;
    impl LexHandler for AlwaysNone {
        fn on_render(&self, _: &LabelCtx, _: Format) -> Result<Option<RenderOut>, HandlerError> {
            Ok(None)
        }
    }
    let ast = DocumentLoader::from_string(DOC_WITH_SCOPED_ANNOTATION)
        .parse()
        .expect("parse");
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["html"])],
            Box::new(AlwaysNone),
        )
        .unwrap();
    let outcome =
        serialize_to_html_with_registry(&ast, HtmlOptions::new(HtmlTheme::default()), &registry)
            .expect("serialise");
    assert!(
        outcome.html.contains("<!-- lex:acme.task"),
        "Ok(None) should fall through to default rendering. got:\n{}",
        outcome.html
    );
}

#[test]
fn no_splice_when_handler_returns_wire_ast_with_diagnostic() {
    use crate::formats::html::{serialize_to_html_with_registry, HtmlOptions, HtmlTheme};
    use lex_extension::wire::{Range as WireRange, WireNode};
    use lex_extension::Position;
    struct WireOnly;
    impl LexHandler for WireOnly {
        fn on_render(&self, _: &LabelCtx, _: Format) -> Result<Option<RenderOut>, HandlerError> {
            Ok(Some(RenderOut::WireAst {
                ast: WireNode::Paragraph {
                    range: WireRange::new(Position::new(0, 0), Position::new(0, 0)),
                    inlines: Vec::new(),
                    origin: None,
                },
            }))
        }
    }
    let ast = DocumentLoader::from_string(DOC_WITH_SCOPED_ANNOTATION)
        .parse()
        .expect("parse");
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["html"])],
            Box::new(WireOnly),
        )
        .unwrap();
    let outcome =
        serialize_to_html_with_registry(&ast, HtmlOptions::new(HtmlTheme::default()), &registry)
            .expect("serialise");
    assert!(outcome.html.contains("<!-- lex:acme.task"));
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|d| d.contains("WireAst") || d.contains("wire_ast")),
        "expected WireAst-shape-mismatch diagnostic, got: {:?}",
        outcome.diagnostics
    );
}

#[test]
fn unregistered_namespace_renders_default_unchanged() {
    use crate::formats::html::{
        serialize_to_html, serialize_to_html_with_registry, HtmlOptions, HtmlTheme,
    };
    let ast = DocumentLoader::from_string("1. Heading\n\n    :: unknown.label ::\n        body.\n")
        .parse()
        .expect("parse");
    let registry = Registry::new();
    let outcome =
        serialize_to_html_with_registry(&ast, HtmlOptions::new(HtmlTheme::default()), &registry)
            .expect("serialise");
    let baseline = serialize_to_html(&ast, HtmlTheme::default()).expect("baseline");
    assert_eq!(outcome.html, baseline);
}

/// Regression for the doc-scope/body splice misalignment that
/// Copilot flagged on PR #621.
///
/// `dispatch_render` walks `document_annotations` first and emits
/// plan entries, but `tree_to_events` doesn't synthesise events
/// for that slot (Phase 3b of #614). The HTML serializer must
/// slice `plan.doc_scope_count` entries off the front before
/// handing the plan to the splice walker. This test locks that
/// contract.
#[test]
fn doc_scope_annotation_does_not_misroute_body_splice() {
    use crate::formats::html::{serialize_to_html_with_registry, HtmlOptions, HtmlTheme};

    struct ByLabel;
    impl LexHandler for ByLabel {
        fn on_render(&self, ctx: &LabelCtx, _: Format) -> Result<Option<RenderOut>, HandlerError> {
            let marker = match ctx.label.as_str() {
                "acme.docscope" => "<DOCSCOPE_HANDLER_OUTPUT/>",
                "acme.body" => "<BODY_HANDLER_OUTPUT/>",
                _ => return Ok(None),
            };
            Ok(Some(RenderOut::String {
                string: marker.to_string(),
            }))
        }
    }

    let src = ":: acme.docscope ::\n\
               \n\
               1. Heading\n\
               \n    \
               :: acme.body ::\n        \
               Body that should be replaced.\n";
    let ast = DocumentLoader::from_string(src).parse().expect("parse");
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![
                schema("acme.docscope", &["html"]),
                schema("acme.body", &["html"]),
            ],
            Box::new(ByLabel),
        )
        .unwrap();
    let ir_doc = crate::to_ir(&ast);
    let plan = dispatch_render(&ir_doc, &registry, "html");
    assert_eq!(
        plan.doc_scope_count, 1,
        "dispatch_render should report one doc-scope plan entry"
    );
    assert_eq!(
        plan.nodes.len(),
        2,
        "plan should have one doc-scope + one body entry"
    );

    let outcome =
        serialize_to_html_with_registry(&ast, HtmlOptions::new(HtmlTheme::default()), &registry)
            .expect("serialise");

    assert!(
        outcome.html.contains("<BODY_HANDLER_OUTPUT/>"),
        "body annotation must splice the body handler's output. got:\n{}",
        outcome.html
    );
    assert!(
        !outcome.html.contains("<DOCSCOPE_HANDLER_OUTPUT/>"),
        "doc-scope handler output must not be spliced into the body. got:\n{}",
        outcome.html
    );
    assert!(
        !outcome.html.contains("Body that should be replaced."),
        "body handler must own its body content. got:\n{}",
        outcome.html
    );
}
