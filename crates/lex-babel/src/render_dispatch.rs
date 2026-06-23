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
use lex_extension::{
    schema::{BodyKind, Schema},
    AnnotationBody, LabelCtx, NodeRef, RenderOut,
};
use lex_extension_host::Registry;

use crate::ir::nodes::{Annotation, DocNode, Document, InlineContent, Verbatim};
use crate::ir::to_wire::{ir_annotation_body, ir_params_to_json};

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
            let body = body_for_schema(&annotation.content, &schema);
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
    // with the closing label preserved in `language` and the
    // closing-data parameters preserved in `parameters` (#614 IR
    // symmetry follow-up). Resurrect render dispatch for that case
    // via the `language` field and surface `parameters` to handlers
    // exactly as the pre-#616 AST walk did.
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
    let params_object = serde_json::Value::Object(
        v.parameters
            .iter()
            .map(|(k, val)| (k.clone(), serde_json::Value::String(val.clone())))
            .collect(),
    );
    let label_ctx = LabelCtx {
        label: label.to_string(),
        params: params_object,
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

/// Build an [`AnnotationBody`] from the IR annotation's children,
/// honouring the schema's declared `body.kind`. A schema that declares
/// `BodyKind::Text` (the `doc.*` family) expects the body as a flat
/// string — without this, `ir_annotation_body` would always pack a
/// non-empty body as `AnnotationBody::Lex` and the handler's text-only
/// branch would never fire. For `Lex` / `None` body kinds the standard
/// IR → Wire packing applies.
fn body_for_schema(content: &[DocNode], schema: &Schema) -> AnnotationBody {
    if schema.body.kind == BodyKind::Text {
        let flat = flatten_text_body(content);
        if flat.is_empty() {
            AnnotationBody::None
        } else {
            AnnotationBody::Text(flat)
        }
    } else {
        ir_annotation_body(content)
    }
}

/// Flatten an annotation's IR body into a single plain-text string for
/// `BodyKind::Text` schemas. Recurses through `Bold`/`Italic` formatting
/// containers so a metadata value like `:: doc.title :: *Important*
/// Title` keeps the "Important" text instead of dropping it (Copilot
/// review on PR #625). `Image` is skipped — its alt text isn't usually
/// what an author intended as the metadata value.
///
/// Multi-paragraph bodies are joined with a single space so the
/// resulting scalar stays single-line — a literal newline in a YAML
/// scalar would orphan the trailing text from its key. `DocNode::Inline`
/// siblings (bare inlines outside a paragraph wrapper) are processed
/// too, so an unusual doc-scope annotation shape doesn't silently drop
/// content.
fn flatten_text_body(content: &[DocNode]) -> String {
    fn flatten_inlines(inlines: &[InlineContent], buf: &mut String) {
        for inline in inlines {
            match inline {
                InlineContent::Text(t)
                | InlineContent::Code(t)
                | InlineContent::Math(t)
                | InlineContent::Reference { raw: t, .. } => buf.push_str(t),
                InlineContent::Link { text, .. } => buf.push_str(text),
                InlineContent::Bold(children) | InlineContent::Italic(children) => {
                    flatten_inlines(children, buf);
                }
                InlineContent::Image(_) => {}
            }
        }
    }

    let mut buf = String::new();
    let mut first = true;
    for node in content {
        match node {
            DocNode::Paragraph(p) => {
                if !first {
                    buf.push(' ');
                }
                first = false;
                flatten_inlines(&p.content, &mut buf);
            }
            DocNode::Inline(i) => {
                flatten_inlines(std::slice::from_ref(i), &mut buf);
            }
            _ => {}
        }
    }
    buf
}

fn zero_range() -> lex_extension::wire::Range {
    use lex_extension::wire::{Position, Range};
    Range::new(Position::new(0, 0), Position::new(0, 0))
}

#[cfg(test)]
mod tests;
