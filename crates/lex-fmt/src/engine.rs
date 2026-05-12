//! Public [`Engine`] facade for Rust embedders.
//!
//! `Engine` is the canonical entry point for any application embedding
//! Lex: docs pipelines, publishing servers, batch converters, custom
//! CLIs. It wraps the workspace's individual library crates
//! (`lex-core` parser, `lex-babel` format conversion, `lex-analysis`
//! semantic analysis, `lex-extension-host` registry) behind a single
//! `Engine::builder()` entry point so the embedder doesn't have to
//! wire the four together themselves.
//!
//! ## Usage
//!
//! ```ignore
//! use lex_fmt::Engine;
//!
//! let engine = Engine::builder()
//!     .workspace_root("/path/to/project")
//!     .load_lex_toml("/path/to/project/lex.toml")?
//!     .build()?;
//!
//! let doc = engine.resolve_source(source, Some(path.as_path()))?;
//! let diagnostics = engine.analyze(&doc);
//! let html = engine.render(&doc, "html")?;
//! ```
//!
//! With an in-process native handler:
//!
//! ```ignore
//! use lex_fmt::Engine;
//! use lex_extension::schema::Schema;
//!
//! let schema: Schema = serde_yaml::from_str(r#"
//! schema_version: 1
//! label: mit.plasma
//! hooks: { render: [html] }
//! "#)?;
//!
//! let engine = Engine::builder()
//!     .workspace_root(".")
//!     .with_native_namespace("mit.plasma", vec![schema], Box::new(MyPlasmaHandler))
//!     .build()?;
//! ```
//!
//! ## Concurrency
//!
//! `Engine` wraps its state in `Arc` so `engine.clone()` is a cheap
//! atomic bump. Embedders running web servers / batch pipelines can
//! share a single `Engine` across threads or async tasks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lex_analysis::diagnostics::{analyze_with_registry, AnalysisDiagnostic};
use lex_babel::error::FormatError;
use lex_babel::format::{Format, SerializedDocument};
use lex_babel::registry::FormatRegistry;
use lex_config::{LabelsConfig, LabelsConfigError};
use lex_core::lex::ast::Document;
use lex_core::lex::includes::{resolve_from_source, IncludeError, ResolveConfig};
use lex_core::lex::parsing::parse_document;
use lex_extension::schema::Schema;
use lex_extension::LexHandler;
use lex_extension_host::{Registry, RegistryError, Surface, TrustPromptHandler};

use crate::prompts::AutoDenyPrompt;
use crate::setup::{
    boot_registry, BootDiagnostic, BootOutcome, ExtensionSetup, RegisteredNamespace,
};

// ============================================================================
// Errors
// ============================================================================

/// Failure assembling an [`Engine`] from an [`EngineBuilder`].
///
/// Boot-time *diagnostics* (per-namespace resolver failures, denied
/// trust prompts, schema load errors for individual namespaces,
/// unreadable workspace root) are accumulated on the resulting
/// `Engine` and surfaced via [`Engine::diagnostics`] — they don't
/// fail the build. `BuildError` is reserved for the two things that
/// prevent the engine from existing at all today: a malformed
/// `lex.toml` ([`Self::LabelsConfig`]) and a native-namespace
/// collision with an already-registered namespace
/// ([`Self::NamespaceCollision`] / [`Self::InvalidNativeSchemas`]).
#[derive(Debug)]
pub enum BuildError {
    /// `load_lex_toml` couldn't read or parse the supplied path.
    LabelsConfig(LabelsConfigError),
    /// A native namespace registered via
    /// [`EngineBuilder::with_native_namespace`] collided with a
    /// namespace already registered through `boot_registry` (either
    /// a `lex.*` built-in or a `[labels]` entry). The embedder must
    /// either drop the `lex.toml` entry or skip the native
    /// registration.
    NamespaceCollision {
        namespace: String,
        source: RegistryError,
    },
    /// A native namespace's schemas referenced labels outside the
    /// namespace's prefix.
    InvalidNativeSchemas {
        namespace: String,
        source: RegistryError,
    },
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::LabelsConfig(e) => write!(f, "labels config: {e}"),
            BuildError::NamespaceCollision { namespace, source } => write!(
                f,
                "native namespace `{namespace}` collides with one already registered: {source}",
            ),
            BuildError::InvalidNativeSchemas { namespace, source } => write!(
                f,
                "native namespace `{namespace}`: schema validation failed: {source}",
            ),
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BuildError::LabelsConfig(e) => Some(e),
            BuildError::NamespaceCollision { source, .. }
            | BuildError::InvalidNativeSchemas { source, .. } => Some(source),
        }
    }
}

impl From<LabelsConfigError> for BuildError {
    fn from(value: LabelsConfigError) -> Self {
        BuildError::LabelsConfig(value)
    }
}

/// Failure parsing source text into a [`Document`].
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error: {}", self.message)
    }
}
impl std::error::Error for ParseError {}

/// Failure during the resolve pass (`lex.include` + third-party
/// resolve handlers). `IncludeError` carries a stack trace's worth of
/// chain context, so the variant is boxed to keep the `Result` small.
#[derive(Debug)]
pub enum ResolveError {
    /// Parser failed on the source text supplied to
    /// [`Engine::resolve_source`] (or on the re-parse step inside
    /// [`Engine::resolve`]).
    Parse(ParseError),
    /// [`Engine::resolve`]'s serialize→reparse round-trip couldn't
    /// turn the input [`Document`] back into source text. Distinct
    /// from `Parse` so callers can tell the document went *into*
    /// resolve broken from a downstream parse failure.
    Serialize(FormatError),
    /// The include resolver or a third-party `on_resolve` hook
    /// reported failure. See [`IncludeError`] for the chain.
    Include(Box<IncludeError>),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Parse(e) => write!(f, "{e}"),
            ResolveError::Serialize(e) => write!(f, "serialize before resolve: {e}"),
            ResolveError::Include(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for ResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ResolveError::Parse(e) => Some(e),
            ResolveError::Serialize(e) => Some(e),
            ResolveError::Include(e) => Some(e),
        }
    }
}

/// Failure during render (format conversion + `on_render` hook
/// dispatch).
#[derive(Debug)]
pub enum RenderError {
    Format(FormatError),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Format(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RenderError::Format(e) => Some(e),
        }
    }
}

impl From<FormatError> for RenderError {
    fn from(value: FormatError) -> Self {
        RenderError::Format(value)
    }
}

// ============================================================================
// Engine
// ============================================================================

/// The canonical Rust embedder API for the lex document format.
///
/// See the [module docs](crate::engine) for usage patterns. Construct
/// via [`Engine::builder`]; share across threads via [`Engine::clone`]
/// (cheap `Arc` bump).
#[derive(Clone)]
pub struct Engine {
    inner: Arc<EngineInner>,
}

struct EngineInner {
    registry: Arc<Registry>,
    formats: FormatRegistry,
    resolve_config: ResolveConfig,
    workspace_root: PathBuf,
    boot_diagnostics: Vec<BootDiagnostic>,
    registered: Vec<RegisteredNamespace>,
}

impl Engine {
    /// Start configuring an engine. See [`EngineBuilder`].
    pub fn builder() -> EngineBuilder {
        EngineBuilder::new()
    }

    // ----- pipeline -----

    /// Parse source text into a [`Document`]. No resolve hooks fire,
    /// no include splicing happens — the result is the literal AST of
    /// `source` with annotations attached as metadata.
    pub fn parse(&self, source: &str) -> Result<Document, ParseError> {
        parse_document(source).map_err(|message| ParseError { message })
    }

    /// Parse and resolve in one call. Splices `lex.include` and any
    /// third-party resolve-hook output into the tree; attaches
    /// annotations at the end. `source_path` (when supplied) anchors
    /// relative include paths and feeds cycle detection.
    pub fn resolve_source(
        &self,
        source: &str,
        source_path: Option<&Path>,
    ) -> Result<Document, ResolveError> {
        resolve_from_source(
            source,
            source_path.map(|p| p.to_path_buf()),
            &self.inner.resolve_config,
            &self.inner.registry,
        )
        .map_err(|e| ResolveError::Include(Box::new(e)))
    }

    /// Run the resolve pass on an already-parsed [`Document`].
    /// Implementation note: round-trips through serialize → parse →
    /// resolve, since the splice walker needs annotations in their
    /// pre-attach form. Acceptable for occasional use; prefer
    /// [`Engine::resolve_source`] when you have the source text.
    pub fn resolve(
        &self,
        doc: &Document,
        source_path: Option<&Path>,
    ) -> Result<Document, ResolveError> {
        let source = self
            .inner
            .formats
            .serialize(doc, "lex")
            .map_err(ResolveError::Serialize)?;
        self.resolve_source(&source, source_path)
    }

    /// Run semantic analysis on `doc`. Fires `on_label` and
    /// `on_validate` hooks for every labelled node whose namespace is
    /// registered; merges those diagnostics with the built-in
    /// semantic checks (missing definitions, unreachable footnotes,
    /// etc.).
    pub fn analyze(&self, doc: &Document) -> Vec<AnalysisDiagnostic> {
        analyze_with_registry(doc, &self.inner.registry)
    }

    /// Render `doc` as the named format (`"html"`, `"markdown"`,
    /// `"lex"`, …). Returns text output; use
    /// [`Engine::render_with_options`] for formats that may produce
    /// binary output (PDF, PNG when the `native-export` feature is
    /// enabled).
    ///
    /// **`on_render` hook splicing — current limitation.** Today this
    /// method routes through `FormatRegistry::serialize`, which calls
    /// the format's no-registry serializer path. Per-namespace
    /// `on_render` hooks are *not* invoked, and any handler-rendered
    /// content is *not* spliced into the output. The engine carries
    /// the registry to support hook dispatch once the format-by-format
    /// splice integration lands (tracked at lex-fmt/lex#546); until
    /// then, embedders that need on_render output must call the
    /// per-format registry-aware serializer directly (e.g.,
    /// `lex_babel::formats::html::serializer::serialize_to_html_with_registry`)
    /// using [`Engine::registry`].
    pub fn render(&self, doc: &Document, format: &str) -> Result<String, RenderError> {
        self.inner
            .formats
            .serialize(doc, format)
            .map_err(Into::into)
    }

    /// Render with format-specific options (e.g., HTML theme, PDF
    /// page size). Returns a [`SerializedDocument`] which is either
    /// text or binary depending on the format.
    ///
    /// Same `on_render` caveat as [`Engine::render`]: hook splicing
    /// awaits lex-fmt/lex#546.
    pub fn render_with_options(
        &self,
        doc: &Document,
        format: &str,
        options: &HashMap<String, String>,
    ) -> Result<SerializedDocument, RenderError> {
        self.inner
            .formats
            .serialize_with_options(doc, format, options)
            .map_err(Into::into)
    }

    // ----- observability -----

    /// The underlying extension registry. Exposed for embedders that
    /// need direct dispatch (e.g., custom analysis passes).
    pub fn registry(&self) -> &Registry {
        &self.inner.registry
    }

    /// Boot-time diagnostics surfaced by [`EngineBuilder::build`].
    /// Per-namespace failures (resolver errors, denied trust prompts,
    /// schema validation issues) appear here without aborting the
    /// build.
    pub fn diagnostics(&self) -> &[BootDiagnostic] {
        &self.inner.boot_diagnostics
    }

    /// Successfully registered namespaces, including the `lex.*`
    /// built-ins, every `[labels]` entry, every `--ext-schema`
    /// directory, and every native namespace registered via
    /// [`EngineBuilder::with_native_namespace`].
    pub fn registered_namespaces(&self) -> &[RegisteredNamespace] {
        &self.inner.registered
    }

    /// Names of every format known to this engine's render pipeline
    /// (built-in formats plus any registered via
    /// [`EngineBuilder::with_format`]).
    pub fn formats(&self) -> Vec<String> {
        self.inner.formats.list_formats()
    }

    /// Workspace root configured at build time. Anchors relative
    /// paths in include resolution and in resolver diagnostics.
    pub fn workspace_root(&self) -> &Path {
        &self.inner.workspace_root
    }
}

// ============================================================================
// EngineBuilder
// ============================================================================

/// Builder for [`Engine`]. Construct via [`Engine::builder`].
pub struct EngineBuilder {
    workspace_root: Option<PathBuf>,
    labels_config: LabelsConfig,
    ext_schemas: Vec<PathBuf>,
    enable_handlers: bool,
    surface_override: Option<Surface>,
    trust_prompt: Option<Box<dyn TrustPromptHandler>>,
    host_version: String,
    native_namespaces: Vec<NativeNamespaceEntry>,
    extra_formats: Vec<FormatRegistration>,
}

/// One pending (namespace, schemas, handler) tuple queued by
/// [`EngineBuilder::with_native_namespace`]. Registered into the
/// booted registry during [`EngineBuilder::build`].
type NativeNamespaceEntry = (String, Vec<Schema>, Box<dyn LexHandler>);

/// Deferred format registration captured by
/// [`EngineBuilder::with_format`]. Applied to the
/// [`FormatRegistry`] during [`EngineBuilder::build`].
type FormatRegistration = Box<dyn FnOnce(&mut FormatRegistry) + Send>;

impl EngineBuilder {
    fn new() -> Self {
        Self {
            workspace_root: None,
            labels_config: LabelsConfig::default(),
            ext_schemas: Vec::new(),
            enable_handlers: false,
            surface_override: None,
            trust_prompt: None,
            // `lex-fmt`'s own version. Embedders that want to identify
            // themselves to handlers (a publishing server's version,
            // say) should override via [`Self::host_version`].
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            native_namespaces: Vec::new(),
            extra_formats: Vec::new(),
        }
    }

    /// Set the workspace root. Anchors `[labels]` URI resolution,
    /// include path resolution, and the trust store location. Defaults
    /// to the current directory when [`Self::build`] is called
    /// without an explicit setting.
    pub fn workspace_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(path.into());
        self
    }

    /// Install a pre-parsed labels block (typically constructed in
    /// tests or by an embedder that owns its config format).
    pub fn labels_config(mut self, cfg: LabelsConfig) -> Self {
        self.labels_config = cfg;
        self
    }

    /// Load a `lex.toml` file's `[labels]` block. Replaces any
    /// previously-configured labels config — call this *before*
    /// [`Self::labels_config`] if you want the in-memory config to
    /// override the file. Other sections of the file (`[formatting]`,
    /// `[convert]`, `[includes]`, …) are ignored. Missing files
    /// return the default empty config; only IO / parse errors
    /// surface as [`BuildError::LabelsConfig`].
    pub fn load_lex_toml(mut self, path: impl AsRef<Path>) -> Result<Self, BuildError> {
        let path = path.as_ref();
        match lex_config::load_labels_from_toml(path) {
            Ok(cfg) => {
                self.labels_config = cfg;
                Ok(self)
            }
            Err(LabelsConfigError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(self)
            }
            Err(e) => Err(BuildError::LabelsConfig(e)),
        }
    }

    /// Add a directory of YAML schemas to register as a namespace.
    /// The namespace name is inferred from the directory's basename
    /// (e.g., `./schemas/acme/` registers as `acme`). Repeatable.
    pub fn ext_schema_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.ext_schemas.push(path.into());
        self
    }

    /// Equivalent of the CLI's `--enable-handlers` flag. With
    /// `surface = CliOneShot`, this bypasses the trust prompt for
    /// subprocess handlers — the user explicitly opted in.
    pub fn enable_handlers(mut self, on: bool) -> Self {
        self.enable_handlers = on;
        self
    }

    /// Override the auto-detected host surface. The default checks
    /// CI environment variables and returns [`Surface::Ci`] or
    /// [`Surface::CliOneShot`].
    pub fn surface(mut self, s: Surface) -> Self {
        self.surface_override = Some(s);
        self
    }

    /// Install a custom trust prompt. Defaults to
    /// [`AutoDenyPrompt`](crate::prompts::AutoDenyPrompt), which is
    /// the safe choice for non-interactive embedders.
    pub fn trust_prompt(mut self, prompt: Box<dyn TrustPromptHandler>) -> Self {
        self.trust_prompt = Some(prompt);
        self
    }

    /// Override the host version reported to subprocess handlers in
    /// the `initialize` handshake. Defaults to `lex-fmt`'s own
    /// version.
    pub fn host_version(mut self, v: impl Into<String>) -> Self {
        self.host_version = v.into();
        self
    }

    /// Register an in-process native [`LexHandler`] alongside its
    /// schemas. This is the embedder-only path the proposal §7.1
    /// promises — no IPC, no subprocess to spawn, type-safe payloads.
    /// Schema validation (label prefix, attaches_to, params) runs at
    /// build time the same way it does for subprocess handlers.
    ///
    /// Collisions with a namespace already loaded via `boot_registry`
    /// (a `lex.*` built-in, a `[labels]` entry, or an `ext_schema_dir`)
    /// fail [`Self::build`] with
    /// [`BuildError::NamespaceCollision`]. The embedder must remove
    /// the conflicting entry from `lex.toml` (or skip the native
    /// registration) to resolve.
    pub fn with_native_namespace(
        mut self,
        namespace: impl Into<String>,
        schemas: impl IntoIterator<Item = Schema>,
        handler: Box<dyn LexHandler>,
    ) -> Self {
        self.native_namespaces
            .push((namespace.into(), schemas.into_iter().collect(), handler));
        self
    }

    /// Register an additional output [`Format`] in the engine's
    /// render pipeline. Built-in formats (`lex`, `html`, `markdown`,
    /// `rfc-xml`, `tag`, `treeviz`, `linetreeviz`, and — under the
    /// `native-export` feature — `pdf`, `png`) are always registered.
    pub fn with_format<F: Format + Send + 'static>(mut self, format: F) -> Self {
        self.extra_formats
            .push(Box::new(move |reg| reg.register(format)));
        self
    }

    /// Assemble the engine. See [`BuildError`] for failure modes.
    pub fn build(self) -> Result<Engine, BuildError> {
        let workspace_root = self
            .workspace_root
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));

        let trust_prompt: Box<dyn TrustPromptHandler> = self
            .trust_prompt
            .unwrap_or_else(|| Box::new(AutoDenyPrompt));

        let outcome: BootOutcome = boot_registry(ExtensionSetup {
            workspace_root: &workspace_root,
            labels_config: &self.labels_config,
            ext_schemas: &self.ext_schemas,
            enable_handlers: self.enable_handlers,
            surface_override: self.surface_override,
            trust_prompt,
            host_version: &self.host_version,
        });

        // Native namespaces register on top of the booted registry.
        // The registry already uses interior mutability, so we can
        // mutate through the Arc that boot_registry returned.
        let mut registered = outcome.registered;
        let mut boot_diagnostics = outcome.diagnostics;
        let registry = outcome.registry;

        for (name, schemas, handler) in self.native_namespaces {
            let schema_count = schemas.len();
            match registry.register_namespace(name.clone(), schemas, handler) {
                Ok(()) => {
                    registered.push(RegisteredNamespace {
                        name: name.clone(),
                        source: crate::setup::NamespaceSourceKind::Native,
                        schema_count,
                    });
                }
                Err(source @ RegistryError::NamespaceAlreadyRegistered { .. }) => {
                    return Err(BuildError::NamespaceCollision {
                        namespace: name,
                        source,
                    });
                }
                Err(other) => {
                    return Err(BuildError::InvalidNativeSchemas {
                        namespace: name,
                        source: other,
                    });
                }
            }
        }

        // Capture root-level dispatch diagnostics (handler panics
        // during registration, schema mismatches caught at dispatch
        // time, etc.) and lift them into boot diagnostics so the
        // embedder sees one accumulated list.
        for root_diag in registry.take_root_diagnostics() {
            let message = match &root_diag.code {
                Some(code) => format!("[{}] {}", code, root_diag.message),
                None => root_diag.message.clone(),
            };
            boot_diagnostics.push(BootDiagnostic {
                namespace: None,
                message,
            });
        }

        let mut formats = FormatRegistry::with_defaults();
        for register_fn in self.extra_formats {
            register_fn(&mut formats);
        }

        // The resolve config is anchored to the canonicalised
        // workspace root so include path checks see the same prefix
        // as boot_registry's lex.include built-in. boot_registry
        // already canonicalises and falls back gracefully on macOS
        // /var → /private/var symlinks; mirror that policy here so
        // the Engine and the registry agree on root.
        //
        // If canonicalize fails (rare — non-existent path, permission
        // error), fall back to an absolute path derived from the
        // current directory + the configured root. `ResolveConfig`'s
        // root-escape prefix check compares byte-for-byte, so handing
        // it a relative path would silently weaken include security.
        let resolve_root = match workspace_root.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                let fallback = if workspace_root.is_absolute() {
                    workspace_root.clone()
                } else {
                    std::env::current_dir()
                        .map(|cwd| cwd.join(&workspace_root))
                        .unwrap_or_else(|_| workspace_root.clone())
                };
                boot_diagnostics.push(BootDiagnostic {
                    namespace: None,
                    message: format!(
                        "could not canonicalize workspace root `{}`: {e}; \
                         using `{}` for Engine::resolve_source — \
                         include root-escape checks may be weakened",
                        workspace_root.display(),
                        fallback.display(),
                    ),
                });
                fallback
            }
        };
        let resolve_config = ResolveConfig::with_root(resolve_root);

        Ok(Engine {
            inner: Arc::new(EngineInner {
                registry,
                formats,
                resolve_config,
                workspace_root,
                boot_diagnostics,
                registered,
            }),
        })
    }
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
fn _assert_engine_send_sync() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Engine>();
}
