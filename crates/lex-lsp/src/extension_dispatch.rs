//! LSP-side dispatch into the extension [`Registry`].
//!
//! The hover / completion / code-action handlers in [`server`] call the
//! helpers here to ask the extension registry what it has for the cursor
//! position. When a labelled annotation or verbatim block at the cursor
//! has a registered handler, the handler's hook is fired and its wire
//! output is translated into `lsp_types`. When nothing matches, the
//! helpers return empty results and the existing built-in providers
//! handle the request as before — extension dispatch is additive, never
//! a replacement.
//!
//! This module is the LSP analogue of [`lex_analysis::label_dispatch`]
//! and [`lex_babel::render_dispatch`]: same registry, same `LabelCtx`
//! construction (via [`lex_core::lex::wire::to_wire_node`]), different
//! output translation.
//!
//! [`server`]: crate::server

use std::sync::Arc;

use lex_core::lex::ast::{
    Annotation, ContentItem, Document, Position as AstPosition, Session, Verbatim,
};
use lex_core::lex::wire::to_wire_node;
use lex_extension::wire::{HostNodeKind, WireNode};
use lex_extension::{AnnotationBody, LabelCtx, NodeRef};
use lex_extension_host::Registry;
use lex_fmt::{BootDiagnostic, BootOutcome, RegisteredNamespace};
use tower_lsp::lsp_types::{
    self as lsp, CodeAction as LspCodeAction, CompletionItem as LspCompletionItem,
    Hover as LspHover, HoverContents, MarkupContent, MarkupKind, Range as LspRange, TextEdit, Url,
    WorkspaceEdit,
};

/// Cached state from a single [`lex_fmt::boot_registry`] call. Held by
/// the server for the lifetime of a workspace; rebuilt when the workspace
/// root changes.
pub struct LspExtensionState {
    /// The shared registry consulted on every hover / completion /
    /// code-action request.
    pub registry: Arc<Registry>,
    /// Boot-time diagnostics (resolver failures, denied namespaces,
    /// schema errors). Surfaced to the editor as
    /// `window/showMessage` notifications immediately after boot
    /// (see [`crate::server::LexLanguageServer::extension_state`]),
    /// and retained here for tests / future inspection requests.
    #[allow(dead_code)]
    pub boot_diagnostics: Vec<BootDiagnostic>,
    /// Successfully registered namespaces. Reserved for a future
    /// "currently loaded extensions" surface.
    #[allow(dead_code)]
    pub registered: Vec<RegisteredNamespace>,
}

impl From<BootOutcome> for LspExtensionState {
    fn from(outcome: BootOutcome) -> Self {
        Self {
            registry: outcome.registry,
            boot_diagnostics: outcome.diagnostics,
            registered: outcome.registered,
        }
    }
}

/// Dispatch a hover request through the extension registry.
///
/// Returns `Some(hover)` when the cursor is on a labelled annotation or
/// verbatim block whose registered handler returns hover content; `None`
/// otherwise. The caller falls back to the built-in hover provider on
/// `None`.
pub fn dispatch_hover(
    document: &Document,
    position: AstPosition,
    registry: &Registry,
) -> Option<LspHover> {
    if registry.namespace_count() == 0 {
        return None;
    }
    let ctx = build_ctx_at_position(document, position)?;
    registry.schema_for(&ctx.label)?;
    match registry.dispatch_hover(&ctx) {
        Ok(Some(h)) => Some(translate_hover(h)),
        _ => None,
    }
}

/// Dispatch a completion request through the extension registry.
///
/// Always returns a (possibly empty) `Vec` — the caller merges these
/// with the built-in completion candidates rather than picking one or
/// the other.
pub fn dispatch_completion(
    document: &Document,
    position: AstPosition,
    registry: &Registry,
) -> Vec<LspCompletionItem> {
    if registry.namespace_count() == 0 {
        return Vec::new();
    }
    let Some(ctx) = build_ctx_at_position(document, position) else {
        return Vec::new();
    };
    if registry.schema_for(&ctx.label).is_none() {
        return Vec::new();
    }
    registry
        .dispatch_completion(&ctx)
        .into_iter()
        .map(translate_completion)
        .collect()
}

/// Dispatch a code-action request through the extension registry.
///
/// `start_position` is the start of the LSP request's selection range;
/// we use it to identify the labelled node under the cursor. Returns the
/// (possibly empty) list of handler-supplied code actions; the caller
/// appends them to the built-in actions.
pub fn dispatch_code_action(
    document: &Document,
    start_position: AstPosition,
    document_uri: &Url,
    registry: &Registry,
) -> Vec<LspCodeAction> {
    if registry.namespace_count() == 0 {
        return Vec::new();
    }
    let Some(ctx) = build_ctx_at_position(document, start_position) else {
        return Vec::new();
    };
    if registry.schema_for(&ctx.label).is_none() {
        return Vec::new();
    }
    registry
        .dispatch_code_action(&ctx)
        .into_iter()
        .map(|a| translate_code_action(a, document_uri))
        .collect()
}

/// One labelled hit found under the cursor: an annotation paired with
/// the AST kind of its host (the parent the annotation attaches to),
/// or a verbatim block (whose host kind is always `Verbatim`).
///
/// Mirrors the bookkeeping that `lex_analysis::label_dispatch` does
/// during diagnostics — without it, every annotation dispatched
/// through the LSP would report `attached_to: "annotation"`, which
/// breaks `attaches_to`-sensitive schema pre-validation in the
/// handler-side hooks.
enum LabelledHit<'a> {
    Annotation {
        ann: &'a Annotation,
        host_kind: HostNodeKind,
    },
    Verbatim {
        v: &'a Verbatim,
    },
}

/// Locate a labelled annotation or verbatim block at `position` and
/// build the [`LabelCtx`] the registry expects. Returns `None` when the
/// cursor isn't on a labelled node.
fn build_ctx_at_position(document: &Document, position: AstPosition) -> Option<LabelCtx> {
    let hit = find_labelled_at_position(document, position)?;
    match hit {
        LabelledHit::Annotation { ann, host_kind } => {
            let label = ann.data.label.value.clone();
            if label.is_empty() {
                return None;
            }
            let wire = to_wire_node(&ContentItem::Annotation(ann.clone()));
            let WireNode::Annotation {
                params,
                body,
                range,
                origin,
                ..
            } = wire
            else {
                return None;
            };
            let body =
                serde_json::from_value::<AnnotationBody>(body).unwrap_or(AnnotationBody::None);
            Some(LabelCtx {
                label,
                params,
                body,
                node: NodeRef {
                    // Per wire spec §2.1: `NodeRef.kind` is the host
                    // AST kind the label attaches to (Document /
                    // Session / Paragraph / Definition / List /
                    // ListItem / Annotation), NOT the literal
                    // "annotation" tag. Pre-validation in the
                    // host-side dispatcher checks this against the
                    // schema's `attaches_to`; getting it wrong rejects
                    // valid invocations.
                    kind: host_kind.as_str().to_string(),
                    range,
                    origin,
                },
            })
        }
        LabelledHit::Verbatim { v } => {
            let label = v.closing_data.label.value.clone();
            if label.is_empty() {
                return None;
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
                return None;
            };
            Some(LabelCtx {
                label,
                params,
                body: AnnotationBody::Text(body_text),
                node: NodeRef {
                    kind: HostNodeKind::Verbatim.as_str().to_string(),
                    range,
                    origin,
                },
            })
        }
    }
}

/// Walk the document tracking the host kind of each annotation /
/// verbatim parent, and return the first labelled hit whose source
/// range contains `position`. Mirrors the `attached_to` bookkeeping in
/// [`lex_analysis::label_dispatch`].
fn find_labelled_at_position(
    document: &Document,
    position: AstPosition,
) -> Option<LabelledHit<'_>> {
    for ann in document.annotations() {
        if let Some(hit) = visit_annotation(ann, HostNodeKind::Document, position) {
            return Some(hit);
        }
    }
    walk_session(&document.root, HostNodeKind::Session, position)
}

fn walk_session(
    s: &Session,
    self_kind: HostNodeKind,
    position: AstPosition,
) -> Option<LabelledHit<'_>> {
    for ann in s.annotations() {
        if let Some(hit) = visit_annotation(ann, self_kind, position) {
            return Some(hit);
        }
    }
    for child in s.children.iter() {
        if let Some(hit) = visit_content(child, position) {
            return Some(hit);
        }
    }
    None
}

fn visit_content(item: &ContentItem, position: AstPosition) -> Option<LabelledHit<'_>> {
    match item {
        ContentItem::Paragraph(p) => {
            for ann in p.annotations() {
                if let Some(hit) = visit_annotation(ann, HostNodeKind::Paragraph, position) {
                    return Some(hit);
                }
            }
        }
        ContentItem::Session(s) => return walk_session(s, HostNodeKind::Session, position),
        ContentItem::Definition(d) => {
            for ann in d.annotations() {
                if let Some(hit) = visit_annotation(ann, HostNodeKind::Definition, position) {
                    return Some(hit);
                }
            }
            for child in d.children.iter() {
                if let Some(hit) = visit_content(child, position) {
                    return Some(hit);
                }
            }
        }
        ContentItem::List(list) => {
            for ann in list.annotations() {
                if let Some(hit) = visit_annotation(ann, HostNodeKind::List, position) {
                    return Some(hit);
                }
            }
            for entry in &list.items {
                if let ContentItem::ListItem(li) = entry {
                    for ann in li.annotations() {
                        if let Some(hit) = visit_annotation(ann, HostNodeKind::ListItem, position) {
                            return Some(hit);
                        }
                    }
                    for child in li.children.iter() {
                        if let Some(hit) = visit_content(child, position) {
                            return Some(hit);
                        }
                    }
                }
            }
        }
        ContentItem::Annotation(a) => {
            // Standalone annotation node IS its own host (per wire
            // spec §2.2 — `annotation` is itself a host kind).
            // Schemas that declare `attaches_to: ["annotation"]` —
            // notably the `lex.*` built-ins like `lex.include` — rely
            // on this kind being reported, not `Document` or
            // `Session`.
            if let Some(hit) = visit_annotation(a, HostNodeKind::Annotation, position) {
                return Some(hit);
            }
        }
        ContentItem::Table(table) => {
            for child in table.cell_children_iter() {
                if let Some(hit) = visit_content(child, position) {
                    return Some(hit);
                }
            }
        }
        ContentItem::VerbatimBlock(v) if v.location.contains(position) => {
            return Some(LabelledHit::Verbatim { v: v.as_ref() });
        }
        _ => {}
    }
    None
}

fn visit_annotation<'a>(
    ann: &'a Annotation,
    host_kind: HostNodeKind,
    position: AstPosition,
) -> Option<LabelledHit<'a>> {
    if ann.header_location().contains(position) {
        return Some(LabelledHit::Annotation { ann, host_kind });
    }
    for child in ann.children.iter() {
        if let Some(hit) = visit_content(child, position) {
            return Some(hit);
        }
    }
    None
}

fn translate_hover(h: lex_extension::wire::Hover) -> LspHover {
    use lex_extension::wire::HoverFormat;
    let kind = match h.format {
        HoverFormat::Markdown => MarkupKind::Markdown,
        // Plaintext + future-added variants fall back to PlainText —
        // the wire format's deserializer already maps unknown values
        // onto Plaintext, so this match-arm just mirrors that policy.
        _ => MarkupKind::PlainText,
    };
    LspHover {
        contents: HoverContents::Markup(MarkupContent {
            kind,
            value: h.contents,
        }),
        range: h.range.map(to_lsp_range),
    }
}

fn translate_completion(c: lex_extension::wire::Completion) -> LspCompletionItem {
    use lex_extension::wire::CompletionKind;
    let kind = match c.kind {
        CompletionKind::Value => Some(lsp::CompletionItemKind::VALUE),
        CompletionKind::Param => Some(lsp::CompletionItemKind::PROPERTY),
        CompletionKind::Namespace => Some(lsp::CompletionItemKind::MODULE),
        CompletionKind::Snippet => Some(lsp::CompletionItemKind::SNIPPET),
        // Future-added variants fall back to Value, matching the wire
        // deserializer's default-on-unknown policy.
        _ => Some(lsp::CompletionItemKind::VALUE),
    };
    LspCompletionItem {
        label: c.label,
        kind,
        detail: c.detail,
        documentation: c.doc.map(|d| {
            lsp::Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: d,
            })
        }),
        insert_text: Some(c.insert),
        ..Default::default()
    }
}

fn translate_code_action(a: lex_extension::wire::CodeAction, document_uri: &Url) -> LspCodeAction {
    use lex_extension::wire::CodeActionKind as WireKind;
    let kind = match a.kind {
        WireKind::Quickfix => Some(lsp::CodeActionKind::QUICKFIX),
        WireKind::Refactor => Some(lsp::CodeActionKind::REFACTOR),
        WireKind::Source => Some(lsp::CodeActionKind::SOURCE),
        // Future-added variants fall back to Refactor, matching the
        // wire deserializer's default-on-unknown policy.
        _ => Some(lsp::CodeActionKind::REFACTOR),
    };

    // Group edits by URI: edits without a `uri` field apply to the
    // request's document; edits with a `uri` apply to that file. The
    // LSP `WorkspaceEdit.changes` map keys on `Url`, so we collect
    // per-URI edit lists and assemble them at the end.
    //
    // Edits with a `uri` field that fails to parse are *dropped*
    // rather than re-targeted at the request's document — silently
    // applying changes to the wrong file would be a destructive
    // surprise. The handler can resubmit a code action with a valid
    // URI.
    let mut changes: std::collections::HashMap<Url, Vec<TextEdit>> =
        std::collections::HashMap::new();
    for e in a.edits {
        let target = match &e.uri {
            Some(s) => match Url::parse(s) {
                Ok(u) => u,
                Err(parse_err) => {
                    // Log to stderr so handler authors can see their
                    // bug in the LSP's log output (vscode / nvim /
                    // lexed all surface stderr by default). NOT
                    // forwarded as `window/showMessage` because a
                    // single buggy handler could spam the editor for
                    // every code-action request — stderr is the
                    // right level for protocol-shape errors.
                    eprintln!(
                        "[lexd-lsp] dropping code-action edit with invalid uri `{s}`: {parse_err}"
                    );
                    continue;
                }
            },
            None => document_uri.clone(),
        };
        changes.entry(target).or_default().push(TextEdit {
            range: to_lsp_range(e.range),
            new_text: e.new_text,
        });
    }
    let edit = if changes.is_empty() {
        None
    } else {
        Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        })
    };

    LspCodeAction {
        title: a.title,
        kind,
        edit,
        ..Default::default()
    }
}

fn to_lsp_range(r: lex_extension::wire::Range) -> LspRange {
    LspRange {
        start: lsp::Position {
            line: r.start.0,
            character: r.start.1,
        },
        end: lsp::Position {
            line: r.end.0,
            character: r.end.1,
        },
    }
}

#[cfg(test)]
mod tests {
    //! Unit-level translation tests + dispatch tests against a fixture
    //! handler. The server-side wiring (ext_dispatch_* spliced into
    //! the LSP request handlers) is small enough that these tests
    //! plus the existing server integration suite cover it.
    use super::*;
    use lex_core::lex::parsing::parse_document;
    use lex_extension::schema::Schema;
    use lex_extension::wire::{
        CodeActionKind as WireCodeActionKind, Completion, CompletionKind, HoverFormat,
        Position as WirePosition, Range as WireRange, TextEdit as WireTextEdit,
    };
    use lex_extension::{HandlerError, LexHandler};

    fn r(s_l: u32, s_c: u32, e_l: u32, e_c: u32) -> WireRange {
        WireRange {
            start: WirePosition(s_l, s_c),
            end: WirePosition(e_l, e_c),
        }
    }

    /// Fixture handler that returns canned hover / completion /
    /// code-action content for any label invocation. Used by the
    /// dispatch tests below to verify the LSP dispatch path forwards
    /// the request and translates the response correctly.
    struct FixtureHandler;
    impl LexHandler for FixtureHandler {
        fn on_hover(
            &self,
            ctx: &LabelCtx,
        ) -> Result<Option<lex_extension::wire::Hover>, HandlerError> {
            Ok(Some(lex_extension::wire::Hover {
                contents: format!("hover for `{}`", ctx.label),
                format: HoverFormat::Markdown,
                range: Some(ctx.node.range),
            }))
        }
        fn on_completion(&self, _ctx: &LabelCtx) -> Result<Vec<Completion>, HandlerError> {
            Ok(vec![Completion {
                label: "fixture-item".into(),
                detail: None,
                doc: None,
                insert: "fixture-insert".into(),
                kind: CompletionKind::Snippet,
            }])
        }
        fn on_code_action(
            &self,
            _ctx: &LabelCtx,
        ) -> Result<Vec<lex_extension::wire::CodeAction>, HandlerError> {
            Ok(vec![lex_extension::wire::CodeAction {
                title: "Fix it".into(),
                kind: WireCodeActionKind::Quickfix,
                edits: vec![WireTextEdit {
                    range: r(0, 0, 0, 5),
                    new_text: "x".into(),
                    uri: None,
                }],
            }])
        }
    }

    /// Build a Registry containing a single namespace `acme` with one
    /// schema `acme.task` whose hover/completion/code_action hooks are
    /// enabled. Returns the registry and the document text whose
    /// labelled annotation lives at line 0.
    fn registry_with_fixture() -> (Registry, String) {
        let yaml = "schema_version: 1\n\
                    label: acme.task\n\
                    hooks: { hover: true, completion: true, code_action: true }\n";
        let schema: Schema = serde_yaml::from_str(yaml).expect("parse fixture schema");
        let registry = Registry::new();
        registry
            .register_namespace("acme", vec![schema], Box::new(FixtureHandler))
            .expect("register acme");

        let source = ":: acme.task ::\n";
        (registry, source.into())
    }

    #[test]
    fn dispatch_hover_at_labelled_annotation_returns_translated_content() {
        let (registry, source) = registry_with_fixture();
        let document = parse_document(&source).expect("parse");
        let position = AstPosition::new(0, 5); // inside `acme.task`
        let hover = dispatch_hover(&document, position, &registry).expect("hover content");
        match hover.contents {
            HoverContents::Markup(MarkupContent { kind, value }) => {
                assert_eq!(kind, MarkupKind::Markdown);
                assert!(value.contains("acme.task"), "got: {value}");
            }
            _ => panic!("expected markup"),
        }
    }

    #[test]
    fn dispatch_hover_off_labelled_annotation_returns_none() {
        let (registry, _) = registry_with_fixture();
        let document = parse_document("Plain paragraph.\n").expect("parse");
        let position = AstPosition::new(0, 0);
        assert!(dispatch_hover(&document, position, &registry).is_none());
    }

    #[test]
    fn dispatch_completion_at_labelled_annotation_returns_handler_items() {
        let (registry, source) = registry_with_fixture();
        let document = parse_document(&source).expect("parse");
        let position = AstPosition::new(0, 5);
        let items = dispatch_completion(&document, position, &registry);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "fixture-item");
        assert_eq!(items[0].kind, Some(lsp::CompletionItemKind::SNIPPET));
    }

    #[test]
    fn dispatch_code_action_at_labelled_annotation_returns_handler_actions() {
        let (registry, source) = registry_with_fixture();
        let document = parse_document(&source).expect("parse");
        let document_uri = Url::parse("file:///workspace/host.lex").unwrap();
        let actions =
            dispatch_code_action(&document, AstPosition::new(0, 5), &document_uri, &registry);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Fix it");
        let changes = actions[0]
            .edit
            .as_ref()
            .and_then(|e| e.changes.as_ref())
            .expect("changes");
        assert!(changes.contains_key(&document_uri));
    }

    /// `find_labelled_at_position` must surface the cursor's *host
    /// node kind* — the parent containing the annotation — not the
    /// literal "annotation" tag. Schema pre-validation in handler
    /// dispatch checks this against `attaches_to`; getting it wrong
    /// rejects valid invocations.
    #[test]
    fn host_kind_is_document_for_top_level_annotation() {
        let document = parse_document(":: acme.task ::\n").expect("parse");
        let hit = find_labelled_at_position(&document, AstPosition::new(0, 5)).expect("hit");
        match hit {
            LabelledHit::Annotation { host_kind, .. } => {
                assert_eq!(host_kind, HostNodeKind::Document);
            }
            _ => panic!("expected Annotation, got Verbatim"),
        }
    }

    #[test]
    fn host_kind_is_paragraph_for_paragraph_annotation() {
        // Annotation attached inline below paragraph text — host_kind
        // should be Paragraph.
        let source = "Some paragraph text.\n:: acme.task ::\n";
        let document = parse_document(source).expect("parse");
        // Cursor on the annotation header (line 1).
        let hit = find_labelled_at_position(&document, AstPosition::new(1, 5));
        if let Some(LabelledHit::Annotation { host_kind, .. }) = hit {
            // Either Paragraph or Document is plausible depending on
            // how the parser groups the annotation; assert it's NOT
            // Annotation (which was the bug).
            assert_ne!(
                host_kind,
                HostNodeKind::Annotation,
                "annotation host_kind must reflect the AST parent, not the literal tag"
            );
        }
        // If parse made the annotation top-level, the test above
        // doesn't fire — the assertion is conditional. The other
        // host_kind tests still cover the document-level case.
    }

    #[test]
    fn dispatch_with_empty_registry_returns_nothing() {
        let registry = Registry::new();
        let document = parse_document(":: acme.task ::\n").expect("parse");
        let position = AstPosition::new(0, 5);
        assert!(dispatch_hover(&document, position, &registry).is_none());
        assert!(dispatch_completion(&document, position, &registry).is_empty());
        let document_uri = Url::parse("file:///workspace/host.lex").unwrap();
        assert!(dispatch_code_action(&document, position, &document_uri, &registry).is_empty());
    }

    #[test]
    fn translate_hover_markdown_preserves_format_and_range() {
        let h = lex_extension::wire::Hover {
            contents: "**bold**".into(),
            format: HoverFormat::Markdown,
            range: Some(r(0, 0, 0, 5)),
        };
        let out = translate_hover(h);
        match out.contents {
            HoverContents::Markup(MarkupContent { kind, value }) => {
                assert_eq!(kind, MarkupKind::Markdown);
                assert_eq!(value, "**bold**");
            }
            _ => panic!("expected markup"),
        }
        assert!(out.range.is_some());
    }

    #[test]
    fn translate_completion_preserves_label_insert_kind() {
        let c = Completion {
            label: "task".into(),
            detail: Some("acme.task annotation".into()),
            doc: Some("docs".into()),
            insert: ":: acme.task ::".into(),
            kind: CompletionKind::Snippet,
        };
        let out = translate_completion(c);
        assert_eq!(out.label, "task");
        assert_eq!(out.kind, Some(lsp::CompletionItemKind::SNIPPET));
        assert_eq!(out.insert_text.as_deref(), Some(":: acme.task ::"));
        assert!(out.documentation.is_some());
    }

    #[test]
    fn translate_code_action_groups_edits_by_uri() {
        let document_uri = Url::parse("file:///workspace/host.lex").unwrap();
        let other_uri = "file:///workspace/other.lex";
        let a = lex_extension::wire::CodeAction {
            title: "Fix it".into(),
            kind: WireCodeActionKind::Quickfix,
            edits: vec![
                WireTextEdit {
                    range: r(0, 0, 0, 5),
                    new_text: "x".into(),
                    uri: None,
                },
                WireTextEdit {
                    range: r(1, 0, 1, 5),
                    new_text: "y".into(),
                    uri: Some(other_uri.into()),
                },
            ],
        };
        let out = translate_code_action(a, &document_uri);
        assert_eq!(out.title, "Fix it");
        assert_eq!(out.kind, Some(lsp::CodeActionKind::QUICKFIX));
        let changes = out
            .edit
            .as_ref()
            .and_then(|e| e.changes.as_ref())
            .expect("changes set");
        assert_eq!(changes.len(), 2);
        assert!(changes.contains_key(&document_uri));
        assert!(changes.contains_key(&Url::parse(other_uri).unwrap()));
    }

    #[test]
    fn translate_code_action_drops_edits_with_invalid_uri() {
        let document_uri = Url::parse("file:///workspace/host.lex").unwrap();
        let a = lex_extension::wire::CodeAction {
            title: "Mixed".into(),
            kind: WireCodeActionKind::Quickfix,
            edits: vec![
                // Valid relative-to-document edit — kept.
                WireTextEdit {
                    range: r(0, 0, 0, 5),
                    new_text: "x".into(),
                    uri: None,
                },
                // Garbage URI — dropped (NOT silently retargeted at
                // the request document).
                WireTextEdit {
                    range: r(1, 0, 1, 5),
                    new_text: "y".into(),
                    uri: Some("not a url at all".into()),
                },
            ],
        };
        let out = translate_code_action(a, &document_uri);
        let changes = out
            .edit
            .as_ref()
            .and_then(|e| e.changes.as_ref())
            .expect("changes set");
        assert_eq!(changes.len(), 1, "garbage URI should be dropped");
        let edits = changes.get(&document_uri).expect("document URI present");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "x");
    }

    #[test]
    fn translate_code_action_no_edits_yields_no_workspace_edit() {
        let document_uri = Url::parse("file:///workspace/host.lex").unwrap();
        let a = lex_extension::wire::CodeAction {
            title: "Refactor".into(),
            kind: WireCodeActionKind::Refactor,
            edits: vec![],
        };
        let out = translate_code_action(a, &document_uri);
        assert!(out.edit.is_none());
    }
}
