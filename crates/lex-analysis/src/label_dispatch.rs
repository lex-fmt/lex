//! Visit every labelled annotation / verbatim block in a document and
//! dispatch the registered handler's `on_label` and `on_validate` hooks.
//!
//! The walker is the analysis-side glue between the parser and the
//! extension registry: a label whose namespace is registered goes
//! through schema pre-validation, then (if pre-validation passes) the
//! handler is asked for diagnostics. Schema-level failures and
//! handler-emitted diagnostics both surface as `AnalysisDiagnostic`s on
//! the same channel as the existing footnote / table checks.
//!
//! Bounded extensibility: a label whose namespace is *not* registered
//! is silently ignored — unknown namespaces are not document errors.

use lex_core::lex::ast::{
    Annotation, ContentItem, Document, Range as CoreRange, Session, Verbatim,
};
use lex_core::lex::wire::to_wire_node;
use lex_extension::wire::{HostNodeKind, Range as WireRange, WireNode};
use lex_extension::{schema::Schema, AnnotationBody, LabelCtx, NodeRef};
use lex_extension_host::Registry;

use crate::diagnostics::{
    AnalysisDiagnostic, DiagnosticKind, DiagnosticSeverity, SchemaValidationKind,
};

/// Walk `document` and append diagnostics for every labelled node whose
/// namespace is registered in `registry`. No-op on an empty registry.
pub fn dispatch_labels(
    document: &Document,
    registry: &Registry,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
    if registry.namespace_count() == 0 {
        return;
    }
    // Document-level annotations are parsed before the body, so they
    // come first in the diagnostic stream too.
    for annotation in document.annotations() {
        visit_annotation(annotation, HostNodeKind::Document, registry, diagnostics);
    }
    walk_session(&document.root, HostNodeKind::Session, registry, diagnostics);
}

fn walk_session(
    session: &Session,
    self_kind: HostNodeKind,
    registry: &Registry,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
    for annotation in session.annotations() {
        visit_annotation(annotation, self_kind, registry, diagnostics);
    }
    for child in session.children.iter() {
        visit_content(child, HostNodeKind::Session, registry, diagnostics);
    }
}

fn visit_content(
    item: &ContentItem,
    parent_kind: HostNodeKind,
    registry: &Registry,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
    match item {
        ContentItem::Paragraph(p) => {
            for ann in p.annotations() {
                visit_annotation(ann, HostNodeKind::Paragraph, registry, diagnostics);
            }
        }
        ContentItem::Session(s) => walk_session(s, HostNodeKind::Session, registry, diagnostics),
        ContentItem::Definition(def) => {
            for ann in def.annotations() {
                visit_annotation(ann, HostNodeKind::Definition, registry, diagnostics);
            }
            for child in def.children.iter() {
                visit_content(child, HostNodeKind::Definition, registry, diagnostics);
            }
        }
        ContentItem::List(list) => {
            // List-level annotations attach to the list itself, NOT
            // to its items.
            for ann in list.annotations() {
                visit_annotation(ann, HostNodeKind::List, registry, diagnostics);
            }
            for entry in &list.items {
                if let ContentItem::ListItem(li) = entry {
                    for ann in li.annotations() {
                        visit_annotation(ann, HostNodeKind::ListItem, registry, diagnostics);
                    }
                    for child in li.children.iter() {
                        visit_content(child, HostNodeKind::ListItem, registry, diagnostics);
                    }
                }
            }
        }
        ContentItem::Annotation(a) => {
            visit_annotation(a, parent_kind, registry, diagnostics);
        }
        ContentItem::VerbatimBlock(v) => {
            visit_verbatim(v, registry, diagnostics);
            for ann in v.annotations() {
                visit_annotation(ann, HostNodeKind::Verbatim, registry, diagnostics);
            }
        }
        ContentItem::Table(t) => {
            for ann in t.annotations() {
                visit_annotation(ann, HostNodeKind::Table, registry, diagnostics);
            }
        }
        _ => {}
    }
}

fn visit_annotation(
    annotation: &Annotation,
    attached_to: HostNodeKind,
    registry: &Registry,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
    let label = annotation.data.label.value.clone();
    let Some(schema) = registry.schema_for(&label) else {
        // Unknown label. If the namespace IS registered, this is an
        // error worth surfacing — the namespace owner doesn't declare
        // a label by this name. If the namespace is unregistered we
        // pass through silently (bounded extensibility: unknown
        // namespaces are never document errors).
        if let Some((ns, _)) = label.split_once('.') {
            if registry.is_namespace_healthy(ns) {
                diagnostics.push(AnalysisDiagnostic {
                    range: annotation.location.clone(),
                    severity: DiagnosticSeverity::Error,
                    kind: DiagnosticKind::SchemaValidation(SchemaValidationKind::UnknownLabel),
                    message: format!(
                        "label `{label}` is not declared in registered namespace `{ns}`"
                    ),
                });
            }
        }
        return;
    };

    // Build a wire-shaped LabelCtx via the lex-core codec, then run
    // schema pre-validation against it before bothering the handler.
    let wire = to_wire_node(&ContentItem::Annotation(annotation.clone()));
    let WireNode::Annotation {
        label: _,
        params,
        body,
        range,
        origin,
    } = wire
    else {
        return;
    };

    let body = match serde_json::from_value::<AnnotationBody>(body.clone()) {
        Ok(b) => b,
        Err(_) => AnnotationBody::None,
    };

    let ctx = LabelCtx {
        label: label.clone(),
        params: params.clone(),
        body,
        node: NodeRef {
            kind: "annotation".into(),
            range,
            origin,
        },
    };

    if let Some(diag) = pre_validate(&schema, &ctx, attached_to, &annotation.location) {
        diagnostics.push(diag);
        return;
    }

    // on_label is a notification (no return); fire-and-forget.
    if schema.hooks.label {
        registry.dispatch_label(&ctx);
    }

    let namespace = label
        .split_once('.')
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| label.clone());

    if schema.hooks.validate {
        for d in registry.dispatch_validate(&ctx) {
            diagnostics.push(handler_diagnostic_to_analysis(
                d,
                namespace.clone(),
                annotation.location.clone(),
            ));
        }
    }

    // Register-level root diagnostics (panics, namespace disabled).
    for d in registry.take_root_diagnostics() {
        diagnostics.push(handler_diagnostic_to_analysis(
            d,
            namespace.clone(),
            annotation.location.clone(),
        ));
    }
}

fn visit_verbatim(v: &Verbatim, registry: &Registry, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    let label = v.closing_data.label.value.clone();
    if label.is_empty() {
        return;
    }
    let Some(schema) = registry.schema_for(&label) else {
        return;
    };
    if !schema.verbatim_label {
        // Schema declares the label is annotation-only; using it as a
        // verbatim closing is a schema-validation error.
        diagnostics.push(AnalysisDiagnostic {
            range: v.location.clone(),
            severity: DiagnosticSeverity::Error,
            kind: DiagnosticKind::SchemaValidation(SchemaValidationKind::BadAttachment),
            message: format!(
                "label `{label}` is not declared as a verbatim closing (verbatim_label: false)"
            ),
        });
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
    let ctx = LabelCtx {
        label: label.clone(),
        params,
        body: AnnotationBody::Text(body_text),
        node: NodeRef {
            kind: "verbatim".into(),
            range,
            origin,
        },
    };

    let namespace = label
        .split_once('.')
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| label.clone());

    if schema.hooks.label {
        registry.dispatch_label(&ctx);
    }
    if schema.hooks.validate {
        for d in registry.dispatch_validate(&ctx) {
            diagnostics.push(handler_diagnostic_to_analysis(
                d,
                namespace.clone(),
                v.location.clone(),
            ));
        }
    }
    for d in registry.take_root_diagnostics() {
        diagnostics.push(handler_diagnostic_to_analysis(
            d,
            namespace.clone(),
            v.location.clone(),
        ));
    }
}

/// Schema pre-validation: the six checks the analyser owns before
/// dispatch reaches the handler. Per the wire spec / proposal §13.2,
/// these are:
///
/// 1. namespace registered (caller already filtered)
/// 2. label present in the namespace's schema (already true once
///    `schema_for` returned `Some`)
/// 3. required params present
/// 4. param types match schema
/// 5. attachment kind permitted by `attaches_to`
/// 6. body shape matches `body.kind` and `body.presence`
///
/// Returns `Some(diag)` on the first failure; `None` if the invocation
/// is well-formed and the handler may run.
fn pre_validate(
    schema: &Schema,
    ctx: &LabelCtx,
    attached_to: HostNodeKind,
    range: &CoreRange,
) -> Option<AnalysisDiagnostic> {
    use lex_extension::schema::{BodyKind, BodyPresence};

    // 5. attaches_to (cheaper than param walks; do first).
    let attached_str = attached_to.as_str();
    if !schema.attaches_to.is_empty() && !schema.attaches_to.iter().any(|kind| kind == attached_str)
    {
        return Some(AnalysisDiagnostic {
            range: range.clone(),
            severity: DiagnosticSeverity::Error,
            kind: DiagnosticKind::SchemaValidation(SchemaValidationKind::BadAttachment),
            message: format!(
                "label `{}` is not permitted on `{attached_str}` (attaches_to: {})",
                schema.label,
                schema.attaches_to.join(", ")
            ),
        });
    }

    // 3 + 4. required params present, types match.
    let params_obj = ctx.params.as_object();
    for (name, spec) in &schema.params {
        let provided = params_obj.and_then(|m| m.get(name));
        match (provided, spec.required) {
            (None, true) => {
                return Some(AnalysisDiagnostic {
                    range: range.clone(),
                    severity: DiagnosticSeverity::Error,
                    kind: DiagnosticKind::SchemaValidation(SchemaValidationKind::MissingParam),
                    message: format!(
                        "label `{}` is missing required param `{name}`",
                        schema.label
                    ),
                });
            }
            (None, false) => continue,
            (Some(value), _) => {
                // The lex parser stores all param values as strings;
                // for typed schemas we accept either the string form
                // (e.g., "42" for an int) or the JSON-typed form (a
                // bare 42). The pragmatic check: a string value
                // should be parseable as the declared type. Bool +
                // int + float get a parse-attempt; enum gets a
                // membership check.
                if let Some(diag) = check_param_type(name, value, spec, schema, range) {
                    return Some(diag);
                }
            }
        }
    }

    // 6. body shape.
    //
    // Empty bodies are treated as matching any declared `body.kind`.
    // The wire codec may emit a `Lex { children: [<empty paragraph>] }`
    // even for marker-form annotations (`:: foo ::`) that semantically
    // have no body, so we normalise by ignoring empty content rather
    // than punishing handlers for a parser quirk. The presence rule
    // below still fires when the schema declares `body.presence: required`.
    let body_effectively_empty = body_is_empty(&ctx.body);
    if !body_effectively_empty {
        let kind_matches = match (&ctx.body, schema.body.kind) {
            (AnnotationBody::Text(_), BodyKind::Text) => true,
            (AnnotationBody::Lex { .. }, BodyKind::Lex) => true,
            // `body.kind: none` with a non-empty body is the unambiguous
            // mismatch — the handler said "no body" but one was passed.
            (_, BodyKind::None) => false,
            // Cross-shape (Text body declared as lex, or vice versa) is
            // a genuine mismatch — the handler will see the wrong shape.
            _ => false,
        };
        if !kind_matches {
            return Some(AnalysisDiagnostic {
                range: range.clone(),
                severity: DiagnosticSeverity::Error,
                kind: DiagnosticKind::SchemaValidation(SchemaValidationKind::BodyShapeMismatch),
                message: format!(
                    "label `{}` body does not match declared body.kind: {:?}",
                    schema.label, schema.body.kind
                ),
            });
        }
    }
    if matches!(schema.body.presence, BodyPresence::Required)
        && body_effectively_empty
        && !matches!(schema.body.kind, BodyKind::None)
    {
        return Some(AnalysisDiagnostic {
            range: range.clone(),
            severity: DiagnosticSeverity::Error,
            kind: DiagnosticKind::SchemaValidation(SchemaValidationKind::BodyShapeMismatch),
            message: format!(
                "label `{}` declares body.presence: required but no body was provided",
                schema.label
            ),
        });
    }

    None
}

/// True when `body` carries no semantically meaningful content. Used to
/// avoid false body-shape mismatches when the wire codec emits an empty
/// paragraph for marker-form annotations.
fn body_is_empty(body: &AnnotationBody) -> bool {
    match body {
        AnnotationBody::None => true,
        AnnotationBody::Text(s) => s.trim().is_empty(),
        AnnotationBody::Lex { children } => {
            children.is_empty() || children.iter().all(is_blank_wire_node)
        }
    }
}

fn is_blank_wire_node(node: &WireNode) -> bool {
    match node {
        WireNode::Paragraph { inlines, .. } => inlines.is_empty(),
        WireNode::Blank { .. } => true,
        _ => false,
    }
}

fn check_param_type(
    name: &str,
    value: &serde_json::Value,
    spec: &lex_extension::schema::ParamSpec,
    schema: &Schema,
    range: &CoreRange,
) -> Option<AnalysisDiagnostic> {
    use lex_extension::schema::ParamType;
    let mismatch = |reason: String| AnalysisDiagnostic {
        range: range.clone(),
        severity: DiagnosticSeverity::Error,
        kind: DiagnosticKind::SchemaValidation(SchemaValidationKind::ParamTypeMismatch),
        message: format!(
            "label `{}` param `{name}` type mismatch: {reason}",
            schema.label
        ),
    };
    match spec.ty {
        ParamType::String => {
            if !value.is_string() {
                return Some(mismatch(format!("expected string, got {value}")));
            }
        }
        ParamType::Bool => {
            // Accept JSON bool or string "true"/"false".
            if !value.is_boolean()
                && !matches!(
                    value.as_str().map(str::to_ascii_lowercase).as_deref(),
                    Some("true") | Some("false")
                )
            {
                return Some(mismatch(format!("expected bool, got {value}")));
            }
        }
        ParamType::Int => {
            let ok = value.is_i64()
                || value.is_u64()
                || value
                    .as_str()
                    .map(|s| s.parse::<i64>().is_ok())
                    .unwrap_or(false);
            if !ok {
                return Some(mismatch(format!("expected int, got {value}")));
            }
        }
        ParamType::Float => {
            let ok = value.is_number()
                || value
                    .as_str()
                    .map(|s| s.parse::<f64>().is_ok())
                    .unwrap_or(false);
            if !ok {
                return Some(mismatch(format!("expected float, got {value}")));
            }
        }
        ParamType::Enum => {
            let s = value.as_str().unwrap_or("").to_string();
            let known = spec.values.iter().any(|v| v.name == s);
            if !known {
                return Some(mismatch(format!(
                    "value {value} is not in declared enum (allowed: {})",
                    spec.values
                        .iter()
                        .map(|v| v.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
        // ParamType is #[non_exhaustive]; future variants conservatively
        // accept until taught.
        _ => {}
    }
    None
}

/// Map a handler-emitted `lex_extension::Diagnostic` into an
/// `AnalysisDiagnostic`. Falls back to the labelled-node range when
/// the handler didn't supply a meaningful range (defensive against a
/// handler returning the default zero range).
fn handler_diagnostic_to_analysis(
    diag: lex_extension::Diagnostic,
    namespace_label: String,
    fallback_range: CoreRange,
) -> AnalysisDiagnostic {
    let range = wire_range_to_core(&diag.range, &fallback_range);
    let severity = match diag.severity {
        lex_extension::DiagnosticSeverity::Error => DiagnosticSeverity::Error,
        lex_extension::DiagnosticSeverity::Warning => DiagnosticSeverity::Warning,
        lex_extension::DiagnosticSeverity::Info => DiagnosticSeverity::Info,
        lex_extension::DiagnosticSeverity::Hint => DiagnosticSeverity::Hint,
        // DiagnosticSeverity is #[non_exhaustive]; future wire variants
        // fall back to Info per the wire spec.
        _ => DiagnosticSeverity::Info,
    };
    AnalysisDiagnostic {
        range,
        severity,
        kind: DiagnosticKind::Handler {
            namespace: namespace_label,
            code: diag.code,
        },
        message: diag.message,
    }
}

/// Convert a wire-format range into a lex-core range. The wire range
/// only carries 0-indexed line/col, not byte spans, so the byte span
/// in the result is empty (downstream surfaces — LSP — don't consume
/// the byte span anyway).
fn wire_range_to_core(wire: &WireRange, fallback: &CoreRange) -> CoreRange {
    use lex_core::lex::ast::Position as CorePosition;
    let zero = wire.start.0 == 0 && wire.start.1 == 0 && wire.end.0 == 0 && wire.end.1 == 0;
    if zero {
        return fallback.clone();
    }
    CoreRange::new(
        0..0,
        CorePosition::new(wire.start.0 as usize, wire.start.1 as usize),
        CorePosition::new(wire.end.0 as usize, wire.end.1 as usize),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::loader::DocumentLoader;

    fn parse(src: &str) -> Document {
        DocumentLoader::from_string(src)
            .parse()
            .expect("parse fixture")
    }
    use lex_extension::schema::{
        BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, ParamSpec, ParamType, Schema,
    };
    use lex_extension::{HandlerError, LexHandler};
    use std::collections::BTreeMap;

    fn schema(label: &str, attaches_to: Vec<&str>, hooks: HookSet) -> Schema {
        Schema {
            schema_version: 1,
            label: label.into(),
            description: None,
            params: BTreeMap::new(),
            attaches_to: attaches_to.into_iter().map(String::from).collect(),
            body: BodyShape {
                kind: BodyKind::None,
                presence: BodyPresence::Optional,
                description: None,
            },
            verbatim_label: false,
            capabilities: Capabilities::default(),
            hooks,
            handler: None,
        }
    }

    struct EchoValidate;
    impl LexHandler for EchoValidate {
        fn on_validate(
            &self,
            ctx: &LabelCtx,
        ) -> Result<Vec<lex_extension::Diagnostic>, HandlerError> {
            Ok(vec![lex_extension::Diagnostic {
                severity: lex_extension::DiagnosticSeverity::Warning,
                message: format!("validate {}", ctx.label),
                range: ctx.node.range,
                code: Some("test.code".into()),
                related: Vec::new(),
            }])
        }
    }

    #[test]
    fn empty_registry_is_a_noop() {
        let doc = parse(":: acme.task ::\n");
        let registry = Registry::new();
        let mut diags = Vec::new();
        dispatch_labels(&doc, &registry, &mut diags);
        assert!(diags.is_empty());
    }

    #[test]
    fn unknown_namespace_silently_passes_through() {
        let doc = parse(":: foo.bar ::\n");
        let registry = Registry::new();
        let acme = schema(
            "acme.task",
            vec!["paragraph", "annotation"],
            HookSet {
                validate: true,
                ..HookSet::default()
            },
        );
        registry
            .register_namespace("acme", vec![acme], Box::new(EchoValidate))
            .unwrap();
        let mut diags = Vec::new();
        dispatch_labels(&doc, &registry, &mut diags);
        // foo.bar is not registered → silent.
        assert!(diags.is_empty());
    }

    #[test]
    fn registered_label_dispatches_validate_and_collects_diagnostic() {
        let doc = parse(":: acme.task ::\n");
        let registry = Registry::new();
        let s = schema(
            "acme.task",
            vec!["annotation", "document"],
            HookSet {
                validate: true,
                ..HookSet::default()
            },
        );
        registry
            .register_namespace("acme", vec![s], Box::new(EchoValidate))
            .unwrap();
        let mut diags = Vec::new();
        dispatch_labels(&doc, &registry, &mut diags);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("acme.task"));
        match &diags[0].kind {
            DiagnosticKind::Handler { namespace, code } => {
                assert_eq!(namespace, "acme");
                assert_eq!(code.as_deref(), Some("test.code"));
            }
            other => panic!("expected Handler, got {other:?}"),
        }
    }

    #[test]
    fn missing_required_param_produces_schema_diagnostic_without_dispatching() {
        let mut params = BTreeMap::new();
        params.insert(
            "src".into(),
            ParamSpec {
                ty: ParamType::String,
                required: true,
                default: None,
                description: None,
                pattern: None,
                values: Vec::new(),
            },
        );
        let s = Schema {
            schema_version: 1,
            label: "acme.thing".into(),
            description: None,
            params,
            attaches_to: vec!["annotation".into(), "document".into()],
            body: BodyShape {
                kind: BodyKind::None,
                presence: BodyPresence::Optional,
                description: None,
            },
            verbatim_label: false,
            capabilities: Capabilities::default(),
            hooks: HookSet {
                validate: true,
                ..HookSet::default()
            },
            handler: None,
        };
        // Handler that should NOT be called — schema pre-validation
        // catches the missing param before dispatch.
        struct Boom;
        impl LexHandler for Boom {
            fn on_validate(
                &self,
                _ctx: &LabelCtx,
            ) -> Result<Vec<lex_extension::Diagnostic>, HandlerError> {
                panic!("handler must not be called; schema pre-validation failed");
            }
        }
        let registry = Registry::new();
        registry
            .register_namespace("acme", vec![s], Box::new(Boom))
            .unwrap();
        let doc = parse(":: acme.thing ::\n");
        let mut diags = Vec::new();
        dispatch_labels(&doc, &registry, &mut diags);
        assert_eq!(diags.len(), 1);
        match &diags[0].kind {
            DiagnosticKind::SchemaValidation(SchemaValidationKind::MissingParam) => {}
            other => panic!("expected MissingParam, got {other:?}"),
        }
        assert!(diags[0].message.contains("src"));
    }

    #[test]
    fn bad_attachment_produces_schema_diagnostic() {
        // Schema only allows attachment to definitions; we attach
        // it to a paragraph.
        let s = schema(
            "acme.def",
            vec!["definition"],
            HookSet {
                validate: true,
                ..HookSet::default()
            },
        );
        let registry = Registry::new();
        registry
            .register_namespace("acme", vec![s], Box::new(EchoValidate))
            .unwrap();
        // Paragraph-attached annotation:
        let doc = parse("Some paragraph.\n:: acme.def ::\n");
        let mut diags = Vec::new();
        dispatch_labels(&doc, &registry, &mut diags);
        // We expect a bad-attachment diagnostic.
        assert!(
            diags.iter().any(|d| matches!(
                d.kind,
                DiagnosticKind::SchemaValidation(SchemaValidationKind::BadAttachment)
            )),
            "expected at least one BadAttachment diag, got: {diags:?}"
        );
    }

    /// Regression for the kinds-misalignment bug: a schema that
    /// attaches to `document` should match a top-level annotation,
    /// not get a BadAttachment diagnostic. Prior to the
    /// `HostNodeKind` unification the loader rejected `document`
    /// schemas outright; with the fix the loader accepts them and
    /// the walker emits `HostNodeKind::Document` for top-level
    /// annotations, so the two sides agree.
    #[test]
    fn document_level_annotation_matches_document_attaches_to() {
        let s = schema(
            "acme.docmeta",
            vec!["document"],
            HookSet {
                validate: true,
                ..HookSet::default()
            },
        );
        let registry = Registry::new();
        registry
            .register_namespace("acme", vec![s], Box::new(EchoValidate))
            .unwrap();
        // Top-level annotation, parsed as a document-level one.
        let doc = parse(":: acme.docmeta ::\n");
        let mut diags = Vec::new();
        dispatch_labels(&doc, &registry, &mut diags);
        // Expect handler-emitted diagnostic (no BadAttachment).
        assert!(
            !diags.iter().any(|d| matches!(
                d.kind,
                DiagnosticKind::SchemaValidation(SchemaValidationKind::BadAttachment)
            )),
            "document-level annotation should match attaches_to: [document], got: {diags:?}"
        );
    }
}
