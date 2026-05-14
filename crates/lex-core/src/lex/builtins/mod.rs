//! Built-in `LexHandler` implementations for the `lex.*` namespace.
//!
//! Built-ins flow through the same `lex_extension::LexHandler` trait and
//! `lex_extension_host::Registry` dispatch fabric as third-party namespaces.
//! Their only privilege is being compiled-in: at host startup, the CLI
//! and LSP call this module's [`register_into`] helper to attach the
//! bundled `lex.*` schemas and handlers.
//!
//! # What ships today
//!
//! | Label family            | Handler                | Status                                |
//! |-------------------------|------------------------|---------------------------------------|
//! | `lex.include`           | [`LexIncludeHandler`]  | Registrable; resolve pass runs        |
//! |                         |                        | through the legacy inline path until  |
//! |                         |                        | PR 3d (#533).                         |
//! | `lex.metadata.*` (×8)   | [`LexBuiltinsHandler`] | Schemas registered (#570 Phase 1).    |
//! |                         |                        | Legacy frontmatter promotion in       |
//! |                         |                        | `lex-babel/src/ir/from_lex.rs` still  |
//! |                         |                        | owns the IR work; on_format returns   |
//! |                         |                        | None for this family in Phase 4b.     |
//! | `lex.tabular.table`     | [`LexBuiltinsHandler`] | Schema registered (#570 Phase 1) +    |
//! |                         |                        | `on_format` implemented (#570 Phase   |
//! |                         |                        | 4b). The legacy `VerbatimRegistry::`  |
//! |                         |                        | `TableHandler` still parses pipe-     |
//! |                         |                        | tables on the from_lex direction; the |
//! |                         |                        | to_lex production wiring lands in a   |
//! |                         |                        | Phase 4b follow-up.                   |
//! | `lex.media.{image,…}`   | [`LexBuiltinsHandler`] | Same shape as `lex.tabular.table`:    |
//! |                         |                        | schemas + on_format implemented;      |
//! |                         |                        | legacy `VerbatimRegistry::Image/…`    |
//! |                         |                        | handlers still build the IR nodes.    |
//!
//! The single `lex` namespace is shared by every built-in label; the
//! composite [`LexBuiltinsHandler`] routes each dispatch by
//! [`LabelCtx::label`](lex_extension::wire::LabelCtx::label) to the right
//! sub-handler.

use std::sync::Arc;

use lex_extension::{
    handler::{HandlerError, LexHandler},
    schema::{
        BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, ParamSpec, ParamType, Schema,
    },
    wire::{AnnotationBody, FormatCtx, LabelCtx, LexAnnotationOut, Position, Range, WireNode},
};
use lex_extension_host::registry::{Registry, RegistryError};

use crate::lex::includes::{Loader, ResolveConfig};

pub mod include;
pub mod media;
pub mod metadata;
pub mod notes;
pub mod tabular;

pub use include::LexIncludeHandler;

/// The reserved namespace owned by the lex core. Its prefix is
/// `lex.` (with the trailing dot), so registered labels look like
/// `lex.include`, `lex.metadata.title`, `lex.tabular.table`, etc.
pub const NAMESPACE: &str = "lex";

/// Every canonical `lex.*` label the core ships. Aggregated from
/// `include`, `metadata::METADATA_LABELS`, `tabular::LEX_TABULAR_TABLE`,
/// and `media::{LEX_MEDIA_IMAGE, LEX_MEDIA_VIDEO, LEX_MEDIA_AUDIO}` so
/// the parse-time `NormalizeLabels` stage in `assembling::stages` can
/// resolve user-authored bare and prefix-stripped forms to the
/// canonical registry without depending on a runtime registry handle.
///
/// Adding a new `lex.*` canonical requires adding it here too — the
/// builtin-tests in each family enforce the corresponding ordering /
/// presence checks. Order within the slice is informational only;
/// lookups are unordered.
pub const CANONICAL_LABELS: &[&str] = &[
    "lex.include",
    "lex.notes",
    // metadata family
    "lex.metadata.title",
    "lex.metadata.author",
    "lex.metadata.date",
    "lex.metadata.tags",
    "lex.metadata.category",
    "lex.metadata.template",
    "lex.metadata.publishing-date",
    "lex.metadata.front-matter",
    // tabular family
    "lex.tabular.table",
    // media family
    "lex.media.image",
    "lex.media.video",
    "lex.media.audio",
];

/// Return `true` if `label` names a canonical built-in. Lookup is a
/// linear scan of [`CANONICAL_LABELS`]; the slice is small (13 entries
/// today) so this stays cheaper than a `HashSet` materialised at
/// startup.
pub fn is_canonical_label(label: &str) -> bool {
    CANONICAL_LABELS.contains(&label)
}

/// Composite handler for the `lex.*` namespace.
///
/// `Registry::register_namespace` accepts one handler per namespace; the
/// composite shape lets every `lex.*` built-in live under a single
/// namespace registration while keeping per-label logic isolated.
///
/// Implementations across hooks:
///
/// - `on_resolve`: only [`LexIncludeHandler`] (#532) — the
///   `lex.tabular.*` / `lex.media.*` / `lex.metadata.*` labels return
///   the default `Ok(None)` because the legacy `from_lex` direction
///   in `lex-babel` already hydrates the AST.
/// - `on_format`: implemented for `lex.tabular.table` and
///   `lex.media.{image,video,audio}` (#570 Phase 4b). `lex.include` is
///   resolve-only and falls back; `lex.metadata.*` flows through the
///   render hook + legacy frontmatter promotion.
pub struct LexBuiltinsHandler {
    include: LexIncludeHandler,
}

impl LexBuiltinsHandler {
    pub fn new(loader: Arc<dyn Loader + Send + Sync>, config: ResolveConfig) -> Self {
        Self {
            include: LexIncludeHandler::new(loader, config),
        }
    }
}

impl LexHandler for LexBuiltinsHandler {
    fn on_resolve(&self, ctx: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
        match ctx.label.as_str() {
            "lex.include" => self.include.on_resolve(ctx),
            "lex.tabular.table" => Ok(Some(resolve_tabular_table(ctx))),
            "lex.media.image" => Ok(Some(resolve_media_image(ctx))),
            "lex.media.video" => Ok(Some(resolve_media_video(ctx))),
            "lex.media.audio" => Ok(Some(resolve_media_audio(ctx))),
            _ => Ok(None),
        }
    }

    /// Phase 4b of #570: emit the canonical Lex-source shape for the
    /// built-in `lex.tabular.*` and `lex.media.*` labels.
    ///
    /// The verbatim labels round-trip as `:: lex.<family>.<kind> ::`
    /// closers with the body text and parameters carried verbatim from
    /// the supplied `WireNode::Verbatim`. Anything else (e.g.
    /// `lex.include`, metadata labels, unrecognised labels) returns
    /// `Ok(None)` so the host falls back to its built-in formatter.
    fn on_format(&self, ctx: &FormatCtx) -> Result<Option<LexAnnotationOut>, HandlerError> {
        match ctx.label.as_str() {
            "lex.tabular.table" | "lex.media.image" | "lex.media.video" | "lex.media.audio" => {
                verbatim_label_on_format(ctx)
            }
            // `lex.include` is the resolve-only direction — it splices
            // content in via on_resolve; no IR→Lex emission path. The
            // metadata labels (`lex.metadata.*`) and any unrecognised
            // label fall back to the host's default formatter.
            _ => Ok(None),
        }
    }
}

/// Shared `on_format` body for the four built-in verbatim labels
/// (`lex.tabular.table`, `lex.media.{image,video,audio}`). Each one
/// has the same wire shape: a `WireNode::Verbatim` whose `body_text`
/// carries the verbatim source (pipe-table syntax, alt-text fallback,
/// etc.).
///
/// The handler uses `ctx.params` directly — the wire spec
/// (`lex-extension-wire.lex` §4.8) treats `FormatCtx::params` as the
/// authoritative originating parameters, with `WireNode::Verbatim.params`
/// being a wire-internal copy of the same data. A well-formed host
/// fills both with the same `(key, value)` pairs; in the
/// hypothetical case where they diverge, `ctx.params` wins.
/// `on_resolve` for `lex.tabular.table`: parse the verbatim body
/// (pipe-table source) into a typed [`WireNode::Table`]. The wire
/// table carries per-column alignment in `column_aligns` — no
/// fidelity loss on mixed-alignment tables.
fn resolve_tabular_table(ctx: &LabelCtx) -> WireNode {
    let body = match &ctx.body {
        AnnotationBody::Text(s) => s.as_str(),
        // No body or a `Lex`-shaped body — fall back to an empty
        // table. (Verbatim labels can't legitimately have a `Lex`
        // body; schema enforcement keeps this branch unreachable in
        // well-formed inputs.)
        _ => "",
    };
    let mut table = tabular::parse_pipe_table_to_wire(body);
    // Stamp the host's range + origin onto the wire node — the
    // parser builds with `(0,0)` defaults since it has no source
    // context of its own.
    if let WireNode::Table { range, origin, .. } = &mut table {
        *range = ctx.node.range;
        *origin = ctx.node.origin.clone();
    }
    table
}

/// `on_resolve` for `lex.media.image`: read `src`/`alt`/`title` from
/// `ctx.params`. Falls back to the verbatim body for `alt` when the
/// `alt=` param is missing — mirrors the lex-babel
/// `image_from_params` contract.
fn resolve_media_image(ctx: &LabelCtx) -> WireNode {
    let src = string_param(ctx, "src").unwrap_or_default();
    let alt = string_param(ctx, "alt").unwrap_or_else(|| match &ctx.body {
        AnnotationBody::Text(s) => s.trim().to_string(),
        _ => String::new(),
    });
    let title = string_param(ctx, "title");
    WireNode::Image {
        range: ctx.node.range,
        origin: ctx.node.origin.clone(),
        src,
        alt,
        title,
    }
}

/// `on_resolve` for `lex.media.video`: read `src`/`title`/`poster`
/// from `ctx.params`.
fn resolve_media_video(ctx: &LabelCtx) -> WireNode {
    WireNode::Video {
        range: ctx.node.range,
        origin: ctx.node.origin.clone(),
        src: string_param(ctx, "src").unwrap_or_default(),
        title: string_param(ctx, "title"),
        poster: string_param(ctx, "poster"),
    }
}

/// `on_resolve` for `lex.media.audio`: read `src`/`title` from
/// `ctx.params`.
fn resolve_media_audio(ctx: &LabelCtx) -> WireNode {
    WireNode::Audio {
        range: ctx.node.range,
        origin: ctx.node.origin.clone(),
        src: string_param(ctx, "src").unwrap_or_default(),
        title: string_param(ctx, "title"),
    }
}

/// Extract a string-typed parameter from `ctx.params`. Returns `None`
/// when the key is missing or the value isn't a string. Used by the
/// media resolve helpers.
fn string_param(ctx: &LabelCtx, key: &str) -> Option<String> {
    ctx.params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[allow(dead_code)]
fn default_resolve_range() -> Range {
    Range {
        start: Position(0, 0),
        end: Position(0, 0),
    }
}

fn verbatim_label_on_format(ctx: &FormatCtx) -> Result<Option<LexAnnotationOut>, HandlerError> {
    let body = match &ctx.node {
        WireNode::Verbatim { body_text, .. } => body_text.clone(),
        // For non-verbatim wire nodes (e.g. a typed Table kind that a
        // future wire-spec revision adds), the built-in handler
        // doesn't have a serializer yet — return None so the host
        // falls back. Phase 4b ships the verbatim path only.
        _ => return Ok(None),
    };
    Ok(Some(LexAnnotationOut {
        label: ctx.label.clone(),
        params: ctx.params.clone(),
        body,
        verbatim_label: true,
    }))
}

/// Register every built-in `lex.*` schema and handler into `registry`.
///
/// `loader` and `config` are forwarded verbatim to the composite
/// handler; today they're consumed only by [`LexIncludeHandler`] but
/// future built-ins may need filesystem access too (e.g. an asset
/// resolver for `lex.media.*`).
pub fn register_into(
    registry: &Registry,
    loader: Arc<dyn Loader + Send + Sync>,
    config: ResolveConfig,
) -> Result<(), RegistryError> {
    let mut schemas = Vec::with_capacity(14);
    schemas.push(lex_include_schema());
    schemas.extend(notes::all_schemas());
    schemas.extend(metadata::all_schemas());
    schemas.extend(tabular::all_schemas());
    schemas.extend(media::all_schemas());

    let handler = Box::new(LexBuiltinsHandler::new(loader, config));
    registry.register_namespace(NAMESPACE, schemas, handler)
}

/// Schema for the `lex.include` label. Inlined here because v1 has
/// exactly one built-in label of its kind; once the YAML schema loader
/// lands in PR 4 (#520), built-ins will share the same load path as
/// third parties (a baked-in `lex.yaml` shipped with the crate).
pub fn lex_include_schema() -> Schema {
    let mut params = std::collections::BTreeMap::new();
    params.insert(
        "src".into(),
        ParamSpec {
            ty: ParamType::String,
            required: true,
            default: None,
            description: Some(
                "Path to the file to splice in. Resolves relative to the host file's directory; \
                 leading `/` resolves under the resolution root."
                    .into(),
            ),
            pattern: None,
            values: Vec::new(),
        },
    );
    Schema {
        schema_version: 1,
        label: "lex.include".into(),
        description: Some(
            "Splice the referenced Lex file's content into the parent container at this \
             annotation's position."
                .into(),
        ),
        params,
        attaches_to: vec!["annotation".into()],
        body: BodyShape {
            kind: BodyKind::None,
            presence: BodyPresence::Optional,
            description: None,
        },
        verbatim_label: false,
        // Built-ins read from the filesystem; once the trust matrix
        // gates third-party fs access in δ (PR 12), built-ins remain
        // trusted by linkage.
        capabilities: Capabilities {
            fs: true,
            net: false,
        },
        hooks: HookSet {
            resolve: true,
            ..HookSet::default()
        },
        // Native built-ins skip the handler-spec field — the registry
        // dispatches in-process via `Box<dyn LexHandler>`.
        handler: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::includes::MemoryLoader;
    use lex_extension::wire::{AnnotationBody, LabelCtx, NodeRef, Position, Range};
    use std::path::PathBuf;

    fn make_ctx(label: &str, src: Option<&str>, host_origin: Option<&str>) -> LabelCtx {
        LabelCtx {
            label: label.into(),
            params: match src {
                Some(s) => serde_json::json!({ "src": s }),
                None => serde_json::json!({}),
            },
            body: AnnotationBody::None,
            node: NodeRef {
                kind: "annotation".into(),
                range: Range {
                    start: Position(0, 0),
                    end: Position(0, 0),
                },
                origin: host_origin.map(|s| s.to_string()),
            },
        }
    }

    fn fresh_registry() -> Registry {
        let mut loader = MemoryLoader::new();
        loader.insert(PathBuf::from("/root/inner.lex"), "Hello.\n");
        let registry = Registry::new();
        register_into(
            &registry,
            Arc::new(loader),
            ResolveConfig::with_root(PathBuf::from("/root")),
        )
        .expect("registration ok");
        registry
    }

    #[test]
    fn canonical_labels_matches_registered_schemas() {
        // CANONICAL_LABELS feeds the parse-time NormalizeLabels stage —
        // it MUST contain exactly the same labels that register_into
        // registers, in any order. If a new lex.* schema is added without
        // updating CANONICAL_LABELS, NormalizeLabels will start rejecting
        // valid documents authored using its canonical or stripped forms.
        let mut registered: Vec<String> = Vec::new();
        registered.push(lex_include_schema().label);
        registered.extend(notes::all_schemas().into_iter().map(|s| s.label));
        registered.extend(metadata::all_schemas().into_iter().map(|s| s.label));
        registered.extend(tabular::all_schemas().into_iter().map(|s| s.label));
        registered.extend(media::all_schemas().into_iter().map(|s| s.label));

        let constant: Vec<String> = CANONICAL_LABELS.iter().map(|s| (*s).to_string()).collect();

        let mut registered_sorted = registered.clone();
        registered_sorted.sort();
        let mut constant_sorted = constant.clone();
        constant_sorted.sort();
        assert_eq!(
            registered_sorted, constant_sorted,
            "CANONICAL_LABELS and registered schemas must match; \
             registered={registered:?} constant={constant:?}"
        );
    }

    #[test]
    fn is_canonical_label_recognizes_every_constant() {
        for label in CANONICAL_LABELS {
            assert!(is_canonical_label(label), "{label} must be canonical");
        }
        assert!(!is_canonical_label(""));
        assert!(!is_canonical_label("title"));
        assert!(!is_canonical_label("metadata.title"));
        assert!(!is_canonical_label("doc.table"));
        assert!(!is_canonical_label("acme.task"));
    }

    #[test]
    fn register_into_attaches_namespace_and_schema() {
        let registry = fresh_registry();
        assert_eq!(registry.namespace_count(), 1);
        assert!(registry.is_namespace_healthy(NAMESPACE));
        let schema = registry
            .schema_for("lex.include")
            .expect("schema indexed under fully-qualified label");
        assert_eq!(schema.label, "lex.include");
        assert!(schema.hooks.resolve, "resolve hook must be declared");
        assert!(
            schema.params.contains_key("src"),
            "src parameter must be declared"
        );
    }

    #[test]
    fn dispatch_resolve_round_trip_via_registry() {
        // End-to-end through the registry: register handler, dispatch
        // a resolve via dispatch_resolve, get back Some(WireNode).
        let registry = fresh_registry();
        let ctx = make_ctx("lex.include", Some("inner.lex"), Some("/root/host.lex"));
        let wire = registry
            .dispatch_resolve(&ctx)
            .expect("dispatch_resolve ok")
            .expect("returned Some");
        match wire {
            lex_extension::wire::WireNode::Document { children, .. } => {
                assert!(
                    !children.is_empty(),
                    "registry-routed resolve must surface the included content"
                );
            }
            other => panic!("expected WireNode::Document, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_resolve_load_error_surfaces_diagnostic() {
        let registry = Registry::new();
        register_into(
            &registry,
            Arc::new(MemoryLoader::new()),
            ResolveConfig::with_root(PathBuf::from("/root")),
        )
        .expect("registration ok");

        let ctx = make_ctx("lex.include", Some("missing.lex"), Some("/root/host.lex"));
        let err = registry
            .dispatch_resolve(&ctx)
            .expect_err("registry must surface the load error");
        assert_eq!(err.code.as_deref(), Some("handler.custom"));
        assert!(
            err.message.contains("not found"),
            "diagnostic must mention the load failure"
        );
    }

    #[test]
    fn duplicate_register_into_call_is_rejected() {
        let registry = Registry::new();
        register_into(
            &registry,
            Arc::new(MemoryLoader::new()),
            ResolveConfig::with_root(PathBuf::from("/root")),
        )
        .expect("first registration ok");
        let second = register_into(
            &registry,
            Arc::new(MemoryLoader::new()),
            ResolveConfig::with_root(PathBuf::from("/root")),
        );
        assert!(
            matches!(
                second,
                Err(RegistryError::NamespaceAlreadyRegistered { .. })
            ),
            "second register_into must error: {second:?}"
        );
    }

    #[test]
    fn metadata_schemas_are_registered() {
        let registry = fresh_registry();
        for label in metadata::METADATA_LABELS {
            let schema = registry
                .schema_for(label)
                .unwrap_or_else(|| panic!("schema_for({label}) must be Some"));
            assert_eq!(schema.label, *label);
            assert_eq!(schema.attaches_to, vec!["document".to_string()]);
            assert!(!schema.verbatim_label);
        }
    }

    #[test]
    fn tabular_table_schema_is_registered() {
        let registry = fresh_registry();
        let schema = registry
            .schema_for(tabular::LEX_TABULAR_TABLE)
            .expect("lex.tabular.table schema must be registered");
        assert!(schema.verbatim_label);
        assert_eq!(schema.attaches_to, vec!["verbatim".to_string()]);
    }

    #[test]
    fn media_schemas_are_registered() {
        let registry = fresh_registry();
        for label in [
            media::LEX_MEDIA_IMAGE,
            media::LEX_MEDIA_VIDEO,
            media::LEX_MEDIA_AUDIO,
        ] {
            let schema = registry
                .schema_for(label)
                .unwrap_or_else(|| panic!("schema_for({label}) must be Some"));
            assert!(schema.verbatim_label);
            assert_eq!(schema.attaches_to, vec!["verbatim".to_string()]);
            assert!(
                schema
                    .params
                    .get("src")
                    .map(|p| p.required)
                    .unwrap_or(false),
                "{label} must require src"
            );
        }
    }

    #[test]
    fn dispatch_resolve_metadata_returns_none() {
        // Metadata schemas don't declare `on_resolve` (they're
        // attached to the document, consumed by analysis). Dispatch
        // must short-circuit to None for each one.
        let registry = fresh_registry();
        for label in metadata::METADATA_LABELS {
            let ctx = make_ctx(label, None, None);
            let result = registry
                .dispatch_resolve(&ctx)
                .unwrap_or_else(|e| panic!("dispatch_resolve({label}) errored: {e:?}"));
            assert!(
                result.is_none(),
                "dispatch_resolve({label}) must return None; got Some(...)"
            );
        }
    }

    #[test]
    fn dispatch_resolve_media_returns_typed_wire_kinds() {
        // Phase 3 of #570: `lex.media.*` resolve to typed
        // `WireNode::{Image, Video, Audio}` variants.
        let registry = fresh_registry();
        for (label, expect_kind) in [
            (media::LEX_MEDIA_IMAGE, "image"),
            (media::LEX_MEDIA_VIDEO, "video"),
            (media::LEX_MEDIA_AUDIO, "audio"),
        ] {
            let ctx = make_ctx(label, Some("./asset.media"), None);
            let result = registry
                .dispatch_resolve(&ctx)
                .unwrap_or_else(|e| panic!("dispatch_resolve({label}) errored: {e:?}"))
                .unwrap_or_else(|| panic!("dispatch_resolve({label}) must return Some"));
            let actual = match result {
                lex_extension::wire::WireNode::Image { .. } => "image",
                lex_extension::wire::WireNode::Video { .. } => "video",
                lex_extension::wire::WireNode::Audio { .. } => "audio",
                other => panic!("dispatch_resolve({label}) produced unexpected variant {other:?}"),
            };
            assert_eq!(actual, expect_kind, "wire variant for {label}");
        }
    }

    #[test]
    fn dispatch_resolve_propagates_ctx_range_and_origin() {
        // Resolve handlers must stamp `ctx.node.range` and
        // `ctx.node.origin` onto the WireNode they return so
        // downstream diagnostics can attribute back to source. Hard-
        // coded `(0,0)` and `origin: None` would silently break LSP
        // hover / go-to-def for handler-emitted nodes.
        let registry = fresh_registry();
        let stamped_range = Range {
            start: Position(12, 4),
            end: Position(14, 10),
        };
        let stamped_origin = Some("/host/doc.lex".to_string());
        let cases: &[(&str, &str)] = &[
            (
                tabular::LEX_TABULAR_TABLE,
                "| a | b |\n|---|---|\n| 1 | 2 |",
            ),
            (media::LEX_MEDIA_IMAGE, ""),
            (media::LEX_MEDIA_VIDEO, ""),
            (media::LEX_MEDIA_AUDIO, ""),
        ];
        for (label, body) in cases {
            let ctx = LabelCtx {
                label: (*label).into(),
                params: serde_json::json!({ "src": "x" }),
                body: AnnotationBody::Text((*body).into()),
                node: NodeRef {
                    kind: "verbatim".into(),
                    range: stamped_range,
                    origin: stamped_origin.clone(),
                },
            };
            let result = registry
                .dispatch_resolve(&ctx)
                .unwrap_or_else(|e| panic!("dispatch_resolve({label}) errored: {e:?}"))
                .unwrap_or_else(|| panic!("dispatch_resolve({label}) must return Some"));
            let (got_range, got_origin) = match result {
                lex_extension::wire::WireNode::Table { range, origin, .. }
                | lex_extension::wire::WireNode::Image { range, origin, .. }
                | lex_extension::wire::WireNode::Video { range, origin, .. }
                | lex_extension::wire::WireNode::Audio { range, origin, .. } => (range, origin),
                other => panic!("dispatch_resolve({label}) produced unexpected variant {other:?}"),
            };
            assert_eq!(
                got_range, stamped_range,
                "range must propagate from LabelCtx to WireNode for {label}"
            );
            assert_eq!(
                got_origin, stamped_origin,
                "origin must propagate from LabelCtx to WireNode for {label}"
            );
        }
    }

    #[test]
    fn namespace_count_is_one_namespace_with_thirteen_labels() {
        let registry = fresh_registry();
        assert_eq!(
            registry.namespace_count(),
            1,
            "all built-ins share the single `lex` namespace"
        );
        // 1 include + 8 metadata + 1 tabular + 3 media = 13.
        let expected_labels = [
            "lex.include",
            "lex.metadata.title",
            "lex.metadata.author",
            "lex.metadata.date",
            "lex.metadata.tags",
            "lex.metadata.category",
            "lex.metadata.template",
            "lex.metadata.publishing-date",
            "lex.metadata.front-matter",
            "lex.tabular.table",
            "lex.media.image",
            "lex.media.video",
            "lex.media.audio",
        ];
        for label in expected_labels {
            assert!(
                registry.schema_for(label).is_some(),
                "expected label {label} to be registered"
            );
        }
    }

    /// Build a `FormatCtx` whose `node` is a `WireNode::Verbatim`
    /// carrying the supplied body text and params. The four built-in
    /// verbatim labels share this shape; this helper keeps each test
    /// to a single line of meaningful setup.
    fn format_ctx_verbatim(
        label: &str,
        params: Vec<(&str, &str)>,
        body_text: &str,
    ) -> lex_extension::wire::FormatCtx {
        use lex_extension::wire::{FormatCtx, WireNode};
        let owned_params: Vec<(String, String)> = params
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        FormatCtx {
            label: label.into(),
            params: owned_params.clone(),
            node: WireNode::Verbatim {
                range: Range {
                    start: Position(0, 0),
                    end: Position(0, 0),
                },
                origin: None,
                label: label.into(),
                params: serde_json::Value::Object(
                    owned_params
                        .iter()
                        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                        .collect(),
                ),
                body_text: body_text.into(),
                subject: String::new(),
                mode: "inflow".into(),
            },
            format_options: None,
        }
    }

    #[test]
    fn dispatch_format_for_lex_tabular_table_returns_verbatim_annotation() {
        // Phase 4b of #570: the built-in `lex.tabular.table` handler
        // takes a WireNode::Verbatim whose body_text carries the
        // pipe-table source and emits a LexAnnotationOut that the
        // caller (`to_lex` etc.) can splice as `:: lex.tabular.table ::`.
        let registry = fresh_registry();
        let body = "| a | b |\n|---|---|\n| 1 | 2 |";
        let ctx = format_ctx_verbatim("lex.tabular.table", vec![("header", "1")], body);
        let out = registry
            .dispatch_format(&ctx)
            .expect("dispatch_format ok")
            .expect("handler returned Some");
        assert_eq!(out.label, "lex.tabular.table");
        assert_eq!(out.params, vec![("header".into(), "1".into())]);
        assert_eq!(out.body, body);
        assert!(out.verbatim_label);
    }

    #[test]
    fn dispatch_format_for_lex_media_image_returns_verbatim_annotation() {
        let registry = fresh_registry();
        let ctx = format_ctx_verbatim(
            "lex.media.image",
            vec![("src", "chart.png"), ("alt", "Q4 chart")],
            "",
        );
        let out = registry
            .dispatch_format(&ctx)
            .expect("dispatch_format ok")
            .expect("handler returned Some");
        assert_eq!(out.label, "lex.media.image");
        let src = out
            .params
            .iter()
            .find(|(k, _)| k == "src")
            .map(|(_, v)| v.as_str());
        assert_eq!(src, Some("chart.png"));
        assert!(out.verbatim_label);
    }

    #[test]
    fn dispatch_format_for_lex_media_video_and_audio_return_verbatim_annotation() {
        let registry = fresh_registry();
        for label in ["lex.media.video", "lex.media.audio"] {
            let ctx = format_ctx_verbatim(label, vec![("src", "media.mp4")], "");
            let out = registry
                .dispatch_format(&ctx)
                .expect("dispatch_format ok")
                .unwrap_or_else(|| panic!("handler must return Some for {label}"));
            assert_eq!(out.label, label);
            assert!(out.verbatim_label);
        }
    }

    #[test]
    fn dispatch_format_for_lex_include_returns_none() {
        // `lex.include` is the resolve-only direction; on_format is
        // not implemented for it.
        let registry = fresh_registry();
        let ctx = format_ctx_verbatim("lex.include", vec![("src", "other.lex")], "");
        let out = registry.dispatch_format(&ctx).expect("dispatch_format ok");
        assert!(out.is_none(), "lex.include has no on_format path");
    }

    #[test]
    fn dispatch_format_for_lex_metadata_returns_none() {
        // Metadata labels fall back to the host's built-in formatter
        // in Phase 4b — they're not verbatim and the wire shape isn't
        // a Verbatim node, so the shared verbatim-label helper bails.
        let registry = fresh_registry();
        let ctx = format_ctx_verbatim("lex.metadata.title", vec![], "My Doc");
        let out = registry.dispatch_format(&ctx).expect("dispatch_format ok");
        assert!(out.is_none(), "metadata labels fall back to host default");
    }

    #[test]
    fn dispatch_format_with_non_verbatim_node_returns_none() {
        // Even for a built-in verbatim label, if the host passes a
        // non-verbatim WireNode (e.g. a Paragraph), the handler bails
        // rather than emit a malformed annotation.
        use lex_extension::wire::{FormatCtx, WireNode};
        let registry = fresh_registry();
        let ctx = FormatCtx {
            label: "lex.tabular.table".into(),
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
        let out = registry.dispatch_format(&ctx).expect("dispatch_format ok");
        assert!(
            out.is_none(),
            "non-verbatim wire node must fall back to host default"
        );
    }
}
