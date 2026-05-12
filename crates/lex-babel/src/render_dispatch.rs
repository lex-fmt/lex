//! Render-hook dispatch: walk the AST, ask registered handlers to
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
//! # Status
//!
//! HTML wiring lands in this PR through
//! [`crate::formats::html::serialize_to_html_with_registry`]: it runs
//! the AST walk, collects the plan, surfaces handler diagnostics in
//! the [`HtmlExportOutcome`](crate::formats::html::HtmlExportOutcome),
//! and returns the default-rendered HTML. The actual *splice* — i.e.,
//! replacing the default rendering of the labelled node with the
//! handler's output — requires hooking the IR-events pipeline (the
//! HTML serializer collapses all annotations into a synthetic
//! `frontmatter` block, so there is no per-annotation HTML comment to
//! post-process). That integration is a follow-up; today's PR
//! delivers the dispatch surface and the diagnostic plumbing so PR 9
//! and downstream consumers can light up handler-driven rendering as
//! the splice sites land.

use lex_core::lex::ast::{Annotation, ContentItem, Document, Session, Verbatim};
use lex_core::lex::wire::to_wire_node;
use lex_extension::wire::{Format, HostNodeKind, WireNode};
use lex_extension::{schema::Schema, AnnotationBody, LabelCtx, NodeRef, RenderOut};
use lex_extension_host::Registry;

/// One render result for a labelled node, captured during the AST
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
            root_diagnostics: Vec::new(),
        };
    }
    let format = format_for_name(format_name);
    let mut ctx = WalkCtx {
        registry,
        format: &format,
        out: &mut nodes,
    };
    // Document-level annotations (parsed before the body) come first
    // so the plan reflects source order.
    for ann in document.annotations() {
        visit_annotation(ann, HostNodeKind::Document, &mut ctx);
    }
    walk_session(&document.root, HostNodeKind::Session, &mut ctx);
    let root_diagnostics = registry
        .take_root_diagnostics()
        .into_iter()
        .map(|d| d.message)
        .collect();
    RenderPlan {
        nodes,
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

fn walk_session(session: &Session, self_kind: HostNodeKind, ctx: &mut WalkCtx<'_>) {
    for ann in session.annotations() {
        visit_annotation(ann, self_kind, ctx);
    }
    for child in session.children.iter() {
        visit_content(child, HostNodeKind::Session, ctx);
    }
}

fn visit_content(item: &ContentItem, parent_kind: HostNodeKind, ctx: &mut WalkCtx<'_>) {
    match item {
        ContentItem::Paragraph(p) => {
            for ann in p.annotations() {
                visit_annotation(ann, HostNodeKind::Paragraph, ctx);
            }
        }
        ContentItem::Session(s) => walk_session(s, HostNodeKind::Session, ctx),
        ContentItem::Definition(def) => {
            for ann in def.annotations() {
                visit_annotation(ann, HostNodeKind::Definition, ctx);
            }
            for child in def.children.iter() {
                visit_content(child, HostNodeKind::Definition, ctx);
            }
        }
        ContentItem::List(list) => {
            // List-level annotations attach to the list itself, NOT
            // to its items.
            for ann in list.annotations() {
                visit_annotation(ann, HostNodeKind::List, ctx);
            }
            for entry in &list.items {
                if let ContentItem::ListItem(li) = entry {
                    for ann in li.annotations() {
                        visit_annotation(ann, HostNodeKind::ListItem, ctx);
                    }
                    for child in li.children.iter() {
                        visit_content(child, HostNodeKind::ListItem, ctx);
                    }
                }
            }
        }
        ContentItem::Annotation(a) => {
            visit_annotation(a, parent_kind, ctx);
        }
        ContentItem::VerbatimBlock(v) => {
            visit_verbatim(v, ctx);
            for ann in v.annotations() {
                visit_annotation(ann, HostNodeKind::Verbatim, ctx);
            }
        }
        ContentItem::Table(t) => {
            for ann in t.annotations() {
                visit_annotation(ann, HostNodeKind::Table, ctx);
            }
            // Walk block-level content nested inside table cells (a
            // cell can hold a list / definition / verbatim, which can
            // in turn carry labelled annotations).
            for child in t.cell_children_iter() {
                visit_content(child, HostNodeKind::Table, ctx);
            }
            if let Some(footnotes) = t.footnotes.as_deref() {
                for ann in footnotes.annotations() {
                    visit_annotation(ann, HostNodeKind::List, ctx);
                }
                for entry in &footnotes.items {
                    if let ContentItem::ListItem(li) = entry {
                        for ann in li.annotations() {
                            visit_annotation(ann, HostNodeKind::ListItem, ctx);
                        }
                        for child in li.children.iter() {
                            visit_content(child, HostNodeKind::ListItem, ctx);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn visit_annotation(annotation: &Annotation, attached_to: HostNodeKind, ctx: &mut WalkCtx<'_>) {
    let label = annotation.data.label.value.clone();
    if let Some(schema) = ctx.registry.schema_for(&label) {
        if schema_has_render(&schema, ctx.format.as_str()) {
            let wire = to_wire_node(&ContentItem::Annotation(annotation.clone()));
            if let WireNode::Annotation {
                params,
                body,
                range,
                origin,
                ..
            } = wire
            {
                // Body deserialization is only fallible if the wire
                // codec produced a value `AnnotationBody`'s untagged
                // representation can't accept — that's a codec bug
                // worth surfacing rather than silently dropping the
                // body.
                let body = match serde_json::from_value::<AnnotationBody>(body) {
                    Ok(b) => b,
                    Err(e) => {
                        // Codec bug: emit the diagnostic plan entry
                        // (one entry per labelled node, per the
                        // RenderPlan contract) and skip dispatch. The
                        // earlier flow continued to dispatch with
                        // `AnnotationBody::None`, producing a second
                        // entry for the same node and breaking the
                        // 1:1 invariant the splice site relies on.
                        ctx.out.push(RenderedNode {
                            label: label.clone(),
                            output: None,
                            diagnostic: Some(format!(
                                "internal: failed to decode annotation body for `{label}`: {e}"
                            )),
                        });
                        // Children still need walking — a malformed
                        // body codec doesn't excuse skipping nested
                        // labelled content.
                        for child in annotation.children.iter() {
                            visit_content(child, HostNodeKind::Annotation, ctx);
                        }
                        return;
                    }
                };
                let label_ctx = LabelCtx {
                    label: label.clone(),
                    params,
                    body,
                    node: NodeRef {
                        // Wire spec §2.1: NodeRef.kind is the host AST
                        // kind the label is attached to (paragraph /
                        // list / table / …) — handlers use it to
                        // disambiguate context. Previously hardcoded
                        // to "annotation" regardless of the actual
                        // host.
                        kind: attached_to.as_str().to_string(),
                        range,
                        origin,
                    },
                };
                ctx.out
                    .push(dispatch_one(&label, ctx.registry, &label_ctx, ctx.format));
            }
        }
    }
    // Long-form annotations carry nested content (further annotations,
    // verbatim blocks, …) that also needs render dispatch. Walk
    // children unconditionally — even when this annotation's own
    // schema doesn't match, a registered label inside its body still
    // needs its handler called.
    for child in annotation.children.iter() {
        visit_content(child, HostNodeKind::Annotation, ctx);
    }
}

fn visit_verbatim(v: &Verbatim, ctx: &mut WalkCtx<'_>) {
    let label = v.closing_data.label.value.clone();
    if label.is_empty() {
        return;
    }
    let Some(schema) = ctx.registry.schema_for(&label) else {
        return;
    };
    if !schema.verbatim_label || !schema_has_render(&schema, ctx.format.as_str()) {
        return;
    }
    let wire = to_wire_node(&ContentItem::VerbatimBlock(Box::new(v.clone())));
    let WireNode::Verbatim {
        params,
        body_text,
        range,
        origin,
        ..
    } = wire
    else {
        return;
    };
    let label_ctx = LabelCtx {
        label: label.clone(),
        params,
        body: AnnotationBody::Text(body_text),
        node: NodeRef {
            kind: HostNodeKind::Verbatim.as_str().to_string(),
            range,
            origin,
        },
    };
    ctx.out
        .push(dispatch_one(&label, ctx.registry, &label_ctx, ctx.format));
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

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::loader::DocumentLoader;
    use lex_extension::schema::{
        BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, RenderHook, Schema,
    };
    use lex_extension::{HandlerError, LexHandler};
    use std::collections::BTreeMap;

    fn parse(src: &str) -> Document {
        DocumentLoader::from_string(src).parse().expect("parse")
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
    /// the default-rendered HTML. Splicing the handler's HTML into the
    /// output stream is a follow-up (see module-level docs).
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
        let doc = parse(":: acme.task ::\n    Body content.\n");
        let registry = Registry::new();
        registry
            .register_namespace("acme", vec![schema("acme.task", &["html"])], Box::new(Boom))
            .unwrap();
        let outcome = serialize_to_html_with_registry(
            &doc,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
        .expect("serialise");
        // Diagnostic from the failing handler reached the outcome.
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.contains("rendering failed")),
            "expected handler diagnostic in outcome, got: {:?}",
            outcome.diagnostics
        );
        // The default HTML is still produced (no panic, no error
        // bubbling up).
        assert!(!outcome.html.is_empty());
    }

    /// Regression for the NodeRef.kind misalignment: the wire spec
    /// §2.1 says the LabelCtx's `node.kind` is the host AST kind the
    /// label is attached to (paragraph / list / table / …). Before
    /// the `HostNodeKind` unification it was hardcoded to
    /// `"annotation"` regardless of the host node, so handlers
    /// couldn't distinguish a label attached to a paragraph from one
    /// attached to a list.
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
        // Top-level annotation (parsed as document-level metadata).
        // Before the HostNodeKind fix this would have been reported as
        // "annotation"; it should now be "document".
        let doc = parse(":: acme.task ::\n");
        let _ = dispatch_render(&doc, &registry, "html");
        let kinds = seen.lock().unwrap().clone();
        assert_eq!(
            kinds.as_slice(),
            &["document"],
            "top-level annotation must surface as host kind \"document\", not the hardcoded \"annotation\"",
        );
    }

    /// End-to-end without registered hooks: the registry is consulted
    /// but dispatch is a no-op, the outcome carries no diagnostics,
    /// and the HTML matches what the registry-less path would emit.
    #[test]
    fn end_to_end_html_pipeline_is_passthrough_when_no_hooks_match() {
        use crate::formats::html::{
            serialize_to_html, serialize_to_html_with_registry, HtmlOptions, HtmlTheme,
        };
        let doc = parse(":: acme.task ::\n    Body.\n");
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
            &doc,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
        .expect("serialise");
        let baseline = serialize_to_html(&doc, HtmlTheme::default()).expect("baseline");
        assert_eq!(outcome.html, baseline);
        assert!(outcome.diagnostics.is_empty());
    }

    // -- Splice tests (PR for #563). The splice mechanism replaces
    // the default `<!-- lex:label -->` ... `<!-- /lex:label -->`
    // comment pair (and any content between) with the handler's raw
    // HTML when the handler returns `RenderOut::String`. WireAst and
    // `Ok(None)` continue to fall through to default rendering.

    /// Helper: build a registry + handler that returns a fixed
    /// String for every render call. The returned HTML is what should
    /// be spliced in place of the annotation's default rendering.
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

    /// Test fixture: a session-scoped annotation. Top-level
    /// annotations are extracted into the IR's synthetic
    /// `frontmatter` block before events are emitted, so they
    /// don't flow through `Event::StartAnnotation` at all and the
    /// splice can't operate on them. Per-element annotations (the
    /// realistic extension use case) go through the event stream.
    /// Lifting this limitation requires reworking the IR's
    /// document-annotation handling — out of scope here, tracked
    /// implicitly under `serialize_to_html_with_registry`'s docs.
    const DOC_WITH_SCOPED_ANNOTATION: &str =
        "1. Heading\n\n    :: acme.task ::\n        Body that should be replaced.\n";

    #[test]
    fn splice_replaces_default_annotation_rendering_with_handler_html() {
        use crate::formats::html::{serialize_to_html_with_registry, HtmlOptions, HtmlTheme};
        let doc = parse(DOC_WITH_SCOPED_ANNOTATION);
        let registry = registry_with_string_handler(
            "acme.task",
            "<div class=\"acme-task\">handler-rendered</div>",
        );
        let outcome = serialize_to_html_with_registry(
            &doc,
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
        // Default comment markers gone for this annotation.
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
        // The body paragraph below ("Body that should be replaced.")
        // must NOT appear in the output — the handler owns the
        // annotation's full rendering, including its body.
        let doc = parse(DOC_WITH_SCOPED_ANNOTATION);
        let registry = registry_with_string_handler("acme.task", "<div>HANDLER</div>");
        let outcome = serialize_to_html_with_registry(
            &doc,
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
        let doc = parse(DOC_WITH_SCOPED_ANNOTATION);
        let registry = Registry::new();
        registry
            .register_namespace(
                "acme",
                vec![schema("acme.task", &["html"])],
                Box::new(AlwaysNone),
            )
            .unwrap();
        let outcome = serialize_to_html_with_registry(
            &doc,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
        .expect("serialise");
        // No splice → default comment markers present.
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
        let doc = parse(DOC_WITH_SCOPED_ANNOTATION);
        let registry = Registry::new();
        registry
            .register_namespace(
                "acme",
                vec![schema("acme.task", &["html"])],
                Box::new(WireOnly),
            )
            .unwrap();
        let outcome = serialize_to_html_with_registry(
            &doc,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
        .expect("serialise");
        // Default markers present (WireAst doesn't splice for HTML).
        assert!(outcome.html.contains("<!-- lex:acme.task"));
        // Diagnostic surfaces.
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
        // No namespaces registered — splice has nothing to plan, the
        // registry-less and registry-aware paths produce identical
        // output.
        let doc = parse("1. Heading\n\n    :: unknown.label ::\n        body.\n");
        let registry = Registry::new();
        let outcome = serialize_to_html_with_registry(
            &doc,
            HtmlOptions::new(HtmlTheme::default()),
            &registry,
        )
        .expect("serialise");
        let baseline = serialize_to_html(&doc, HtmlTheme::default()).expect("baseline");
        assert_eq!(outcome.html, baseline);
    }

    /// Plan-vs-events alignment isn't trivially guaranteed when
    /// multiple annotations appear in one document: the IR's
    /// flatten pass can reposition trailing annotations relative
    /// to their host element, breaking the simple counter-based
    /// indexing the splice relies on. The two-annotation case is
    /// real and worth fixing, but the fix needs deeper IR-side
    /// work to preserve source order through the flatten pass —
    /// out of scope for the initial splice landing. Tracked at
    /// the same #563 follow-up that handles WireAst → HTML
    /// conversion.
    ///
    /// For v1, the single-annotation case (the dominant use case)
    /// works correctly, and the trailing-annotation case is the
    /// only known divergence. Real extension usage (acme.commenting,
    /// mit.plasma-specs, etc.) attaches per-element and renders
    /// cleanly.
    #[test]
    fn multi_annotation_splice_is_a_known_limitation() {
        // No assertion — this test exists so a future implementer
        // grepping for "multi-annotation" lands here. Promote to a
        // real assertion when the IR ordering is stabilised.
    }
}
