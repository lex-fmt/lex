//! Render-hook dispatch: walk the IR, ask registered handlers to
//! render their labelled annotations / verbatim blocks, and surface
//! the results to the format-specific serializer.
//!
//! This module is format-independent: it builds a [`RenderPlan`] of
//! `(label, rendered string, optional diagnostic)` triples in document
//! order. The serializer for each format (HTML first, others later) is
//! responsible for splicing the rendered strings into its output and
//! routing the diagnostics onto the host's diagnostic channel.
//!
//! Bounded extensibility: a label whose namespace is *not* registered
//! is left untouched — the default rendering applies.
//!
//! # Status (#616)
//!
//! Render dispatch now walks the IR (`ir::nodes::Document`), not the
//! lex-core AST. This brings the "centralize hard logic in the IR"
//! principle from `crates/lex-babel/src/lib.rs:60-64` back to a single
//! center: the same IR feeds both the event-stream serializer and the
//! render-hook dispatch.
//!
//! ## Behavioural note: `NodeRef.kind` for attached annotations
//!
//! The wire spec §2.1 says `LabelCtx.node.kind` is the host AST kind
//! the label is attached to. The IR flattens attached annotations into
//! sibling DocNodes (see `ir::from_lex::extract_attached_annotations`),
//! so the original attachment kind is not recoverable from the IR
//! alone. Handlers receiving an attached annotation see the kind of
//! its IR *container* (the session, list-item, definition, or outer
//! annotation it sits inside) rather than the kind of the element it
//! was attached to in the source.
//!
//! Doc-scope annotations — those that live in
//! `Document::document_annotations` — continue to surface as
//! `kind="document"`.
//!
//! Restoring source-attachment precision needs an IR-side change (a
//! per-Annotation `attached_to: Option<HostNodeKind>` tag set by
//! `from_lex`); that belongs to the IR symmetry work-stream
//! (#614 / Sub A territory), not the render-dispatch migration.

use lex_extension::wire::{Format, HostNodeKind};
use lex_extension::{schema::Schema, AnnotationBody, LabelCtx, NodeRef, RenderOut};
use lex_extension_host::Registry;

use crate::ir::nodes::{Annotation, DocNode, Document, Verbatim};
use crate::ir::to_wire::ir_annotation_body_to_json;

/// One render result for a labelled node, captured during the IR
/// walk so the format-specific serializer can splice it into the
/// final output.
pub struct RenderedNode {
    /// Fully-qualified label.
    pub label: String,
    /// Format-shaped string the handler returned (HTML for the HTML
    /// pipeline, Markdown for the Markdown pipeline, etc.). `None`
    /// means "fall back to default rendering" — either because the
    /// handler said `Ok(None)` or because it errored / returned a
    /// wrong-shape value.
    pub output: Option<String>,
    /// Optional diagnostic the handler produced via `Err`. Surfaced
    /// to the caller alongside the rendered output so they can route
    /// it onto whatever channel the host uses (stderr in `lexd`,
    /// `publishDiagnostics` in `lex-lsp`).
    pub diagnostic: Option<String>,
}

/// Outcome of the render-dispatch walk: a list of per-node render
/// results in document order, plus any document-root diagnostics
/// (panic, namespace disabled).
pub struct RenderPlan {
    pub nodes: Vec<RenderedNode>,
    /// How many entries at the head of [`nodes`](Self::nodes) came
    /// from `document.document_annotations` (the doc-scope slot)
    /// rather than the body walk. `tree_to_events` does **not** emit
    /// events for that slot (Phase 3b of #614 made doc-scope IR-only),
    /// so an event-indexed splice consumer (the HTML serializer) must
    /// skip these prefix entries to keep its index aligned with the
    /// `StartAnnotation` events it actually receives. Routing doc-
    /// scope handler output back into the rendered HTML is still a
    /// follow-up — the entries are surfaced in the plan and their
    /// diagnostics flow through, but no splice site exists for them
    /// yet.
    pub doc_scope_count: usize,
    /// Root-level diagnostics from the registry (e.g., a namespace
    /// disabled after a panic).
    pub root_diagnostics: Vec<String>,
}

/// Walk `document` and dispatch `on_render` for every labelled
/// annotation / verbatim whose schema declares the format in
/// `hooks.render`. Returns the plan; the caller (HTML serializer) is
/// responsible for splicing.
///
/// `format_name` is matched case-insensitively against entries in
/// `schema.hooks.render`. Today the HTML pipeline passes `"html"`;
/// future formats pass `"markdown"`, `"latex"`, etc.
pub fn dispatch_render(document: &Document, registry: &Registry, format_name: &str) -> RenderPlan {
    let mut nodes = Vec::new();
    if registry.namespace_count() == 0 {
        return RenderPlan {
            nodes,
            doc_scope_count: 0,
            root_diagnostics: Vec::new(),
        };
    }
    let format = format_for_name(format_name);
    let mut ctx = WalkCtx {
        registry,
        format: &format,
        out: &mut nodes,
    };
    // Document-scope annotations (the `document_annotations` slot
    // populated by Phase 3b) come first so the plan reflects source
    // order. Track how many entries this prefix produced so event-
    // indexed splice consumers can skip them — `tree_to_events`
    // doesn't emit events for this slot.
    for ann in &document.document_annotations {
        visit_annotation(ann, HostNodeKind::Document, &mut ctx);
    }
    let doc_scope_count = ctx.out.len();
    for child in &document.children {
        walk_doc_node(child, HostNodeKind::Session, &mut ctx);
    }
    let root_diagnostics = registry
        .take_root_diagnostics()
        .into_iter()
        .map(|d| d.message)
        .collect();
    RenderPlan {
        nodes,
        doc_scope_count,
        root_diagnostics,
    }
}

fn format_for_name(name: &str) -> Format {
    match name.to_ascii_lowercase().as_str() {
        "html" => Format::Html,
        "markdown" | "md" => Format::Markdown,
        "latex" | "tex" => Format::Latex,
        "pdf" => Format::Pdf,
        other => Format::Custom(other.to_string()),
    }
}

/// Bundle the parameters that thread through every walker frame so
/// the function signatures don't grow another argument every time we
/// add a piece of context. Borrowed-only — the walk doesn't own
/// anything that needs lifetime management here.
struct WalkCtx<'a> {
    registry: &'a Registry,
    /// The canonical [`Format`] for the dispatch pass. Schema gating
    /// uses `format.as_str()` so callers can pass aliases (`"md"`,
    /// `"tex"`) at the entry point without breaking schema lookup —
    /// `format_for_name` normalises before we get here.
    format: &'a Format,
    out: &'a mut Vec<RenderedNode>,
}

fn walk_doc_node(node: &DocNode, parent_kind: HostNodeKind, ctx: &mut WalkCtx<'_>) {
    match node {
        DocNode::Heading(h) => {
            for child in &h.children {
                walk_doc_node(child, HostNodeKind::Session, ctx);
            }
        }
        DocNode::Paragraph(_) | DocNode::Inline(_) => {}
        DocNode::List(l) => {
            for item in &l.items {
                for child in &item.children {
                    walk_doc_node(child, HostNodeKind::ListItem, ctx);
                }
            }
        }
        DocNode::ListItem(li) => {
            for child in &li.children {
                walk_doc_node(child, HostNodeKind::ListItem, ctx);
            }
        }
        DocNode::Definition(d) => {
            for child in &d.description {
                walk_doc_node(child, HostNodeKind::Definition, ctx);
            }
        }
        DocNode::Annotation(a) => {
            visit_annotation(a, parent_kind, ctx);
        }
        DocNode::Verbatim(v) => {
            visit_verbatim(v, ctx);
        }
        DocNode::Table(t) => {
            // Header rows before body rows — must match
            // `nested_to_flat::tree_to_events`'s emit order so the
            // event-indexed splice walker stays aligned with the
            // dispatch plan.
            for cell in t.header.iter().chain(t.rows.iter()).flat_map(|r| &r.cells) {
                for child in &cell.content {
                    walk_doc_node(child, HostNodeKind::Table, ctx);
                }
            }
            for child in &t.footnotes {
                walk_doc_node(child, HostNodeKind::List, ctx);
            }
        }
        DocNode::Image(_) | DocNode::Video(_) | DocNode::Audio(_) | DocNode::Document(_) => {}
    }
}

fn visit_annotation(annotation: &Annotation, attached_to: HostNodeKind, ctx: &mut WalkCtx<'_>) {
    let label = annotation.label.clone();
    if let Some(schema) = ctx.registry.schema_for(&label) {
        if schema_has_render(&schema, ctx.format.as_str()) {
            let body_json = ir_annotation_body_to_json(&annotation.content);
            let body = match serde_json::from_value::<AnnotationBody>(body_json) {
                Ok(b) => b,
                Err(e) => {
                    // Codec bug: emit the diagnostic plan entry and
                    // skip dispatch. The 1:1 plan-entry-per-labelled-
                    // node invariant matters to the splice site.
                    ctx.out.push(RenderedNode {
                        label: label.clone(),
                        output: None,
                        diagnostic: Some(format!(
                            "internal: failed to decode annotation body for `{label}`: {e}"
                        )),
                    });
                    // Children still need walking — a malformed body
                    // codec doesn't excuse skipping nested labelled
                    // content.
                    for child in &annotation.content {
                        walk_doc_node(child, HostNodeKind::Annotation, ctx);
                    }
                    return;
                }
            };
            let label_ctx = LabelCtx {
                label: label.clone(),
                params: ir_params_to_json(&annotation.parameters),
                body,
                node: NodeRef {
                    kind: attached_to.as_str().to_string(),
                    range: zero_range(),
                    origin: None,
                },
            };
            ctx.out
                .push(dispatch_one(&label, ctx.registry, &label_ctx, ctx.format));
        }
    }
    // Long-form annotations carry nested content (further annotations,
    // verbatim blocks, …) that also needs render dispatch. Walk
    // children unconditionally — even when this annotation's own
    // schema doesn't match, a registered label inside its body still
    // needs its handler called.
    for child in &annotation.content {
        walk_doc_node(child, HostNodeKind::Annotation, ctx);
    }
}

fn visit_verbatim(v: &Verbatim, ctx: &mut WalkCtx<'_>) {
    // A labelled verbatim with `on_ir_build` gets hydrated into a
    // typed `DocNode` (Table / Image / Video / Audio) and never
    // reaches here. A labelled verbatim without `on_ir_build` —
    // typical for third-party `verbatim_label: true` schemas that
    // only declare `on_render` — falls back to `DocNode::Verbatim`
    // with the closing label preserved in `language`. Resurrect
    // render dispatch for that case via the `language` field.
    //
    // Known gap: IR `Verbatim` doesn't carry the closing-data
    // parameters, so handlers see empty params here. The pre-#616
    // AST walk had full param access. Restoring this needs an IR-
    // side `params` field on `Verbatim`; that's IR-symmetry work
    // (Sub A territory, #614) and out of scope for #616.
    let Some(label) = v.language.as_deref() else {
        return;
    };
    if label.is_empty() {
        return;
    }
    let Some(schema) = ctx.registry.schema_for(label) else {
        return;
    };
    if !schema.verbatim_label || !schema_has_render(&schema, ctx.format.as_str()) {
        return;
    }
    let label_ctx = LabelCtx {
        label: label.to_string(),
        params: serde_json::Value::Object(serde_json::Map::new()),
        body: AnnotationBody::Text(v.content.clone()),
        node: NodeRef {
            kind: HostNodeKind::Verbatim.as_str().to_string(),
            range: zero_range(),
            origin: None,
        },
    };
    ctx.out
        .push(dispatch_one(label, ctx.registry, &label_ctx, ctx.format));
}

fn dispatch_one(label: &str, registry: &Registry, ctx: &LabelCtx, format: &Format) -> RenderedNode {
    match registry.dispatch_render(ctx, format.clone()) {
        Ok(Some(RenderOut::String { string })) => RenderedNode {
            label: label.to_string(),
            output: Some(string),
            diagnostic: None,
        },
        Ok(Some(RenderOut::WireAst { .. })) => RenderedNode {
            label: label.to_string(),
            output: None,
            diagnostic: Some(format!(
                "handler returned WireAst output for label `{label}` but the requested format is string-shaped; falling back to default rendering"
            )),
        },
        Ok(None) => RenderedNode {
            label: label.to_string(),
            output: None,
            diagnostic: None,
        },
        Err(diag) => RenderedNode {
            label: label.to_string(),
            output: None,
            diagnostic: Some(diag.message),
        },
    }
}

fn schema_has_render(schema: &Schema, format_name: &str) -> bool {
    schema
        .hooks
        .render
        .iter()
        .any(|h| h.0.eq_ignore_ascii_case(format_name))
}

fn ir_params_to_json(params: &[(String, String)]) -> serde_json::Value {
    let mut obj = serde_json::Map::with_capacity(params.len());
    for (k, v) in params {
        obj.insert(k.clone(), serde_json::Value::String(v.clone()));
    }
    serde_json::Value::Object(obj)
}

fn zero_range() -> lex_extension::wire::Range {
    use lex_extension::wire::{Position, Range};
    Range::new(Position::new(0, 0), Position::new(0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::loader::DocumentLoader;
    use lex_extension::schema::{
        BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, RenderHook, Schema,
    };
    use lex_extension::wire::WireNode;
    use lex_extension::{HandlerError, LexHandler};
    use std::collections::BTreeMap;

    fn parse(src: &str) -> Document {
        let ast = DocumentLoader::from_string(src).parse().expect("parse");
        crate::to_ir(&ast)
    }

    fn schema(label: &str, formats: &[&str]) -> Schema {
        Schema {
            schema_version: 1,
            label: label.into(),
            description: None,
            params: BTreeMap::new(),
            attaches_to: vec![
                "annotation".into(),
                "document".into(),
                "session".into(),
                "paragraph".into(),
            ],
            body: BodyShape {
                kind: BodyKind::None,
                presence: BodyPresence::Optional,
                description: None,
            },
            verbatim_label: false,
            capabilities: Capabilities::default(),
            hooks: HookSet {
                render: formats.iter().map(|s| RenderHook::new(*s)).collect(),
                ..HookSet::default()
            },
            handler: None,
        }
    }

    struct EchoRender;
    impl LexHandler for EchoRender {
        fn on_render(
            &self,
            ctx: &LabelCtx,
            _fmt: Format,
        ) -> Result<Option<RenderOut>, HandlerError> {
            Ok(Some(RenderOut::String {
                string: format!("<RENDERED label=\"{}\"/>", ctx.label),
            }))
        }
    }

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
            fn on_render(
                &self,
                _: &LabelCtx,
                _: Format,
            ) -> Result<Option<RenderOut>, HandlerError> {
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
            fn on_render(
                &self,
                _: &LabelCtx,
                _: Format,
            ) -> Result<Option<RenderOut>, HandlerError> {
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

    /// End-to-end: registry-aware HTML serialization runs the dispatch
    /// pass, surfaces handler diagnostics in the outcome, and produces
    /// the default-rendered HTML.
    #[test]
    fn end_to_end_html_pipeline_surfaces_diagnostics() {
        use crate::formats::html::{serialize_to_html_with_registry, HtmlOptions, HtmlTheme};
        struct Boom;
        impl LexHandler for Boom {
            fn on_render(
                &self,
                _: &LabelCtx,
                _: Format,
            ) -> Result<Option<RenderOut>, HandlerError> {
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
        let outcome = serialize_to_html_with_registry(
            &ast,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
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
            fn on_render(
                &self,
                ctx: &LabelCtx,
                _: Format,
            ) -> Result<Option<RenderOut>, HandlerError> {
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
        let outcome = serialize_to_html_with_registry(
            &ast,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
        .expect("serialise");
        let baseline = serialize_to_html(&ast, HtmlTheme::default()).expect("baseline");
        assert_eq!(outcome.html, baseline);
        assert!(outcome.diagnostics.is_empty());
    }

    // -- Splice tests. The splice mechanism replaces the default
    // `<!-- lex:label -->` ... `<!-- /lex:label -->` comment pair (and
    // any content between) with the handler's raw HTML when the
    // handler returns `RenderOut::String`. WireAst and `Ok(None)`
    // continue to fall through to default rendering.

    fn registry_with_string_handler(label: &str, html_output: &'static str) -> Registry {
        struct Fixed(&'static str);
        impl LexHandler for Fixed {
            fn on_render(
                &self,
                _: &LabelCtx,
                _: Format,
            ) -> Result<Option<RenderOut>, HandlerError> {
                Ok(Some(RenderOut::String {
                    string: self.0.to_string(),
                }))
            }
        }
        let registry = Registry::new();
        registry
            .register_namespace(
                label.split_once('.').map(|(ns, _)| ns).unwrap_or(label),
                vec![schema(label, &["html"])],
                Box::new(Fixed(html_output)),
            )
            .unwrap();
        registry
    }

    const DOC_WITH_SCOPED_ANNOTATION: &str =
        "1. Heading\n\n    :: acme.task ::\n        Body that should be replaced.\n";

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
        let outcome = serialize_to_html_with_registry(
            &ast,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
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
        let outcome = serialize_to_html_with_registry(
            &ast,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
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
            fn on_render(
                &self,
                _: &LabelCtx,
                _: Format,
            ) -> Result<Option<RenderOut>, HandlerError> {
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
        let outcome = serialize_to_html_with_registry(
            &ast,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
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
            fn on_render(
                &self,
                _: &LabelCtx,
                _: Format,
            ) -> Result<Option<RenderOut>, HandlerError> {
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
        let outcome = serialize_to_html_with_registry(
            &ast,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
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
        let ast =
            DocumentLoader::from_string("1. Heading\n\n    :: unknown.label ::\n        body.\n")
                .parse()
                .expect("parse");
        let registry = Registry::new();
        let outcome = serialize_to_html_with_registry(
            &ast,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
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
            fn on_render(
                &self,
                ctx: &LabelCtx,
                _: Format,
            ) -> Result<Option<RenderOut>, HandlerError> {
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

        let outcome = serialize_to_html_with_registry(
            &ast,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
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
            fn on_render(
                &self,
                ctx: &LabelCtx,
                _: Format,
            ) -> Result<Option<RenderOut>, HandlerError> {
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

    /// Regression for Copilot's review on PR #623: a labelled verbatim
    /// block without an `on_ir_build` handler still needs render
    /// dispatch — `from_lex_verbatim` falls back to `DocNode::Verbatim`
    /// with the closing label preserved in `language`, and
    /// `visit_verbatim` resurrects dispatch via that field.
    #[test]
    fn verbatim_label_with_on_render_only_dispatches_via_language_field() {
        let registry = Registry::new();
        // `verbatim_label: true`, html render hook, no `on_ir_build` —
        // the schema declares the label as a verbatim closer but
        // doesn't hydrate it into a typed IR node, so it stays a
        // generic `DocNode::Verbatim`.
        let mut s = schema("acme.snippet", &["html"]);
        s.verbatim_label = true;
        registry
            .register_namespace("acme", vec![s], Box::new(EchoRender))
            .unwrap();
        // Note the closing `::` line — that's the verbatim form's
        // closer carrying the label.
        let doc = parse("Code:\n    let x = 1;\n:: acme.snippet ::\n");
        let plan = dispatch_render(&doc, &registry, "html");
        assert_eq!(
            plan.nodes.len(),
            1,
            "labelled verbatim with on_render hook should produce a plan entry"
        );
        assert_eq!(plan.nodes[0].label, "acme.snippet");
        assert!(plan.nodes[0]
            .output
            .as_deref()
            .is_some_and(|s| s.contains("acme.snippet")));
    }

    /// Regression for Copilot's review on PR #623: the dispatch walk
    /// must visit table header rows before body rows so the plan
    /// indexing matches `tree_to_events`'s emit order (header-first).
    /// Otherwise an annotation inside a header cell would get its
    /// handler output spliced into the wrong cell.
    #[test]
    fn table_header_cells_walked_before_body_cells() {
        use std::sync::Mutex;
        struct OrderedCapture {
            seen: std::sync::Arc<Mutex<Vec<String>>>,
        }
        impl LexHandler for OrderedCapture {
            fn on_render(
                &self,
                ctx: &LabelCtx,
                _: Format,
            ) -> Result<Option<RenderOut>, HandlerError> {
                self.seen.lock().unwrap().push(ctx.label.clone());
                Ok(None)
            }
        }
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let registry = Registry::new();
        registry
            .register_namespace(
                "acme",
                vec![
                    schema("acme.in_header", &["html"]),
                    schema("acme.in_body", &["html"]),
                ],
                Box::new(OrderedCapture { seen: seen.clone() }),
            )
            .unwrap();
        // Synthesise an IR Table with a labelled annotation in a
        // header cell and another in a body cell. Test the walker
        // directly rather than parsing — keeps the regression
        // hermetic to the dispatch order.
        use crate::ir::nodes::{
            Annotation as IrAnn, DocNode as IrNode, Document as IrDoc, LabelForm,
            Paragraph as IrPara, Table as IrTable, TableCell as IrCell, TableCellAlignment,
            TableRow as IrRow,
        };
        fn cell_with_annotation(label: &str) -> IrCell {
            IrCell {
                content: vec![
                    IrNode::Annotation(IrAnn {
                        label: label.into(),
                        parameters: Vec::new(),
                        content: Vec::new(),
                        form: LabelForm::Canonical,
                    }),
                    IrNode::Paragraph(IrPara {
                        content: vec![crate::ir::nodes::InlineContent::Text("cell".into())],
                    }),
                ],
                header: false,
                align: TableCellAlignment::None,
                colspan: 1,
                rowspan: 1,
            }
        }
        let doc = IrDoc {
            title: None,
            subtitle: None,
            children: vec![IrNode::Table(IrTable {
                rows: vec![IrRow {
                    cells: vec![cell_with_annotation("acme.in_body")],
                }],
                header: vec![IrRow {
                    cells: vec![cell_with_annotation("acme.in_header")],
                }],
                caption: None,
                footnotes: Vec::new(),
                fullwidth: false,
            })],
            document_annotations: Vec::new(),
        };
        let _ = dispatch_render(&doc, &registry, "html");
        let kinds = seen.lock().unwrap().clone();
        assert_eq!(
            kinds.as_slice(),
            &["acme.in_header", "acme.in_body"],
            "header cells must be walked before body cells"
        );
    }
}
