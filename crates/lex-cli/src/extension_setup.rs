//! Boot the extension registry from CLI flags + `lex.toml`.
//!
//! Glue between the CLI argument layer and the
//! `lex-extension-host` runtime. Given:
//!
//! - the workspace root (where the `lex.toml` lives),
//! - the list of `--ext-schema <path>` flags,
//! - the `--enable-handlers` flag,
//! - the host surface (CliOneShot / Ci, auto-detected),
//!
//! returns a fully-populated [`Registry`] with the bundled `lex.*`
//! built-ins plus every namespace declared in `[labels]` plus every
//! `--ext-schema` directory passed on the command line. Surfaces
//! diagnostics for unresolvable namespaces / unsupported transports
//! / trust-gate denials but doesn't fail the boot — a single bad
//! namespace shouldn't prevent the rest of the host from working.
//!
//! ## What's NOT here
//!
//! - Trust-gate-driven subprocess spawning. The boot helper records
//!   trust decisions but doesn't actually instantiate
//!   `SubprocessHandler`s yet — that would couple the boot with the
//!   subprocess feature, and consumers (lex-cli, lex-lsp, future
//!   lexed) want the shape of the boot independent of which
//!   transports they enable. Subprocess spawning sits in a thin
//!   adapter layer above this module; for now the boot registers
//!   schema-only namespaces (no handler), which surfaces in
//!   diagnostics and the `lexd labels list` output but doesn't run
//!   render/validate hooks.
//! - Network resolvers (github:/gitlab:/https:/git+ssh:). Those
//!   live in the resolver and return `Unimplemented`; this boot
//!   surfaces the error and continues.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lex_config::LabelsConfig;
use lex_core::lex::builtins;
use lex_core::lex::includes::{FsLoader, ResolveConfig};
use lex_extension::schema::Schema;
use lex_extension_host::{
    detect_ci_environment, resolve_namespace, Registry, RegistryError, ResolveError, SchemaError,
    SchemaLoader, Surface, TrustGate, TrustPromptContext, TrustPromptHandler, TrustStore,
};

/// Inputs the boot helper needs from the CLI layer.
pub struct ExtensionSetup<'a> {
    pub workspace_root: &'a Path,
    pub labels_config: &'a LabelsConfig,
    /// Each `--ext-schema <path>` flag on the command line.
    pub ext_schemas: &'a [PathBuf],
    /// `--enable-handlers` global flag.
    pub enable_handlers: bool,
    /// Surface override. `None` auto-detects from env vars (CI →
    /// `Surface::Ci`, otherwise `Surface::CliOneShot`).
    pub surface_override: Option<Surface>,
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

/// Construct a registry from the CLI inputs. Always succeeds — bad
/// namespaces become entries in `diagnostics` rather than aborting
/// the boot.
pub fn boot_registry(setup: ExtensionSetup<'_>) -> BootOutcome {
    let mut registry = Registry::new();
    let mut registered = Vec::new();
    let mut diagnostics = Vec::new();

    // Built-ins: lex.* namespace (currently `lex.include`). The
    // boot helper actually performs the registration so that
    // `lexd labels list` reports the truth and the registry handed
    // back to the caller has the built-in handlers wired. Loader +
    // ResolveConfig are pointed at the workspace root so
    // `lex.include` resolves relative paths the same way `lexd
    // convert` and `lexd inspect` do.
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
            Ok(resolved) => match register_schema_dir(&mut registry, name, &resolved.schema_dir) {
                Ok(count) => registered.push(RegisteredNamespace {
                    name: name.clone(),
                    source: NamespaceSourceKind::LexToml { uri: uri.clone() },
                    schema_count: count,
                }),
                Err(e) => diagnostics.push(BootDiagnostic {
                    namespace: Some(name.clone()),
                    message: format!("failed to register schemas: {e}"),
                }),
            },
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
        match register_schema_dir(&mut registry, &namespace, &dir) {
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
    let trust_gate = TrustGate::new(
        surface,
        setup.enable_handlers,
        store,
        Box::new(CliPromptHandler),
    );

    BootOutcome {
        registry: Arc::new(registry),
        trust_gate,
        registered,
        diagnostics,
    }
}

/// CLI surface installs this prompt: trust prompts in CLI mode are
/// a TTY-interrupt anti-pattern, so we always deny and direct the
/// user at `--enable-handlers`.
struct CliPromptHandler;
impl TrustPromptHandler for CliPromptHandler {
    fn prompt(&self, ctx: &TrustPromptContext) -> lex_extension_host::TrustDecision {
        lex_extension_host::TrustDecision::Denied {
            reason: format!(
                "subprocess handler `{}` requires --enable-handlers in CLI mode",
                ctx.namespace
            ),
        }
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

/// Load every YAML schema in `dir` and register them under
/// `namespace`. Returns the count for `lexd labels list` output.
fn register_schema_dir(
    registry: &mut Registry,
    namespace: &str,
    dir: &Path,
) -> Result<usize, RegisterError> {
    let schemas: Vec<Schema> =
        SchemaLoader::load_dir(dir).map_err(|e| RegisterError::Schema(Box::new(e)))?;
    if schemas.is_empty() {
        return Err(RegisterError::EmptyDir(dir.to_path_buf()));
    }
    let count = schemas.len();
    // Schema-only registration (no handler): the dispatcher in
    // analysis/render will see the schemas in `dispatch_*` calls
    // for pre-validation, but `on_validate` / `on_render` etc. are
    // no-ops because there's no handler. The follow-up that wires
    // subprocess transports will replace this with a real handler
    // pulled from the schema's `handler.command`.
    let stub: Box<dyn lex_extension::LexHandler> = Box::new(NoopHandler);
    registry
        .register_namespace(namespace, schemas, stub)
        .map_err(RegisterError::Registry)?;
    Ok(count)
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
    use std::collections::BTreeMap;

    fn write_schema(dir: &Path, label: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(format!("{}.yaml", label.replace('.', "_"))),
            format!("schema_version: 1\nlabel: {label}\n"),
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
        };
        let outcome = boot_registry(setup);
        assert_eq!(outcome.diagnostics.len(), 1);
        let diag = &outcome.diagnostics[0];
        assert_eq!(diag.namespace.as_deref(), Some("acme"));
        assert!(diag.message.contains("not yet implemented"));
        // No `acme` in `registered` (only `lex` builtin).
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
        };
        let outcome = boot_registry(setup);
        assert_eq!(outcome.trust_gate.surface(), Surface::Ci);
        assert!(outcome.trust_gate.enable_handlers());
    }
}
