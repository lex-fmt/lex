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
//! | `lex.include`           | [`LexIncludeHandler`]  | Registrable; resolve pass still runs  |
//! |                         |                        | through the legacy inline path until  |
//! |                         |                        | PR 3d (#533).                         |
//! | `lex.metadata.*` (×8)   | [`LexBuiltinsHandler`] | Phase 1 of #570 — schemas only.       |
//! |                         |                        | Legacy frontmatter promotion in       |
//! |                         |                        | `lex-babel/src/ir/from_lex.rs` still  |
//! |                         |                        | owns the IR work.                     |
//! | `lex.tabular.table`     | [`LexBuiltinsHandler`] | Phase 1 of #570 — schema only.        |
//! |                         |                        | Legacy `VerbatimRegistry::TableHandler` |
//! |                         |                        | still parses pipe-tables.             |
//! | `lex.media.{image,…}`   | [`LexBuiltinsHandler`] | Phase 1 of #570 — schemas only.       |
//! |                         |                        | Legacy `VerbatimRegistry::Image/…`    |
//! |                         |                        | handlers still build the IR nodes.    |
//!
//! The single `lex` namespace is shared by every built-in label; the
//! composite [`LexBuiltinsHandler`] routes each dispatch by
//! [`LabelCtx::label`](lex_extension::wire::LabelCtx) to the right
//! sub-handler.

use std::sync::Arc;

use lex_extension::{
    handler::{HandlerError, LexHandler},
    schema::{
        BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, ParamSpec, ParamType, Schema,
    },
    wire::{LabelCtx, WireNode},
};
use lex_extension_host::registry::{Registry, RegistryError};

use crate::lex::includes::{Loader, ResolveConfig};

pub mod include;
pub mod media;
pub mod metadata;
pub mod tabular;

pub use include::LexIncludeHandler;

/// The reserved namespace owned by the lex core. Its prefix is
/// `lex.` (with the trailing dot), so registered labels look like
/// `lex.include`, `lex.metadata.title`, `lex.tabular.table`, etc.
pub const NAMESPACE: &str = "lex";

/// Composite handler for the `lex.*` namespace.
///
/// `Registry::register_namespace` accepts one handler per namespace; the
/// composite shape lets every `lex.*` built-in live under a single
/// namespace registration while keeping per-label logic isolated.
///
/// Today the only sub-handler with a non-stub implementation is
/// [`LexIncludeHandler`]; the `lex.metadata.*`, `lex.tabular.*`, and
/// `lex.media.*` labels register their schemas but their hooks return
/// the trait's default `Ok(None)` until Phase 3 of #570 moves the
/// legacy IR transformations into the registry path.
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
            // Phase 1 of #570: schemas register but don't intercept.
            // The legacy IR paths in lex-babel still own the work for
            // every other `lex.*` label. Phase 3 fills these arms in.
            _ => Ok(None),
        }
    }
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
    let mut schemas = Vec::with_capacity(13);
    schemas.push(lex_include_schema());
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
    fn dispatch_resolve_for_new_schemas_returns_none_in_phase_1() {
        // Phase 1 contract: schemas register but don't intercept.
        // Dispatch must succeed (no panic, no error) and return None
        // so the legacy IR paths continue to own the work until
        // Phase 3 retires them.
        let registry = fresh_registry();
        let labels: Vec<&str> = metadata::METADATA_LABELS
            .iter()
            .copied()
            .chain(std::iter::once(tabular::LEX_TABULAR_TABLE))
            .chain([
                media::LEX_MEDIA_IMAGE,
                media::LEX_MEDIA_VIDEO,
                media::LEX_MEDIA_AUDIO,
            ])
            .collect();
        for label in labels {
            let ctx = make_ctx(label, None, None);
            let result = registry
                .dispatch_resolve(&ctx)
                .unwrap_or_else(|e| panic!("dispatch_resolve({label}) errored: {e:?}"));
            assert!(
                result.is_none(),
                "dispatch_resolve({label}) must return None in Phase 1; got Some(...)"
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
}
