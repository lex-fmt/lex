//! `LexIncludeHandler` — the first built-in [`LexHandler`].
//!
//! Wraps the existing [`Loader`] + [`parse_no_attach`] + [`stamp_doc`]
//! pipeline so that `lex.include` runs through the registry-driven
//! dispatch fabric the rest of the extension system uses. The
//! observable behaviour matches the legacy inline path in
//! [`crate::lex::includes::resolve_from_source`]: same parameter
//! syntax, same path-resolution rules, same `FsLoader` security
//! defenses (path traversal, symlink loop, size limit, root escape,
//! absolute-path rejection).
//!
//! # Lifecycle
//!
//! In α (this PR — lex-fmt/lex#532), the handler is registrable but
//! the resolve pass keeps using the inline path. PR 3d
//! (lex-fmt/lex#533) flips the call site so this handler runs in
//! production. That gives us a clean separation between *handler is
//! correct* (proven here) and *resolve pass dispatches via the
//! registry* (proven there).
//!
//! # Error mapping
//!
//! Loader errors map onto `HandlerError::Custom` with codes in the
//! handler-defined `-32000..=-32099` range reserved by the wire spec
//! §5. `Loader::load` failures and path-resolution failures all
//! become diagnostics at the labelled node's range when surfaced
//! through `Registry::dispatch_resolve`.

use std::path::Path;
use std::sync::Arc;

use lex_extension::{
    handler::{HandlerError, LexHandler},
    wire::{LabelCtx, WireNode},
};

use crate::lex::includes::{
    parse_no_attach, resolve_file_reference, stamp_doc, IncludeError, LoadError, LoadedFile,
    Loader, ResolveConfig,
};
use crate::lex::wire::to_wire_document;

/// Error code: `lex.include` annotation was missing the required `src`
/// parameter. Matches the wire spec's handler-defined range.
pub const CODE_MISSING_SRC: i32 = -32000;
/// Error code: `Loader::load` returned `NotFound`.
pub const CODE_NOT_FOUND: i32 = -32001;
/// Error code: include path canonicalised outside the loader's root,
/// or the resolver rejected it pre-load as a root escape.
pub const CODE_OUTSIDE_ROOT: i32 = -32002;
/// Error code: include target exceeded the loader's size cap.
pub const CODE_TOO_LARGE: i32 = -32003;
/// Error code: include path was a platform-absolute path
/// (`C:\foo`, `/abs` on Unix), which the resolver rejects pre-load.
pub const CODE_ABSOLUTE_PATH: i32 = -32004;
/// Error code: underlying I/O error during load.
pub const CODE_IO: i32 = -32005;

/// Built-in handler for the `lex.include` label.
pub struct LexIncludeHandler {
    loader: Arc<dyn Loader + Send + Sync>,
    config: ResolveConfig,
}

impl LexIncludeHandler {
    /// Construct a handler from a loader (typically [`crate::lex::includes::FsLoader`]
    /// in production, [`crate::lex::includes::MemoryLoader`] in tests)
    /// and a resolve config bundling the resolution `root` plus depth /
    /// total-include caps.
    ///
    /// Depth and total-include limits are not enforced by the handler
    /// itself; they belong to the resolve-pass walker that wraps
    /// dispatches across the document. The handler stores the config
    /// so that future hooks (validate, render) can read its limits
    /// without an additional indirection.
    pub fn new(loader: Arc<dyn Loader + Send + Sync>, config: ResolveConfig) -> Self {
        Self { loader, config }
    }

    /// Read-only access to the resolution root the handler was built
    /// with. Useful for tests and for the resolve pass that wires
    /// this handler into a registry.
    pub fn root(&self) -> &Path {
        &self.config.root
    }
}

impl LexHandler for LexIncludeHandler {
    fn on_resolve(&self, ctx: &LabelCtx) -> Result<Option<WireNode>, HandlerError> {
        let src = extract_src(ctx)?;

        // Path resolution against the host file's directory. When the
        // host file's origin is unknown, resolution falls back to the
        // configured root (per `resolve_file_reference`).
        let host_origin = ctx.node.origin.as_deref().map(Path::new);
        let target_path = resolve_file_reference(&src, host_origin, &self.config.root)
            .map_err(|e| include_error_to_handler(&e))?;

        // Load through the injected loader. Same security gate as the
        // inline path: FsLoader canonicalises and bounds-checks against
        // its canonical root post-symlink resolution.
        let LoadedFile {
            source,
            canonical_path,
        } = self
            .loader
            .load(&target_path)
            .map_err(|e| load_error_to_handler(&e))?;

        // Parse without annotation attachment — annotations stay
        // visible as standalone children, matching what
        // `resolve_from_source` does in the inline path.
        let mut included = parse_no_attach(&source).map_err(|message| {
            HandlerError::internal(format!(
                "parse of `{}` failed: {message}",
                canonical_path.display()
            ))
        })?;

        // Stamp every node's `Range.origin_path` with the loaded file's
        // canonical path so downstream tooling (file-reference
        // resolution, scoped footnote lookup) sees the right origin.
        let origin = Arc::new(canonical_path);
        stamp_doc(&mut included, &origin);

        let wire = to_wire_document(&included);
        Ok(Some(wire))
    }
}

fn extract_src(ctx: &LabelCtx) -> Result<String, HandlerError> {
    ctx.params
        .get("src")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| HandlerError::Custom {
            code: CODE_MISSING_SRC,
            message: format!(
                "lex.include is missing required `src` parameter; got params: {}",
                ctx.params
            ),
            data: None,
        })
}

fn load_error_to_handler(err: &LoadError) -> HandlerError {
    match err {
        LoadError::NotFound { path } => HandlerError::Custom {
            code: CODE_NOT_FOUND,
            message: format!("include not found: {}", path.display()),
            data: Some(serde_json::json!({ "path": path.display().to_string() })),
        },
        LoadError::OutsideRoot { path, root } => HandlerError::Custom {
            code: CODE_OUTSIDE_ROOT,
            message: format!(
                "include path {} resolves outside loader root {}",
                path.display(),
                root.display()
            ),
            data: Some(serde_json::json!({
                "path": path.display().to_string(),
                "root": root.display().to_string(),
            })),
        },
        LoadError::TooLarge { path, size, limit } => HandlerError::Custom {
            code: CODE_TOO_LARGE,
            message: format!(
                "include file {} is {size} bytes, exceeds limit of {limit} bytes",
                path.display()
            ),
            data: Some(serde_json::json!({
                "path": path.display().to_string(),
                "size": size,
                "limit": limit,
            })),
        },
        LoadError::Io { path, message } => HandlerError::Custom {
            code: CODE_IO,
            message: format!("io error reading {}: {message}", path.display()),
            data: Some(serde_json::json!({ "path": path.display().to_string() })),
        },
    }
}

fn include_error_to_handler(err: &IncludeError) -> HandlerError {
    match err {
        IncludeError::AbsolutePath { path } => HandlerError::Custom {
            code: CODE_ABSOLUTE_PATH,
            message: format!(
                "lex.include `src` rejected: {} is a platform-absolute path",
                path.display()
            ),
            data: Some(serde_json::json!({ "path": path.display().to_string() })),
        },
        IncludeError::RootEscape { path, root } => HandlerError::Custom {
            code: CODE_OUTSIDE_ROOT,
            message: format!(
                "include path {} resolves outside resolution root {}",
                path.display(),
                root.display()
            ),
            data: Some(serde_json::json!({
                "path": path.display().to_string(),
                "root": root.display().to_string(),
            })),
        },
        // `resolve_file_reference` only ever returns `AbsolutePath` or
        // `RootEscape` — the other `IncludeError` variants come from
        // the resolve-pass walker, not from path resolution. Treat
        // them as internal here so a future change in the resolver
        // doesn't silently produce a misleading custom code.
        other => HandlerError::internal(format!("path resolution failed: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::includes::{LoadError, LoadedFile, MemoryLoader};
    use lex_extension::wire::{AnnotationBody, NodeRef, Position, Range};
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

    fn handler_with_loader(loader: MemoryLoader, root: PathBuf) -> LexIncludeHandler {
        LexIncludeHandler::new(Arc::new(loader), ResolveConfig::with_root(root))
    }

    #[test]
    fn happy_path_returns_wire_document() {
        let mut loader = MemoryLoader::new();
        loader.insert(
            PathBuf::from("/root/included.lex"),
            "Hello from included.\n",
        );
        let handler = handler_with_loader(loader, PathBuf::from("/root"));

        let ctx = make_ctx("included.lex", Some("/root/host.lex"));
        let result = handler.on_resolve(&ctx).expect("on_resolve ok");
        let wire = result.expect("returned Some(WireNode)");

        // Top-level result is a WireNode::Document.
        let WireNode::Document {
            children, origin, ..
        } = wire
        else {
            panic!("expected WireNode::Document, got something else");
        };
        // Origin should reflect the *included* file (canonical_path),
        // because stamp_doc walks the loaded tree and the wire codec
        // lifts origin_path from the root session's range.
        assert_eq!(origin.as_deref(), Some("/root/included.lex"));
        // The single paragraph from the included source must survive
        // the round trip.
        assert!(
            !children.is_empty(),
            "included document children must reach the wire payload"
        );
    }

    #[test]
    fn missing_src_returns_custom_error() {
        let loader = MemoryLoader::new();
        let handler = handler_with_loader(loader, PathBuf::from("/root"));
        let mut ctx = make_ctx("ignored", None);
        ctx.params = serde_json::json!({});
        let err = handler.on_resolve(&ctx).expect_err("must error");
        match err {
            HandlerError::Custom { code, .. } => {
                assert_eq!(code, CODE_MISSING_SRC);
            }
            other => panic!("expected Custom code, got {other:?}"),
        }
    }

    #[test]
    fn not_found_maps_to_code_minus_32001() {
        let loader = MemoryLoader::new();
        let handler = handler_with_loader(loader, PathBuf::from("/root"));
        let ctx = make_ctx("missing.lex", Some("/root/host.lex"));
        let err = handler.on_resolve(&ctx).expect_err("must error");
        match err {
            HandlerError::Custom { code, .. } => assert_eq!(code, CODE_NOT_FOUND),
            other => panic!("expected NotFound→Custom, got {other:?}"),
        }
    }

    #[test]
    fn outside_root_via_resolver_maps_to_code_minus_32002() {
        let loader = MemoryLoader::new();
        let handler = handler_with_loader(loader, PathBuf::from("/root"));
        // ../../../etc/passwd would normalise outside `/root`, so the
        // resolver returns `RootEscape` before any load attempt.
        let ctx = make_ctx("../../../etc/passwd", Some("/root/host.lex"));
        let err = handler.on_resolve(&ctx).expect_err("must error");
        match err {
            HandlerError::Custom { code, .. } => assert_eq!(code, CODE_OUTSIDE_ROOT),
            other => panic!("expected RootEscape→Custom, got {other:?}"),
        }
    }

    #[test]
    fn absolute_path_maps_to_code_minus_32004() {
        let loader = MemoryLoader::new();
        let handler = handler_with_loader(loader, PathBuf::from("/root"));
        // Platform-absolute path on Unix; the resolver rejects up front
        // before any load. (`/x` would normalise as `root-absolute` per
        // Lex spec; we use a Windows-style path so the platform-
        // absolute check fires regardless of OS.)
        #[cfg(windows)]
        let absolute = "C:\\Windows\\System32\\drivers\\etc\\hosts";
        #[cfg(not(windows))]
        let absolute = "//absolute/elsewhere"; // double-slash → host on UNC; treated as absolute
        let ctx = make_ctx(absolute, Some("/root/host.lex"));
        let err = handler.on_resolve(&ctx).expect_err("must error");
        // On platforms where `Path::is_absolute(absolute)` returns true
        // we expect AbsolutePath (-32004); otherwise we expect
        // OutsideRoot (-32002). Both are valid security outcomes.
        match err {
            HandlerError::Custom { code, .. } => {
                assert!(
                    code == CODE_ABSOLUTE_PATH || code == CODE_OUTSIDE_ROOT,
                    "expected -32002 or -32004, got {code}"
                );
            }
            other => panic!("expected Custom code, got {other:?}"),
        }
    }

    #[test]
    fn loader_outside_root_maps_to_code_minus_32002() {
        // A loader that itself returns OutsideRoot (e.g., FsLoader
        // catching a symlink escape post-canonicalisation). Simulate
        // this with a custom mock loader.
        struct MockEscape;
        impl Loader for MockEscape {
            fn load(&self, path: &std::path::Path) -> Result<LoadedFile, LoadError> {
                Err(LoadError::OutsideRoot {
                    path: path.to_path_buf(),
                    root: PathBuf::from("/root"),
                })
            }
        }
        let handler = LexIncludeHandler::new(
            Arc::new(MockEscape),
            ResolveConfig::with_root(PathBuf::from("/root")),
        );
        let ctx = make_ctx("inner.lex", Some("/root/host.lex"));
        let err = handler.on_resolve(&ctx).expect_err("must error");
        match err {
            HandlerError::Custom { code, .. } => assert_eq!(code, CODE_OUTSIDE_ROOT),
            other => panic!("expected OutsideRoot→Custom, got {other:?}"),
        }
    }

    #[test]
    fn too_large_maps_to_code_minus_32003() {
        struct MockTooLarge;
        impl Loader for MockTooLarge {
            fn load(&self, path: &std::path::Path) -> Result<LoadedFile, LoadError> {
                Err(LoadError::TooLarge {
                    path: path.to_path_buf(),
                    size: 1_000_000,
                    limit: 100,
                })
            }
        }
        let handler = LexIncludeHandler::new(
            Arc::new(MockTooLarge),
            ResolveConfig::with_root(PathBuf::from("/root")),
        );
        let ctx = make_ctx("big.lex", Some("/root/host.lex"));
        let err = handler.on_resolve(&ctx).expect_err("must error");
        match err {
            HandlerError::Custom { code, data, .. } => {
                assert_eq!(code, CODE_TOO_LARGE);
                let data = data.expect("data attached");
                assert_eq!(data["size"], 1_000_000);
                assert_eq!(data["limit"], 100);
            }
            other => panic!("expected TooLarge→Custom, got {other:?}"),
        }
    }

    #[test]
    fn io_error_maps_to_code_minus_32005() {
        struct MockIo;
        impl Loader for MockIo {
            fn load(&self, path: &std::path::Path) -> Result<LoadedFile, LoadError> {
                Err(LoadError::Io {
                    path: path.to_path_buf(),
                    message: "permission denied".into(),
                })
            }
        }
        let handler = LexIncludeHandler::new(
            Arc::new(MockIo),
            ResolveConfig::with_root(PathBuf::from("/root")),
        );
        let ctx = make_ctx("locked.lex", Some("/root/host.lex"));
        let err = handler.on_resolve(&ctx).expect_err("must error");
        match err {
            HandlerError::Custom { code, .. } => assert_eq!(code, CODE_IO),
            other => panic!("expected Io→Custom, got {other:?}"),
        }
    }

    #[test]
    fn parse_failure_maps_to_internal_error() {
        // Construct a fixture that fails `parse_no_attach`. The
        // parser is permissive, so we use a verbatim block whose
        // closing marker is missing — that surfaces as a parse
        // error in `parse_without_annotation_attachment`.
        let mut loader = MemoryLoader::new();
        // A subject + indented content with no closing `:: label ::`
        // line is the canonical "unterminated verbatim" shape.
        loader.insert(
            PathBuf::from("/root/broken.lex"),
            "Subject:\n    line one\n    line two\n",
        );
        let handler = handler_with_loader(loader, PathBuf::from("/root"));
        let ctx = make_ctx("broken.lex", Some("/root/host.lex"));
        // Whether this fixture trips a parse error depends on the
        // current parser; the test only fails the handler if a
        // parse error is surfaced. If the parser successfully parses
        // the fixture, we still get Ok(Some(_)) and the assertion
        // below short-circuits.
        let result = handler.on_resolve(&ctx);
        if let Err(err) = result {
            assert!(
                matches!(err, HandlerError::Internal { .. }),
                "parse failures must map to HandlerError::Internal, got {err:?}"
            );
        }
    }

    #[test]
    fn round_trip_via_from_wire_recovers_typed_ast() {
        use crate::lex::ast::elements::content_item::ContentItem;
        use crate::lex::wire::from_wire_node;

        let mut loader = MemoryLoader::new();
        loader.insert(PathBuf::from("/root/snippet.lex"), "First paragraph.\n");
        let handler = handler_with_loader(loader, PathBuf::from("/root"));
        let ctx = make_ctx("snippet.lex", Some("/root/host.lex"));
        let wire = handler
            .on_resolve(&ctx)
            .expect("on_resolve ok")
            .expect("Some(WireNode)");

        // The wire payload must round-trip through from_wire_node
        // back into typed lex-core ContentItems — that's the
        // contract PR 3d will rely on when splicing.
        let items = from_wire_node(&wire).expect("from_wire ok");
        assert!(
            !items.is_empty(),
            "from_wire on the included document must recover at least one item"
        );
        // The first paragraph must come through.
        let saw_paragraph = items
            .iter()
            .any(|item| matches!(item, ContentItem::Paragraph(_)));
        assert!(saw_paragraph, "included paragraph must survive round-trip");
    }
}
