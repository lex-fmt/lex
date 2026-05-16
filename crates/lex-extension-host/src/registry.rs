//! Namespace registry and hook dispatch.
//!
//! The [`Registry`] owns the set of registered namespaces, indexes them by
//! label name, and provides one dispatch helper per [`LexHandler`] hook.
//! Every dispatch wraps the handler call so that:
//!
//! - A returned `Err(HandlerError)` becomes a synthetic [`Diagnostic`] at
//!   the labelled node's range (analyse-equivalent surface).
//! - A panic inside the handler is caught, the namespace is marked
//!   unhealthy, and a single root-level diagnostic is recorded; subsequent
//!   dispatches to the same namespace short-circuit and add no further
//!   diagnostics for the rest of the session.
//! - Lookups for unregistered labels return `None` / empty without error.
//!
//! Thread-safety: the registry is intended to be shared across analysis,
//! render, and LSP-request paths. Internal state lives behind a
//! [`RwLock`]; dispatch methods take `&self`.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::RwLock;

use lex_extension::{
    CodeAction, Completion, Diagnostic, DiagnosticSeverity, Format, FormatCtx, HandlerError, Hover,
    LabelCtx, LexAnnotationOut, LexHandler, Range, RenderOut, Schema, WireNode,
};

/// The namespace registry.
///
/// Construct with [`Registry::new`], populate with
/// [`Registry::register_namespace`], and pass `&self` (or
/// `Arc<Registry>`) to dispatch sites.
pub struct Registry {
    inner: RwLock<Inner>,
}

struct Inner {
    namespaces: HashMap<String, Namespace>,
    /// Fast lookup from a fully-qualified label (e.g. `"acme.task"`) to the
    /// owning namespace name (e.g. `"acme"`).
    label_to_namespace: HashMap<String, String>,
    /// Document-root diagnostics emitted when a namespace is disabled
    /// after a panic. Surfaced via [`Registry::root_diagnostics`].
    root_diagnostics: Vec<Diagnostic>,
}

struct Namespace {
    /// Schemas keyed by full label name (`"acme.task"`, not `"task"`).
    schemas: HashMap<String, Schema>,
    handler: Box<dyn LexHandler>,
    healthy: bool,
}

/// Errors returned by [`Registry::register_namespace`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RegistryError {
    /// A namespace with this name was already registered.
    NamespaceAlreadyRegistered { namespace: String },
    /// A label string is already registered. With v1's strict prefix
    /// enforcement (no nested namespaces) this is defence-in-depth: the
    /// [`LabelOutsideNamespace`](Self::LabelOutsideNamespace) check
    /// usually fires first.
    LabelAlreadyRegistered {
        label: String,
        registered_in: String,
    },
    /// A schema's `label` field does not start with `"<namespace>."`. The
    /// fully-qualified label must live inside the namespace it is
    /// registered under.
    LabelOutsideNamespace { label: String, namespace: String },
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::NamespaceAlreadyRegistered { namespace } => {
                write!(f, "namespace `{namespace}` is already registered")
            }
            RegistryError::LabelAlreadyRegistered {
                label,
                registered_in,
            } => write!(
                f,
                "label `{label}` is already registered in namespace `{registered_in}`"
            ),
            RegistryError::LabelOutsideNamespace { label, namespace } => write!(
                f,
                "label `{label}` does not belong to namespace `{namespace}`"
            ),
        }
    }
}

impl std::error::Error for RegistryError {}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                namespaces: HashMap::new(),
                label_to_namespace: HashMap::new(),
                root_diagnostics: Vec::new(),
            }),
        }
    }

    /// Register a namespace, its schemas, and the handler that backs all
    /// labels in the namespace.
    ///
    /// Fails if the namespace is already registered, if any of the schemas
    /// declares a label outside the namespace's prefix, or if any label
    /// collides with one already registered (in any namespace).
    pub fn register_namespace(
        &self,
        namespace: impl Into<String>,
        schemas: Vec<Schema>,
        handler: Box<dyn LexHandler>,
    ) -> Result<(), RegistryError> {
        let namespace = namespace.into();
        let mut inner = self.inner.write().expect("registry poisoned");

        if inner.namespaces.contains_key(&namespace) {
            return Err(RegistryError::NamespaceAlreadyRegistered { namespace });
        }

        let prefix = format!("{namespace}.");
        let mut schema_map = HashMap::with_capacity(schemas.len());
        for schema in &schemas {
            if schema.label != namespace && !schema.label.starts_with(&prefix) {
                return Err(RegistryError::LabelOutsideNamespace {
                    label: schema.label.clone(),
                    namespace,
                });
            }
            if let Some(existing) = inner.label_to_namespace.get(&schema.label) {
                return Err(RegistryError::LabelAlreadyRegistered {
                    label: schema.label.clone(),
                    registered_in: existing.clone(),
                });
            }
        }

        for schema in schemas {
            inner
                .label_to_namespace
                .insert(schema.label.clone(), namespace.clone());
            schema_map.insert(schema.label.clone(), schema);
        }

        inner.namespaces.insert(
            namespace,
            Namespace {
                schemas: schema_map,
                handler,
                healthy: true,
            },
        );

        Ok(())
    }

    /// Number of registered namespaces. Useful for tests and diagnostics.
    pub fn namespace_count(&self) -> usize {
        self.inner
            .read()
            .expect("registry poisoned")
            .namespaces
            .len()
    }

    /// Return the schema for a label, if any.
    pub fn schema_for(&self, label: &str) -> Option<Schema> {
        let inner = self.inner.read().expect("registry poisoned");
        let ns_name = inner.label_to_namespace.get(label)?;
        let ns = inner.namespaces.get(ns_name)?;
        ns.schemas.get(label).cloned()
    }

    /// Whether a namespace is registered and currently healthy (no panic
    /// has disabled it).
    pub fn is_namespace_healthy(&self, namespace: &str) -> bool {
        self.inner
            .read()
            .expect("registry poisoned")
            .namespaces
            .get(namespace)
            .is_some_and(|n| n.healthy)
    }

    /// Diagnostics accumulated at the document root — currently emitted
    /// when a namespace is disabled after a panic. Cleared on read.
    pub fn take_root_diagnostics(&self) -> Vec<Diagnostic> {
        std::mem::take(
            &mut self
                .inner
                .write()
                .expect("registry poisoned")
                .root_diagnostics,
        )
    }

    /// Dispatch [`LexHandler::on_label`].
    ///
    /// `on_label` is a notification — the trait method returns `()`, so
    /// the handler has no way to surface an error. The closure passed to
    /// [`Self::dispatch`] therefore always returns `Ok(())`; only panics
    /// can fail this path, and `dispatch` catches them and disables the
    /// namespace through the same machinery as the other hooks.
    pub fn dispatch_label(&self, ctx: &LabelCtx) {
        let _ = self.dispatch(ctx, |h| {
            h.on_label(ctx);
            Ok(()) as Result<(), HandlerError>
        });
    }

    /// Dispatch [`LexHandler::on_validate`]; folds errors and panics into
    /// diagnostics.
    pub fn dispatch_validate(&self, ctx: &LabelCtx) -> Vec<Diagnostic> {
        match self.dispatch(ctx, |h| h.on_validate(ctx)) {
            Ok(Some(diagnostics)) => diagnostics,
            Ok(None) => Vec::new(),
            Err(diag) => vec![diag],
        }
    }

    /// Dispatch [`LexHandler::on_resolve`].
    pub fn dispatch_resolve(&self, ctx: &LabelCtx) -> Result<Option<WireNode>, Diagnostic> {
        match self.dispatch(ctx, |h| h.on_resolve(ctx)) {
            Ok(Some(node)) => Ok(node),
            Ok(None) => Ok(None),
            Err(diag) => Err(diag),
        }
    }

    /// Dispatch [`LexHandler::on_resolve`] and return the raw
    /// [`HandlerError`] on failure rather than the cooked
    /// [`Diagnostic`].
    ///
    /// The resolve pass uses this so it can preserve full error
    /// fidelity — specifically, the numeric `code` on
    /// `HandlerError::Custom` that `dispatch_resolve` collapses into
    /// the generic `"handler.custom"` diagnostic code. Panics in the
    /// handler are still caught and folded into a synthetic
    /// `HandlerError::Internal` (so the namespace stays disabled
    /// for the rest of the session, matching `dispatch_resolve`).
    pub fn dispatch_resolve_raw(&self, ctx: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
        match self.dispatch_raw(ctx, |h| h.on_resolve(ctx)) {
            Ok(Some(node)) => Ok(node),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Dispatch [`LexHandler::on_render`].
    pub fn dispatch_render(
        &self,
        ctx: &LabelCtx,
        format: Format,
    ) -> Result<Option<RenderOut>, Diagnostic> {
        match self.dispatch(ctx, |h| h.on_render(ctx, format.clone())) {
            Ok(Some(out)) => Ok(out),
            Ok(None) => Ok(None),
            Err(diag) => Err(diag),
        }
    }

    /// Dispatch [`LexHandler::on_ir_build`] — the IR-construction
    /// lifecycle hook (#615 unified surface). The host invokes this
    /// while building its in-memory IR from the parsed source; the
    /// returned wire node is consumed by the IR builder, not spliced
    /// into the host AST. `Ok(None)` means "no handler is registered
    /// for this label, the namespace is unhealthy, or the handler
    /// declined" — the host falls back to its generic
    /// verbatim/annotation IR for the label in every case.
    ///
    /// The two `dispatch_*` hooks for content-substitution sit on
    /// different lifecycle phases by design:
    ///
    /// - `dispatch_resolve` — AST splice (`lex.include` etc.). Runs
    ///   during the resolve phase, replaces nodes in the host AST.
    /// - `dispatch_ir_build` — typed IR hydration (`lex.tabular.table`,
    ///   `lex.media.*` etc.). Runs during IR build, produces typed
    ///   wire nodes consumed by the IR builder. Decoupled from
    ///   parsing, so a buggy or slow handler can't corrupt the parser.
    pub fn dispatch_ir_build(&self, ctx: &LabelCtx) -> Result<Option<WireNode>, Diagnostic> {
        match self.dispatch(ctx, |h| h.on_ir_build(ctx)) {
            Ok(Some(node)) => Ok(node),
            Ok(None) => Ok(None),
            Err(diag) => Err(diag),
        }
    }

    /// Dispatch [`LexHandler::on_hover`].
    pub fn dispatch_hover(&self, ctx: &LabelCtx) -> Result<Option<Hover>, Diagnostic> {
        match self.dispatch(ctx, |h| h.on_hover(ctx)) {
            Ok(Some(hover)) => Ok(hover),
            Ok(None) => Ok(None),
            Err(diag) => Err(diag),
        }
    }

    /// Dispatch [`LexHandler::on_completion`].
    pub fn dispatch_completion(&self, ctx: &LabelCtx) -> Vec<Completion> {
        match self.dispatch(ctx, |h| h.on_completion(ctx)) {
            Ok(Some(items)) => items,
            Ok(None) => Vec::new(),
            Err(_) => Vec::new(),
        }
    }

    /// Dispatch [`LexHandler::on_format`] — the reverse hook
    /// introduced by #570 Phase 4 (see
    /// `comms/specs/proposals/lex-extension-wire.lex` §4.8).
    ///
    /// Returns `Ok(Some(LexAnnotationOut))` when a handler produced
    /// structured output, `Ok(None)` when no handler is registered for
    /// the label / the handler returned its default fallback / the
    /// namespace is unhealthy, and `Err(diag)` when the handler errored
    /// or panicked. The caller decides whether to retry through a
    /// built-in formatter when `Ok(None)` comes back — that's the
    /// "host falls back" path documented in the spec.
    pub fn dispatch_format(&self, ctx: &FormatCtx) -> Result<Option<LexAnnotationOut>, Diagnostic> {
        // The shared `dispatch` helper expects a `LabelCtx`; the
        // format hook uses `FormatCtx`. The only fields `dispatch`
        // reads off `LabelCtx` are `.label` (to route to the right
        // namespace) and `.node.range` (for diagnostic attribution
        // when something errors). Synthesise a minimal `LabelCtx` from
        // the `FormatCtx` so this path can reuse the panic-isolation
        // and namespace-health bookkeeping the other hooks already
        // share.
        let synthetic = LabelCtx {
            label: ctx.label.clone(),
            params: serde_json::Value::Null,
            body: lex_extension::AnnotationBody::None,
            node: lex_extension::NodeRef {
                kind: "format".into(),
                range: ctx.node.range(),
                origin: None,
            },
        };
        match self.dispatch(&synthetic, |h| h.on_format(ctx)) {
            Ok(Some(out)) => Ok(out),
            Ok(None) => Ok(None),
            Err(diag) => Err(diag),
        }
    }

    /// Dispatch [`LexHandler::on_code_action`].
    pub fn dispatch_code_action(&self, ctx: &LabelCtx) -> Vec<CodeAction> {
        match self.dispatch(ctx, |h| h.on_code_action(ctx)) {
            Ok(Some(actions)) => actions,
            Ok(None) => Vec::new(),
            Err(_) => Vec::new(),
        }
    }

    /// Core dispatch: look up the namespace for `ctx.label`, call the
    /// handler under [`catch_unwind`], and turn `Err(HandlerError)` /
    /// panic into a synthetic [`Diagnostic`].
    ///
    /// Returns:
    /// - `Ok(Some(value))` when the handler returned a value.
    /// - `Ok(None)` when no handler is registered for the label, or the
    ///   namespace is currently disabled.
    /// - `Err(diag)` when the handler returned `Err` or panicked.
    fn dispatch<R>(
        &self,
        ctx: &LabelCtx,
        f: impl FnOnce(&dyn LexHandler) -> Result<R, HandlerError>,
    ) -> Result<Option<R>, Diagnostic> {
        let namespace = {
            let inner = self.inner.read().expect("registry poisoned");
            let ns_name = match inner.label_to_namespace.get(&ctx.label) {
                Some(n) => n.clone(),
                None => return Ok(None),
            };
            match inner.namespaces.get(&ns_name) {
                Some(n) if n.healthy => ns_name,
                _ => return Ok(None),
            }
        };

        let result = {
            let inner = self.inner.read().expect("registry poisoned");
            let ns = inner
                .namespaces
                .get(&namespace)
                .expect("namespace existed when checked");
            catch_unwind(AssertUnwindSafe(|| f(ns.handler.as_ref())))
        };

        match result {
            Ok(Ok(value)) => Ok(Some(value)),
            Ok(Err(handler_err)) => Err(diagnostic_from_error(&handler_err, ctx.node.range)),
            Err(_panic) => {
                self.disable_namespace(&namespace, ctx);
                Err(panic_diagnostic(&namespace, ctx.node.range))
            }
        }
    }

    /// Like [`Self::dispatch`] but surfaces the original
    /// [`HandlerError`] instead of the cooked diagnostic. Used by the
    /// resolve pass so it can preserve `Custom { code, .. }` codes
    /// when mapping handler errors back to typed error variants.
    ///
    /// A panic inside the handler is folded into a synthetic
    /// `HandlerError::Internal` carrying a panic-specific message,
    /// and the namespace is disabled — same disable-once root
    /// diagnostic behaviour as `dispatch`. Panics surface as
    /// `Internal` rather than via a dedicated variant because
    /// `HandlerError` does not have a `Panic` variant.
    fn dispatch_raw<R>(
        &self,
        ctx: &LabelCtx,
        f: impl FnOnce(&dyn LexHandler) -> Result<R, HandlerError>,
    ) -> Result<Option<R>, HandlerError> {
        let namespace = {
            let inner = self.inner.read().expect("registry poisoned");
            let ns_name = match inner.label_to_namespace.get(&ctx.label) {
                Some(n) => n.clone(),
                None => return Ok(None),
            };
            match inner.namespaces.get(&ns_name) {
                Some(n) if n.healthy => ns_name,
                _ => return Ok(None),
            }
        };

        let result = {
            let inner = self.inner.read().expect("registry poisoned");
            let ns = inner
                .namespaces
                .get(&namespace)
                .expect("namespace existed when checked");
            catch_unwind(AssertUnwindSafe(|| f(ns.handler.as_ref())))
        };

        match result {
            Ok(Ok(value)) => Ok(Some(value)),
            Ok(Err(handler_err)) => Err(handler_err),
            Err(_panic) => {
                self.disable_namespace(&namespace, ctx);
                Err(HandlerError::internal(format!(
                    "extension namespace `{namespace}` panicked while handling this label and has been disabled for the rest of the session"
                )))
            }
        }
    }

    fn disable_namespace(&self, namespace: &str, ctx: &LabelCtx) {
        let mut inner = self.inner.write().expect("registry poisoned");
        if let Some(ns) = inner.namespaces.get_mut(namespace) {
            if !ns.healthy {
                return;
            }
            ns.healthy = false;
        }
        inner
            .root_diagnostics
            .push(root_panic_diagnostic(namespace, ctx.node.range));
    }
}

fn diagnostic_from_error(err: &HandlerError, range: Range) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        message: format!("handler error: {err}"),
        range,
        code: Some(handler_error_code(err).to_string()),
        related: Vec::new(),
    }
}

fn handler_error_code(err: &HandlerError) -> &'static str {
    match err {
        HandlerError::Internal { .. } => "handler.internal",
        HandlerError::Unsupported { .. } => "handler.unsupported",
        HandlerError::Custom { .. } => "handler.custom",
    }
}

fn panic_diagnostic(namespace: &str, range: Range) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        message: format!(
            "extension namespace `{namespace}` panicked while handling this label and has been disabled for the rest of the session"
        ),
        range,
        code: Some("handler.panic".into()),
        related: Vec::new(),
    }
}

fn root_panic_diagnostic(namespace: &str, range: Range) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        message: format!(
            "extension namespace `{namespace}` was disabled after a handler panic. No further extension behaviour from this namespace will run in this session."
        ),
        range,
        code: Some("handler.namespace-disabled".into()),
        related: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_extension::{
        AnnotationBody, BodyKind, BodyShape, Capabilities, HookSet, LabelCtx, NodeRef, Position,
        Range, Schema,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn schema(label: &str) -> Schema {
        Schema {
            schema_version: 1,
            label: label.into(),
            description: None,
            params: Default::default(),
            attaches_to: vec!["annotation".into()],
            body: BodyShape {
                kind: BodyKind::None,
                presence: Default::default(),
                description: None,
            },
            verbatim_label: false,
            capabilities: Capabilities::default(),
            hooks: HookSet::default(),
            handler: None,
        }
    }

    fn ctx(label: &str) -> LabelCtx {
        LabelCtx {
            label: label.into(),
            params: serde_json::json!({}),
            body: AnnotationBody::None,
            node: NodeRef {
                kind: "annotation".into(),
                range: Range {
                    start: Position(0, 0),
                    end: Position(0, 0),
                },
                origin: None,
            },
        }
    }

    struct NoOp;
    impl LexHandler for NoOp {}

    /// Records every call into a shared counter so tests can verify
    /// dispatch routes to the right handler.
    struct Counting {
        calls: Arc<AtomicUsize>,
    }
    impl LexHandler for Counting {
        fn on_label(&self, _ctx: &LabelCtx) {
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
        fn on_validate(&self, ctx: &LabelCtx) -> Result<Vec<Diagnostic>, HandlerError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![Diagnostic {
                severity: DiagnosticSeverity::Warning,
                message: format!("from {}", ctx.label),
                range: ctx.node.range,
                code: None,
                related: Vec::new(),
            }])
        }
    }

    struct Erroring;
    impl LexHandler for Erroring {
        fn on_validate(&self, _ctx: &LabelCtx) -> Result<Vec<Diagnostic>, HandlerError> {
            Err(HandlerError::internal("test failure"))
        }
    }

    struct Panicking;
    impl LexHandler for Panicking {
        fn on_validate(&self, _ctx: &LabelCtx) -> Result<Vec<Diagnostic>, HandlerError> {
            panic!("intentional test panic");
        }
    }

    #[test]
    fn empty_registry_returns_none_for_unknown_labels() {
        let r = Registry::new();
        assert!(r.schema_for("acme.task").is_none());
        assert!(r.dispatch_validate(&ctx("acme.task")).is_empty());
    }

    #[test]
    fn register_namespace_indexes_labels() {
        let r = Registry::new();
        r.register_namespace("acme", vec![schema("acme.task")], Box::new(NoOp))
            .unwrap();
        assert_eq!(r.namespace_count(), 1);
        assert!(r.schema_for("acme.task").is_some());
        assert!(r.is_namespace_healthy("acme"));
    }

    #[test]
    fn duplicate_namespace_is_rejected() {
        let r = Registry::new();
        r.register_namespace("acme", vec![schema("acme.task")], Box::new(NoOp))
            .unwrap();
        let err = r
            .register_namespace("acme", vec![schema("acme.user")], Box::new(NoOp))
            .unwrap_err();
        assert_eq!(
            err,
            RegistryError::NamespaceAlreadyRegistered {
                namespace: "acme".into()
            }
        );
    }

    #[test]
    fn label_outside_namespace_is_rejected() {
        let r = Registry::new();
        let err = r
            .register_namespace("acme", vec![schema("foo.task")], Box::new(NoOp))
            .unwrap_err();
        assert!(matches!(err, RegistryError::LabelOutsideNamespace { .. }));
    }

    #[test]
    fn validate_dispatch_routes_to_handler() {
        let calls = Arc::new(AtomicUsize::new(0));
        let r = Registry::new();
        r.register_namespace(
            "acme",
            vec![schema("acme.task"), schema("acme.user")],
            Box::new(Counting {
                calls: calls.clone(),
            }),
        )
        .unwrap();
        let diags = r.dispatch_validate(&ctx("acme.task"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "from acme.task");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Different label in same namespace also routes to the handler.
        let _ = r.dispatch_validate(&ctx("acme.user"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn label_dispatch_is_a_notification() {
        let calls = Arc::new(AtomicUsize::new(0));
        let r = Registry::new();
        r.register_namespace(
            "acme",
            vec![schema("acme.task")],
            Box::new(Counting {
                calls: calls.clone(),
            }),
        )
        .unwrap();
        r.dispatch_label(&ctx("acme.task"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn handler_error_becomes_diagnostic() {
        let r = Registry::new();
        r.register_namespace("acme", vec![schema("acme.task")], Box::new(Erroring))
            .unwrap();
        let diags = r.dispatch_validate(&ctx("acme.task"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diags[0].code.as_deref(), Some("handler.internal"));
        assert!(diags[0].message.contains("test failure"));
    }

    #[test]
    fn handler_panic_disables_namespace_and_surfaces_diagnostics() {
        let r = Registry::new();
        r.register_namespace("acme", vec![schema("acme.task")], Box::new(Panicking))
            .unwrap();
        // First call: panic surfaced as a diagnostic at the label site.
        let diags = r.dispatch_validate(&ctx("acme.task"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("handler.panic"));

        // Namespace is now unhealthy.
        assert!(!r.is_namespace_healthy("acme"));

        // Subsequent dispatches short-circuit (no further diagnostics from
        // the disabled namespace).
        let diags2 = r.dispatch_validate(&ctx("acme.task"));
        assert!(diags2.is_empty());

        // The disable event is recorded as a root-level diagnostic.
        let root = r.take_root_diagnostics();
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].code.as_deref(), Some("handler.namespace-disabled"));

        // Once taken, root diagnostics drain.
        assert!(r.take_root_diagnostics().is_empty());
    }

    #[test]
    fn unregistered_label_returns_empty_diagnostics() {
        let r = Registry::new();
        r.register_namespace("acme", vec![schema("acme.task")], Box::new(NoOp))
            .unwrap();
        // Different namespace, never registered.
        assert!(r.dispatch_validate(&ctx("foo.bar")).is_empty());
    }

    #[test]
    fn registry_is_send_and_sync() {
        // Compile-time check: a Registry must be safe to share across the
        // analyser, renderer, and LSP threads without external locking on
        // the public API.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Registry>();
    }

    /// Runtime concurrency stress: 10 threads dispatch concurrently against
    /// a shared registry whose only handler always panics. The locking
    /// design must:
    ///
    /// - never deadlock (`thread.join` returns within a bounded time),
    /// - disable the namespace exactly once (no double-decrement of
    ///   health, no duplicate root diagnostics),
    /// - still produce a per-call panic diagnostic for every thread that
    ///   reaches the handler before disable lands.
    #[test]
    fn concurrent_dispatch_survives_panics_safely() {
        struct AlwaysPanics;
        impl LexHandler for AlwaysPanics {
            fn on_validate(&self, _ctx: &LabelCtx) -> Result<Vec<Diagnostic>, HandlerError> {
                panic!("intentional concurrency-test panic");
            }
        }

        let r = std::sync::Arc::new(Registry::new());
        r.register_namespace("acme", vec![schema("acme.task")], Box::new(AlwaysPanics))
            .unwrap();

        // Suppress the noisy per-panic backtrace output for this test only;
        // the catch_unwind path still observes the panics correctly.
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        let mut handles = Vec::with_capacity(10);
        for _ in 0..10 {
            let r = r.clone();
            handles.push(std::thread::spawn(move || {
                r.dispatch_validate(&ctx("acme.task"))
            }));
        }
        let per_thread_diagnostics: Vec<_> =
            handles.into_iter().map(|h| h.join().unwrap()).collect();

        std::panic::set_hook(original_hook);

        // The namespace is disabled exactly once — exactly one root
        // diagnostic, regardless of how many threads raced through the
        // panic path.
        assert!(!r.is_namespace_healthy("acme"));
        let root = r.take_root_diagnostics();
        assert_eq!(
            root.len(),
            1,
            "namespace disable should fire exactly once under concurrent panics"
        );

        // At least the first thread to reach the handler (before disable
        // lands) emits a per-call panic diagnostic. After disable, threads
        // short-circuit with an empty result. We don't assert an exact
        // count — racing on the disable transition makes that timing-
        // dependent — but at least one must have made it through.
        let panicking_threads = per_thread_diagnostics
            .iter()
            .filter(|d| !d.is_empty())
            .count();
        assert!(
            panicking_threads >= 1,
            "at least one thread should have observed the panic before namespace was disabled"
        );
    }

    #[test]
    fn dispatch_format_routes_to_handler_and_returns_annotation() {
        // #570 Phase 4a: a handler that implements `on_format` for a
        // registered label produces structured output via
        // `Registry::dispatch_format`.
        use lex_extension::wire::{FormatCtx, LexAnnotationOut};

        struct FormatHandler;
        impl LexHandler for FormatHandler {
            fn on_format(&self, ctx: &FormatCtx) -> Result<Option<LexAnnotationOut>, HandlerError> {
                Ok(Some(LexAnnotationOut {
                    label: ctx.label.clone(),
                    params: ctx.params.clone(),
                    body: "formatted body".into(),
                    verbatim_label: true,
                }))
            }
        }

        let r = Registry::new();
        r.register_namespace("acme", vec![schema("acme.thing")], Box::new(FormatHandler))
            .expect("register ok");

        let fctx = FormatCtx {
            label: "acme.thing".into(),
            params: vec![("size".into(), "large".into())],
            node: WireNode::Paragraph {
                range: Range {
                    start: Position(0, 0),
                    end: Position(0, 0),
                },
                origin: None,
                inlines: vec![],
            },
            format_options: None,
        };

        let out = r
            .dispatch_format(&fctx)
            .expect("dispatch_format ok")
            .expect("handler returned Some");
        assert_eq!(out.label, "acme.thing");
        assert_eq!(out.params, vec![("size".into(), "large".into())]);
        assert_eq!(out.body, "formatted body");
        assert!(out.verbatim_label);
    }

    #[test]
    fn dispatch_format_returns_none_for_unregistered_label() {
        // Unrouted labels yield `Ok(None)` — same fallback signal as
        // every other dispatch path, no error.
        use lex_extension::wire::FormatCtx;

        let r = Registry::new();
        let fctx = FormatCtx {
            label: "nobody.knows".into(),
            params: vec![],
            node: WireNode::Paragraph {
                range: Range {
                    start: Position(0, 0),
                    end: Position(0, 0),
                },
                origin: None,
                inlines: vec![],
            },
            format_options: None,
        };
        let out = r.dispatch_format(&fctx).expect("dispatch_format ok");
        assert!(out.is_none(), "unrouted label must fall back");
    }

    #[test]
    fn dispatch_format_returns_none_when_handler_falls_back() {
        // A handler that returns `Ok(None)` from `on_format` (the
        // default impl, or an explicit decision) makes
        // `dispatch_format` surface `Ok(None)` so the caller can fall
        // back to the host's built-in formatter.
        use lex_extension::wire::FormatCtx;

        let r = Registry::new();
        r.register_namespace("acme", vec![schema("acme.silent")], Box::new(NoOp))
            .expect("register ok");

        let fctx = FormatCtx {
            label: "acme.silent".into(),
            params: vec![],
            node: WireNode::Paragraph {
                range: Range {
                    start: Position(0, 0),
                    end: Position(0, 0),
                },
                origin: None,
                inlines: vec![],
            },
            format_options: None,
        };
        let out = r.dispatch_format(&fctx).expect("dispatch_format ok");
        assert!(
            out.is_none(),
            "NoOp handler must let `dispatch_format` surface None"
        );
    }

    /// #615: `dispatch_ir_build` is the IR-construction lifecycle
    /// dispatch entry point. Same wire shape as `dispatch_resolve`
    /// but a distinct hook so handlers can declare the exact
    /// lifecycle phase they participate in.
    #[test]
    fn dispatch_ir_build_routes_to_handler_and_returns_wire_node() {
        struct IrBuildHandler;
        impl LexHandler for IrBuildHandler {
            fn on_ir_build(&self, ctx: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
                Ok(Some(WireNode::Verbatim {
                    range: ctx.node.range,
                    origin: ctx.node.origin.clone(),
                    label: ctx.label.clone(),
                    params: serde_json::Value::Null,
                    body_text: format!("ir_build:{}", ctx.label),
                    subject: String::new(),
                    mode: "inflow".into(),
                }))
            }
        }
        let r = Registry::new();
        r.register_namespace("acme", vec![schema("acme.thing")], Box::new(IrBuildHandler))
            .expect("register ok");
        let wire = r
            .dispatch_ir_build(&ctx("acme.thing"))
            .expect("dispatch_ir_build ok")
            .expect("handler returned Some");
        match wire {
            WireNode::Verbatim { body_text, .. } => {
                assert_eq!(body_text, "ir_build:acme.thing");
            }
            other => panic!("expected Verbatim, got {other:?}"),
        }
    }

    /// `dispatch_ir_build` must surface `Ok(None)` for unrouted
    /// labels — same contract as the other dispatch helpers.
    #[test]
    fn dispatch_ir_build_returns_none_for_unregistered_label() {
        let r = Registry::new();
        let result = r
            .dispatch_ir_build(&ctx("nobody.knows"))
            .expect("dispatch_ir_build ok");
        assert!(result.is_none());
    }

    /// `dispatch_ir_build` and `dispatch_resolve` are distinct hooks
    /// even though their wire shape matches. A handler that overrides
    /// only `on_resolve` must NOT receive `dispatch_ir_build` calls
    /// (and vice versa). This pins the lifecycle separation that
    /// motivated #615.
    #[test]
    fn dispatch_ir_build_does_not_invoke_on_resolve() {
        struct ResolveOnly;
        impl LexHandler for ResolveOnly {
            fn on_resolve(&self, ctx: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
                Ok(Some(WireNode::Verbatim {
                    range: ctx.node.range,
                    origin: ctx.node.origin.clone(),
                    label: ctx.label.clone(),
                    params: serde_json::Value::Null,
                    body_text: "ROUTED_TO_RESOLVE".into(),
                    subject: String::new(),
                    mode: "inflow".into(),
                }))
            }
        }
        let r = Registry::new();
        r.register_namespace("acme", vec![schema("acme.thing")], Box::new(ResolveOnly))
            .expect("register ok");

        // `dispatch_ir_build` must hit `on_ir_build` (default impl
        // returns `Ok(None)`), NOT `on_resolve`.
        let result = r
            .dispatch_ir_build(&ctx("acme.thing"))
            .expect("dispatch_ir_build ok");
        assert!(
            result.is_none(),
            "dispatch_ir_build must not invoke on_resolve; \
             handler that overrides only on_resolve gets default None on ir_build"
        );

        // Sanity: `dispatch_resolve` still routes correctly.
        let resolved = r
            .dispatch_resolve(&ctx("acme.thing"))
            .expect("dispatch_resolve ok")
            .expect("on_resolve returned Some");
        match resolved {
            WireNode::Verbatim { body_text, .. } => {
                assert_eq!(body_text, "ROUTED_TO_RESOLVE");
            }
            other => panic!("expected Verbatim, got {other:?}"),
        }
    }

    /// Errors from `on_ir_build` fold into a synthetic diagnostic via
    /// the same `dispatch` machinery as the other hooks.
    #[test]
    fn dispatch_ir_build_handler_error_becomes_diagnostic() {
        struct Boom;
        impl LexHandler for Boom {
            fn on_ir_build(&self, _: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
                Err(HandlerError::internal("ir_build boom"))
            }
        }
        let r = Registry::new();
        r.register_namespace("acme", vec![schema("acme.thing")], Box::new(Boom))
            .expect("register ok");
        let err = r
            .dispatch_ir_build(&ctx("acme.thing"))
            .expect_err("must error");
        assert_eq!(err.code.as_deref(), Some("handler.internal"));
        assert!(err.message.contains("ir_build boom"));
    }
}
