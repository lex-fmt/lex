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
//! | Label         | Handler              | Status                            |
//! |---------------|----------------------|-----------------------------------|
//! | `lex.include` | [`LexIncludeHandler`] | Registrable; resolve pass still   |
//! |               |                      | runs through the legacy inline    |
//! |               |                      | path until PR 3d (#533).          |
//!
//! Future built-ins (`lex.toc`, …) follow the same shape: one handler
//! impl per label, registered through [`register_into`].

use std::sync::Arc;

use lex_extension::schema::{
    BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, ParamSpec, ParamType, Schema,
};
use lex_extension_host::registry::{Registry, RegistryError};

use crate::lex::includes::{Loader, ResolveConfig};

pub mod include;

pub use include::LexIncludeHandler;

/// The reserved namespace owned by the lex core. Its prefix is
/// `lex.` (with the trailing dot), so registered labels look like
/// `lex.include`, `lex.toc`, etc.
pub const NAMESPACE: &str = "lex";

/// Register every built-in `lex.*` schema and handler into `registry`.
///
/// The current set is `{ lex.include }`; future built-ins land here as
/// they're added. The single registration call covers the whole
/// namespace because [`Registry::register_namespace`] wants every
/// label for a namespace handed in atomically.
///
/// `loader` and `config` flow into [`LexIncludeHandler`] verbatim: the
/// handler holds them by `Arc` so the registry can be cheaply cloned
/// and shared across threads. `config.root` should already be
/// canonicalised by the caller — see
/// [`ResolveConfig::with_root`](crate::lex::includes::ResolveConfig::with_root).
pub fn register_into(
    registry: &Registry,
    loader: Arc<dyn Loader + Send + Sync>,
    config: ResolveConfig,
) -> Result<(), RegistryError> {
    let schemas = vec![lex_include_schema()];
    let handler = Box::new(LexIncludeHandler::new(loader, config));
    registry.register_namespace(NAMESPACE, schemas, handler)
}

/// Schema for the `lex.include` label. Inlined here because v1 has
/// exactly one built-in label; once the YAML schema loader lands in
/// PR 4 (#520), built-ins will share the same load path as third
/// parties (a baked-in `lex.yaml` shipped with the crate).
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

    fn make_ctx(src: &str, host_origin: Option<&str>) -> LabelCtx {
        LabelCtx {
            label: "lex.include".into(),
            params: serde_json::json!({ "src": src }),
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

    #[test]
    fn register_into_attaches_namespace_and_schema() {
        let mut loader = MemoryLoader::new();
        loader.insert(PathBuf::from("/root/inner.lex"), "Hello.\n");
        let registry = Registry::new();
        register_into(
            &registry,
            Arc::new(loader),
            ResolveConfig::with_root(PathBuf::from("/root")),
        )
        .expect("registration ok");

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
        let mut loader = MemoryLoader::new();
        loader.insert(PathBuf::from("/root/inner.lex"), "Spliced paragraph.\n");
        let registry = Registry::new();
        register_into(
            &registry,
            Arc::new(loader),
            ResolveConfig::with_root(PathBuf::from("/root")),
        )
        .expect("registration ok");

        let ctx = make_ctx("inner.lex", Some("/root/host.lex"));
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
        let loader = MemoryLoader::new();
        let registry = Registry::new();
        register_into(
            &registry,
            Arc::new(loader),
            ResolveConfig::with_root(PathBuf::from("/root")),
        )
        .expect("registration ok");

        let ctx = make_ctx("missing.lex", Some("/root/host.lex"));
        let err = registry
            .dispatch_resolve(&ctx)
            .expect_err("registry must surface the load error");
        // The diagnostic uses `handler.custom` for our `Custom`-coded
        // errors — see Registry::dispatch's error mapping.
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
}
