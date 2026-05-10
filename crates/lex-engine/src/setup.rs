//! Boot the extension registry from host inputs (workspace + `lex.toml` +
//! ext-schema directories + a host-supplied trust prompt).
//!
//! Glue between a lex host (the `lexd` CLI, `lexd-lsp` server, or an
//! embedder using the future `Engine::builder()`) and the
//! `lex-extension-host` runtime. Given:
//!
//! - the workspace root (where the `lex.toml` lives),
//! - the `[labels]` block from the parsed config,
//! - the list of `--ext-schema <path>` directories (or equivalent),
//! - the `--enable-handlers` flag (CLI; `false` for LSP/embedder by default),
//! - the host surface (CliOneShot / Lsp / Embedded / Ci, auto-detected on
//!   `None`),
//! - a [`TrustPromptHandler`] specific to the host (CLI prompts deny with
//!   a message about `--enable-handlers`; LSP forwards a `lex/trustRequest`
//!   notification; embedders inject whatever fits their UX),
//!
//! returns a fully-populated [`Registry`] with the bundled `lex.*`
//! built-ins plus every namespace declared in `[labels]` plus every
//! ext-schema directory passed in. Surfaces diagnostics for unresolvable
//! namespaces / unsupported transports / trust-gate denials but doesn't
//! fail the boot — a single bad namespace shouldn't prevent the rest of
//! the host from working.
//!
//! ## Subprocess instantiation
//!
//! Schemas declaring `handler: { transport: subprocess, command: [...] }`
//! are routed through the trust gate. On `Trusted`, the boot helper
//! calls `SubprocessHandler::spawn(...)` and registers the live
//! handler. On `Denied`, the namespace registers schema-only
//! (`NoopHandler`) so analysis-pass pre-validation still catches
//! typos, but `on_validate` / `on_render` etc. return the trait
//! defaults; the user-facing diagnostic explains why.
//!
//! v1 invariant: every schema in a namespace either declares the
//! same `handler` block or declares none. Mixed handler specs
//! across labels in a namespace are surfaced as a diagnostic and
//! treated as schema-only.
//!
//! ## What's NOT here
//!
//! - Network resolvers (github:/gitlab:/https:/git+ssh:). Those
//!   live in the resolver and return `Unimplemented`; this boot
//!   surfaces the error and continues. Tracked at lex#546.
//! - Third-party `transport: native` handlers. Only the bundled
//!   `lex.*` built-ins use the native transport in v1; user-provided
//!   native handlers would need an in-process registration path
//!   the boot doesn't expose.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lex_config::LabelsConfig;
use lex_core::lex::builtins;
use lex_core::lex::includes::{FsLoader, ResolveConfig};
use lex_extension::schema::{HandlerSpec, HandlerTransport, Schema};
use lex_extension::LexHandler;
use lex_extension_host::transport::{SpawnEnv, SubprocessHandler};
use lex_extension_host::{
    detect_ci_environment, resolve_namespace, Capability, Registry, RegistryError, ResolveError,
    SchemaError, SchemaLoader, Source, Surface, Transport, TrustDecision, TrustGate,
    TrustPromptHandler, TrustStore,
};

/// Inputs the boot helper needs from the host layer.
pub struct ExtensionSetup<'a> {
    pub workspace_root: &'a Path,
    pub labels_config: &'a LabelsConfig,
    /// Each `--ext-schema <path>` flag on the command line (CLI), or an
    /// equivalent host-provided list. LSP / embedders typically pass an
    /// empty slice.
    pub ext_schemas: &'a [PathBuf],
    /// `--enable-handlers` global flag (CLI). `false` for LSP/embedder
    /// hosts whose trust prompt drives the decision.
    pub enable_handlers: bool,
    /// Surface override. `None` auto-detects from env vars (CI →
    /// `Surface::Ci`, otherwise `Surface::CliOneShot`). Hosts that know
    /// their own surface (LSP / embedder) should pass it explicitly.
    pub surface_override: Option<Surface>,
    /// Host-supplied trust prompt. The CLI installs a prompt that denies
    /// with a `--enable-handlers` rationale; the LSP installs one that
    /// forwards a `lex/trustRequest` notification to the editor;
    /// embedders inject whatever fits their UX (or an "auto-deny" stub
    /// for batch / non-interactive use).
    pub trust_prompt: Box<dyn TrustPromptHandler>,
    /// Host crate version (typically `env!("CARGO_PKG_VERSION")`)
    /// reported to subprocess handlers in their `initialize`
    /// handshake. The host (`lexd` / `lexd-lsp` / an embedder) supplies
    /// its own version, *not* `lex-engine`'s — handlers expect to see
    /// the host they're running under, not the boot helper crate.
    pub host_version: &'a str,
}

/// One namespace that was successfully registered, surfaced for
/// `lexd labels list` output.
#[derive(Debug, Clone)]
pub struct RegisteredNamespace {
    pub name: String,
    pub source: NamespaceSourceKind,
    pub schema_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamespaceSourceKind {
    Builtin,
    LexToml { uri: String },
    ExtSchemaFlag { path: PathBuf },
}

/// One thing that went wrong during boot. Surfaced as a diagnostic
/// rather than a hard error so a bad namespace doesn't prevent the
/// host from running with the others.
#[derive(Debug, Clone)]
pub struct BootDiagnostic {
    pub namespace: Option<String>,
    pub message: String,
}

/// Outcome of [`boot_registry`]: the registry plus per-namespace
/// metadata for `lexd labels list` plus the diagnostic stream.
pub struct BootOutcome {
    pub registry: Arc<Registry>,
    pub trust_gate: TrustGate,
    pub registered: Vec<RegisteredNamespace>,
    pub diagnostics: Vec<BootDiagnostic>,
}

/// Construct a registry from the host inputs. Always succeeds — bad
/// namespaces become entries in `diagnostics` rather than aborting
/// the boot.
pub fn boot_registry(setup: ExtensionSetup<'_>) -> BootOutcome {
    let mut registry = Registry::new();
    let mut registered = Vec::new();
    let mut diagnostics = Vec::new();

    // Trust gate is built up-front so per-namespace registration
    // can consult it when deciding whether to spawn a real
    // SubprocessHandler vs fall back to NoopHandler. The previous
    // design left registration as schema-only (NoopHandler always)
    // and constructed the gate after; correct trust-gated dispatch
    // for subprocess transports needs the gate alive during
    // registration.
    let surface = setup.surface_override.unwrap_or_else(|| {
        if detect_ci_environment(|name| std::env::var(name).ok()) {
            Surface::Ci
        } else {
            Surface::CliOneShot
        }
    });
    let store = TrustStore::open(setup.workspace_root).unwrap_or_else(|e| {
        // Fall back to a per-process tempdir under `std::env::temp_dir()`.
        // The PID suffix keeps unrelated runs from sharing trust
        // decisions through `/tmp/.lex/trust.json` (which the previous
        // fallback did — accidentally persistent and shared). With a
        // PID-keyed path the decisions persist within this run but
        // can't be re-read by the next session.
        let fallback_dir = std::env::temp_dir()
            .join(format!("lexd-trust-fallback-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&fallback_dir);
        diagnostics.push(BootDiagnostic {
            namespace: None,
            message: format!(
                "trust store open failed at workspace root: {e}; falling back to per-process temp dir `{}` — decisions persist for this run only",
                fallback_dir.display()
            ),
        });
        TrustStore::open(&fallback_dir).expect("temp dir writable")
    });
    let mut trust_gate = TrustGate::new(surface, setup.enable_handlers, store, setup.trust_prompt);
    let host_version = setup.host_version;

    // Built-ins: lex.* namespace (currently `lex.include`). Native
    // transport, trusted by linkage — gate consultation is a no-op
    // for native handlers, so the registration goes through
    // directly. Loader + ResolveConfig are pointed at the workspace
    // root so `lex.include` resolves relative paths the same way
    // `lexd convert` and `lexd inspect` do.
    let resolve_config = ResolveConfig::with_root(setup.workspace_root.to_path_buf());
    let loader = FsLoader::new(setup.workspace_root.to_path_buf());
    match builtins::register_into(&registry, Arc::new(loader), resolve_config) {
        Ok(()) => registered.push(RegisteredNamespace {
            name: "lex".into(),
            source: NamespaceSourceKind::Builtin,
            schema_count: 1,
        }),
        Err(e) => diagnostics.push(BootDiagnostic {
            namespace: Some("lex".into()),
            message: format!("failed to register lex.* built-ins: {e}"),
        }),
    }

    // [labels] block — resolved through the URI resolver.
    for (name, spec) in &setup.labels_config.namespaces {
        let uri = match spec.canonical_uri() {
            Ok(u) => u,
            Err(e) => {
                diagnostics.push(BootDiagnostic {
                    namespace: Some(name.clone()),
                    message: format!("invalid namespace spec: {e}"),
                });
                continue;
            }
        };
        match resolve_namespace(&uri, setup.workspace_root) {
            Ok(resolved) => {
                let source = Source::LexTomlNamespace { name: name.clone() };
                match register_schema_dir(
                    &mut registry,
                    &mut trust_gate,
                    name,
                    &resolved.schema_dir,
                    &source,
                    setup.workspace_root,
                    host_version,
                    &mut diagnostics,
                ) {
                    Ok(count) => registered.push(RegisteredNamespace {
                        name: name.clone(),
                        source: NamespaceSourceKind::LexToml { uri: uri.clone() },
                        schema_count: count,
                    }),
                    Err(e) => diagnostics.push(BootDiagnostic {
                        namespace: Some(name.clone()),
                        message: format!("failed to register schemas: {e}"),
                    }),
                }
            }
            Err(ResolveError::Unimplemented { .. }) => {
                diagnostics.push(BootDiagnostic {
                    namespace: Some(name.clone()),
                    message: format!(
                        "namespace `{name}` uses a remote URI scheme that's not yet implemented; use `path:` or `--ext-schema` for now (uri: `{uri}`)"
                    ),
                });
            }
            Err(e) => diagnostics.push(BootDiagnostic {
                namespace: Some(name.clone()),
                message: format!("namespace resolve failed: {e}"),
            }),
        }
    }

    // --ext-schema flags — local schema directories, no resolver.
    for path in setup.ext_schemas {
        let dir = if path.is_absolute() {
            path.clone()
        } else {
            setup.workspace_root.join(path)
        };
        let namespace = match infer_namespace_from_dir(&dir) {
            Ok(n) => n,
            Err(msg) => {
                diagnostics.push(BootDiagnostic {
                    namespace: None,
                    message: format!("--ext-schema {}: {msg}", path.display()),
                });
                continue;
            }
        };
        let source = Source::LocalFile { path: path.clone() };
        match register_schema_dir(
            &mut registry,
            &mut trust_gate,
            &namespace,
            &dir,
            &source,
            setup.workspace_root,
            host_version,
            &mut diagnostics,
        ) {
            Ok(count) => registered.push(RegisteredNamespace {
                name: namespace,
                source: NamespaceSourceKind::ExtSchemaFlag { path: path.clone() },
                schema_count: count,
            }),
            Err(e) => diagnostics.push(BootDiagnostic {
                namespace: Some(namespace),
                message: format!("--ext-schema {}: {e}", path.display()),
            }),
        }
    }

    BootOutcome {
        registry: Arc::new(registry),
        trust_gate,
        registered,
        diagnostics,
    }
}

/// Try to infer the namespace name from a schema directory. Convention:
/// the directory's basename is the namespace name. (Future work: read
/// a `namespace.toml` sidecar file if it exists.)
fn infer_namespace_from_dir(dir: &Path) -> Result<String, String> {
    dir.file_name()
        .and_then(|s| s.to_str())
        .map(String::from)
        .ok_or_else(|| {
            format!(
                "cannot infer namespace name from directory `{}`",
                dir.display()
            )
        })
}

/// Load every YAML schema in `dir`, decide which handler to back
/// the namespace with (subprocess via the trust gate, or schema-
/// only `NoopHandler`), and register everything under `namespace`.
/// Returns the schema count for `lexd labels list` output.
///
/// Handler selection:
///
/// - If no schema in the namespace declares a `handler` block,
///   register a `NoopHandler`. Pre-validation still runs, but
///   hook events return the trait defaults.
/// - If schemas declare *different* handler specs, surface a
///   diagnostic and fall back to `NoopHandler`. v1 assumes one
///   binary serves the whole namespace.
/// - If the unique handler spec is `transport: subprocess`,
///   consult the trust gate. On `Trusted`, spawn the binary and
///   register the live `SubprocessHandler`. On `Denied`, surface
///   the gate's reason and register `NoopHandler` (so schema
///   pre-validation still catches typos).
/// - `transport: native` is rejected with a "third-party native
///   handlers are not supported in v1" diagnostic — only bundled
///   `lex.*` built-ins use the native transport, and they go
///   through `builtins::register_into` not this path.
/// - `transport: wasm` is rejected by the schema loader before it
///   reaches us; defensive.
// 8-argument boot helper. Bundling the inputs into a struct just to
// satisfy this lint adds boilerplate without simplifying anything; the
// signature is read top-to-bottom in one site (`boot_registry`).
#[allow(clippy::too_many_arguments)]
fn register_schema_dir(
    registry: &mut Registry,
    trust_gate: &mut TrustGate,
    namespace: &str,
    dir: &Path,
    source: &Source,
    workspace_root: &Path,
    host_version: &str,
    diagnostics: &mut Vec<BootDiagnostic>,
) -> Result<usize, RegisterError> {
    let schemas: Vec<Schema> =
        SchemaLoader::load_dir(dir).map_err(|e| RegisterError::Schema(Box::new(e)))?;
    if schemas.is_empty() {
        return Err(RegisterError::EmptyDir(dir.to_path_buf()));
    }
    let count = schemas.len();
    let handler = build_handler(
        &schemas,
        namespace,
        source,
        workspace_root,
        host_version,
        trust_gate,
        diagnostics,
    );
    registry
        .register_namespace(namespace, schemas, handler)
        .map_err(RegisterError::Registry)?;
    Ok(count)
}

/// Decide which `LexHandler` to back a namespace with. Side-effect:
/// pushes a diagnostic onto `diagnostics` for any non-trusted /
/// non-spawnable case before returning the fallback `NoopHandler`.
fn build_handler(
    schemas: &[Schema],
    namespace: &str,
    source: &Source,
    workspace_root: &Path,
    host_version: &str,
    trust_gate: &mut TrustGate,
    diagnostics: &mut Vec<BootDiagnostic>,
) -> Box<dyn LexHandler> {
    let contract = match find_namespace_contract(schemas, namespace) {
        Ok(c) => c,
        Err(msg) => {
            diagnostics.push(BootDiagnostic {
                namespace: Some(namespace.into()),
                message: msg,
            });
            return Box::new(NoopHandler);
        }
    };
    let handler_spec = match contract.handler {
        Some(spec) => spec,
        None => return Box::new(NoopHandler),
    };

    match handler_spec.transport {
        HandlerTransport::Subprocess => {}
        HandlerTransport::Native => {
            diagnostics.push(BootDiagnostic {
                namespace: Some(namespace.into()),
                message: format!(
                    "namespace `{namespace}` declares transport: native — third-party native handlers are not supported in v1; only bundled lex.* built-ins use the native transport"
                ),
            });
            return Box::new(NoopHandler);
        }
        HandlerTransport::Wasm => {
            // Schema loader rejects WASM upstream; reaching this
            // branch is defensive.
            diagnostics.push(BootDiagnostic {
                namespace: Some(namespace.into()),
                message: format!("namespace `{namespace}`: WASM transport is deferred for v1"),
            });
            return Box::new(NoopHandler);
        }
        // HandlerTransport is #[non_exhaustive]; future variants
        // conservatively register schema-only with a diagnostic.
        _ => {
            diagnostics.push(BootDiagnostic {
                namespace: Some(namespace.into()),
                message: format!(
                    "namespace `{namespace}` declares an unrecognised transport; registering schema-only"
                ),
            });
            return Box::new(NoopHandler);
        }
    }

    // Subprocess transport — consult the trust gate.
    let command_string = handler_spec.command.join(" ");
    let capability = Capability::from_schema(contract.capabilities);
    let transport = Transport::from_schema(handler_spec.transport);
    let decision = trust_gate.evaluate(source, transport, capability, namespace, &command_string);
    match decision {
        TrustDecision::Trusted => {}
        TrustDecision::Denied { reason } => {
            diagnostics.push(BootDiagnostic {
                namespace: Some(namespace.into()),
                message: reason,
            });
            return Box::new(NoopHandler);
        }
        TrustDecision::Pending => {
            // Defensive — the host's prompt callback should always
            // resolve to Trusted/Denied. Pending here is a host bug.
            diagnostics.push(BootDiagnostic {
                namespace: Some(namespace.into()),
                message: format!(
                    "trust gate returned Pending for `{namespace}` — host prompt did not resolve the decision"
                ),
            });
            return Box::new(NoopHandler);
        }
    }

    // Trusted — spawn the binary.
    let labels: Vec<String> = schemas.iter().map(|s| s.label.clone()).collect();
    let env = SpawnEnv {
        workspace_root: Some(workspace_root.display().to_string()),
        lex_cache: None,
        handler_config: None,
    };
    match SubprocessHandler::spawn(
        handler_spec,
        namespace,
        &labels,
        contract.capabilities,
        host_version,
        &env,
    ) {
        Ok(h) => Box::new(h),
        Err(e) => {
            diagnostics.push(BootDiagnostic {
                namespace: Some(namespace.into()),
                message: format!(
                    "trusted but failed to spawn `{namespace}` handler ({command_string}): {e}"
                ),
            });
            Box::new(NoopHandler)
        }
    }
}

/// One namespace's handler + capability contract, validated as
/// uniform across all the namespace's schemas.
///
/// v1 invariant (called out in the issue scope and now enforced
/// here): within a namespace, every schema must agree on its
/// `handler` block AND its `capabilities` block. Mixed `handler`
/// (some schemas declare it, others don't, or differ in shape)
/// or mixed `capabilities` (different `fs`/`net` declarations
/// across labels) are misconfiguration and surface as a diagnostic.
struct NamespaceContract<'a> {
    handler: Option<&'a HandlerSpec>,
    capabilities: lex_extension::schema::Capabilities,
}

/// Validate that every schema in `schemas` agrees on its `handler`
/// and `capabilities` blocks, then return the consensus.
fn find_namespace_contract<'a>(
    schemas: &'a [Schema],
    namespace: &str,
) -> Result<NamespaceContract<'a>, String> {
    debug_assert!(!schemas.is_empty(), "caller must pass a non-empty slice");
    let first = &schemas[0];
    let first_handler = first.handler.as_ref();
    let first_caps = first.capabilities;

    for s in schemas.iter().skip(1) {
        if s.handler.as_ref() != first_handler {
            return Err(format!(
                "namespace `{namespace}` has schemas declaring inconsistent `handler` blocks (label `{}` differs from `{}`); v1 requires every schema in a namespace to declare the same handler — or all of them to declare none",
                s.label, schemas[0].label
            ));
        }
        if s.capabilities != first_caps {
            return Err(format!(
                "namespace `{namespace}` has schemas declaring inconsistent `capabilities` blocks (label `{}` differs from `{}`); v1 requires uniform capabilities per namespace",
                s.label, schemas[0].label
            ));
        }
    }

    Ok(NamespaceContract {
        handler: first_handler,
        capabilities: first_caps,
    })
}

#[derive(Debug)]
enum RegisterError {
    Schema(Box<SchemaError>),
    Registry(RegistryError),
    EmptyDir(PathBuf),
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterError::Schema(e) => write!(f, "schema load: {e}"),
            RegisterError::Registry(e) => write!(f, "registry register: {e}"),
            RegisterError::EmptyDir(p) => write!(
                f,
                "schema directory `{}` contains no .yaml files",
                p.display()
            ),
        }
    }
}

/// No-op handler used for schema-only registration. Returns the
/// trait defaults for every hook; the dispatcher's pre-validation
/// pass still surfaces schema mismatches even with this stub.
struct NoopHandler;
impl lex_extension::LexHandler for NoopHandler {}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_config::{NamespaceSpec, NamespaceTable};
    use lex_extension_host::TrustPromptContext;
    use std::collections::BTreeMap;

    /// Test prompt that always denies — equivalent to the CLI's
    /// `--enable-handlers`-not-set behavior, but free of any CLI
    /// vocabulary so the tests in this crate stay host-agnostic.
    struct DenyAllPrompt;
    impl TrustPromptHandler for DenyAllPrompt {
        fn prompt(&self, ctx: &TrustPromptContext) -> TrustDecision {
            TrustDecision::Denied {
                reason: format!(
                    "subprocess handler `{}` denied by host (test stub)",
                    ctx.namespace
                ),
            }
        }
    }

    fn write_schema(dir: &Path, label: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(format!("{}.yaml", label.replace('.', "_"))),
            format!("schema_version: 1\nlabel: {label}\n"),
        )
        .unwrap();
    }

    /// Schema with a subprocess handler block. Used to drive the
    /// trust-gate instantiation paths.
    fn write_schema_with_subprocess_handler(dir: &Path, label: &str, command: &[&str]) {
        std::fs::create_dir_all(dir).unwrap();
        let cmd_yaml = command
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        std::fs::write(
            dir.join(format!("{}.yaml", label.replace('.', "_"))),
            format!(
                "schema_version: 1\n\
                 label: {label}\n\
                 handler:\n  \
                 transport: subprocess\n  \
                 command: [{cmd_yaml}]\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn boot_with_empty_inputs_yields_only_builtin() {
        let workspace = tempfile::tempdir().unwrap();
        let labels = LabelsConfig::default();
        let setup = ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[],
            enable_handlers: false,
            surface_override: Some(Surface::CliOneShot),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        };
        let outcome = boot_registry(setup);
        assert_eq!(outcome.registered.len(), 1);
        assert_eq!(outcome.registered[0].name, "lex");
        assert_eq!(outcome.registered[0].source, NamespaceSourceKind::Builtin);
        assert!(outcome.diagnostics.is_empty());
    }

    #[test]
    fn boot_with_path_uri_in_labels_registers_namespace() {
        let workspace = tempfile::tempdir().unwrap();
        let acme_dir = workspace.path().join("acme-labels");
        write_schema(&acme_dir, "acme.task");

        let mut namespaces = BTreeMap::new();
        namespaces.insert("acme".into(), NamespaceSpec::Uri("path:acme-labels".into()));
        let labels = LabelsConfig { namespaces };

        let setup = ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[],
            enable_handlers: false,
            surface_override: Some(Surface::CliOneShot),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        };
        let outcome = boot_registry(setup);
        let acme = outcome
            .registered
            .iter()
            .find(|r| r.name == "acme")
            .expect("acme registered");
        assert_eq!(acme.schema_count, 1);
        assert!(matches!(acme.source, NamespaceSourceKind::LexToml { .. }));
        assert!(outcome.diagnostics.is_empty());
    }

    #[test]
    fn boot_with_remote_uri_emits_unimplemented_diagnostic() {
        let workspace = tempfile::tempdir().unwrap();
        let mut namespaces = BTreeMap::new();
        namespaces.insert(
            "acme".into(),
            NamespaceSpec::Table(NamespaceTable {
                tap: Some("acme".into()),
                ..Default::default()
            }),
        );
        let labels = LabelsConfig { namespaces };

        let setup = ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[],
            enable_handlers: false,
            surface_override: Some(Surface::CliOneShot),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        };
        let outcome = boot_registry(setup);
        assert_eq!(outcome.diagnostics.len(), 1);
        let diag = &outcome.diagnostics[0];
        assert_eq!(diag.namespace.as_deref(), Some("acme"));
        assert!(diag.message.contains("not yet implemented"));
        assert!(!outcome.registered.iter().any(|r| r.name == "acme"));
    }

    #[test]
    fn boot_with_ext_schema_flag_registers_namespace() {
        let workspace = tempfile::tempdir().unwrap();
        let acme_dir = workspace.path().join("acme");
        write_schema(&acme_dir, "acme.task");

        let labels = LabelsConfig::default();
        let setup = ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[acme_dir.clone()],
            enable_handlers: false,
            surface_override: Some(Surface::CliOneShot),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        };
        let outcome = boot_registry(setup);
        let acme = outcome
            .registered
            .iter()
            .find(|r| r.name == "acme")
            .expect("registered");
        assert!(matches!(
            acme.source,
            NamespaceSourceKind::ExtSchemaFlag { .. }
        ));
    }

    /// Subprocess handler + LSP surface + denying prompt: the trust
    /// gate consults the prompt (LSP doesn't short-circuit on
    /// `enable_handlers` the way CLI does), the prompt denies, and
    /// the namespace registers schema-only with the prompt's
    /// rationale surfaced as a diagnostic.
    #[test]
    fn subprocess_handler_in_lsp_surface_with_denying_prompt_is_schema_only() {
        let workspace = tempfile::tempdir().unwrap();
        let acme_dir = workspace.path().join("acme");
        write_schema_with_subprocess_handler(&acme_dir, "acme.task", &["acme-handler"]);

        let labels = LabelsConfig::default();
        let setup = ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[acme_dir.clone()],
            enable_handlers: false,
            surface_override: Some(Surface::LspSession),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        };
        let outcome = boot_registry(setup);
        // Namespace IS registered (so pre-validation still works
        // when analysing documents), but with the prompt's reason
        // as a diagnostic.
        assert!(outcome.registered.iter().any(|r| r.name == "acme"));
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.namespace.as_deref() == Some("acme")
                    && d.message.contains("denied by host (test stub)")),
            "expected prompt's denial reason, got: {:?}",
            outcome.diagnostics
        );
    }

    /// Subprocess handler + `--enable-handlers`: the trust gate
    /// trusts the namespace and `SubprocessHandler::spawn` is
    /// called. The test points the schema at a non-existent path
    /// — `spawn` then fails at `Command::spawn` with a clear "no
    /// such file" error, surfaced as a `trusted but failed to spawn`
    /// diagnostic. Verifying the diagnostic exists is what proves
    /// the trust gate let the request through.
    #[test]
    fn subprocess_handler_with_enable_flag_attempts_spawn() {
        let workspace = tempfile::tempdir().unwrap();
        let acme_dir = workspace.path().join("acme");
        write_schema_with_subprocess_handler(
            &acme_dir,
            "acme.task",
            &["/this/binary/does/not/exist"],
        );

        let labels = LabelsConfig::default();
        let outcome = boot_registry(ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[acme_dir.clone()],
            enable_handlers: true,
            surface_override: Some(Surface::CliOneShot),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        });
        assert!(outcome.registered.iter().any(|r| r.name == "acme"));
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.message.contains("failed to spawn")),
            "expected spawn-failure diagnostic, got: {:?}",
            outcome.diagnostics
        );
    }

    /// Mixed handler specs across schemas in one namespace is
    /// rejected with a clear diagnostic. v1 requires one handler
    /// per namespace.
    #[test]
    fn mixed_handler_specs_in_one_namespace_emit_diagnostic() {
        let workspace = tempfile::tempdir().unwrap();
        let acme_dir = workspace.path().join("acme");
        write_schema_with_subprocess_handler(&acme_dir, "acme.task", &["acme-v1"]);
        write_schema_with_subprocess_handler(&acme_dir, "acme.note", &["acme-v2"]);

        let labels = LabelsConfig::default();
        let outcome = boot_registry(ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[acme_dir],
            enable_handlers: true,
            surface_override: Some(Surface::CliOneShot),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        });
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.message.contains("inconsistent `handler` blocks")),
            "expected mixed-handler diagnostic, got: {:?}",
            outcome.diagnostics
        );
    }

    /// Partial handler declaration — some schemas in the namespace
    /// declare a `handler` block, others don't — also surfaces as
    /// "inconsistent handler blocks".
    #[test]
    fn partial_handler_declaration_in_one_namespace_emits_diagnostic() {
        let workspace = tempfile::tempdir().unwrap();
        let acme_dir = workspace.path().join("acme");
        write_schema_with_subprocess_handler(&acme_dir, "acme.task", &["acme-bin"]);
        write_schema(&acme_dir, "acme.note");

        let labels = LabelsConfig::default();
        let outcome = boot_registry(ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[acme_dir],
            enable_handlers: true,
            surface_override: Some(Surface::CliOneShot),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        });
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.message.contains("inconsistent `handler` blocks")),
            "expected partial-handler diagnostic, got: {:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn mixed_capabilities_in_one_namespace_emit_diagnostic() {
        let workspace = tempfile::tempdir().unwrap();
        let acme_dir = workspace.path().join("acme");
        std::fs::create_dir_all(&acme_dir).unwrap();
        std::fs::write(
            acme_dir.join("task.yaml"),
            "schema_version: 1\nlabel: acme.task\ncapabilities: { fs: true, net: false }\n",
        )
        .unwrap();
        std::fs::write(
            acme_dir.join("note.yaml"),
            "schema_version: 1\nlabel: acme.note\ncapabilities: { fs: false, net: true }\n",
        )
        .unwrap();

        let labels = LabelsConfig::default();
        let outcome = boot_registry(ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[acme_dir],
            enable_handlers: false,
            surface_override: Some(Surface::CliOneShot),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        });
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.message.contains("inconsistent `capabilities`")),
            "expected mixed-capabilities diagnostic, got: {:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn ext_schema_flag_with_no_yaml_files_emits_diagnostic() {
        let workspace = tempfile::tempdir().unwrap();
        let empty_dir = workspace.path().join("empty");
        std::fs::create_dir(&empty_dir).unwrap();

        let labels = LabelsConfig::default();
        let setup = ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[empty_dir],
            enable_handlers: false,
            surface_override: Some(Surface::CliOneShot),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        };
        let outcome = boot_registry(setup);
        assert!(outcome
            .diagnostics
            .iter()
            .any(|d| d.message.contains("contains no .yaml files")));
    }

    #[test]
    fn surface_override_threads_through_to_trust_gate() {
        let workspace = tempfile::tempdir().unwrap();
        let labels = LabelsConfig::default();
        let setup = ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[],
            enable_handlers: true,
            surface_override: Some(Surface::Ci),
            trust_prompt: Box::new(DenyAllPrompt),
            host_version: "test",
        };
        let outcome = boot_registry(setup);
        assert_eq!(outcome.trust_gate.surface(), Surface::Ci);
        assert!(outcome.trust_gate.enable_handlers());
    }
}
