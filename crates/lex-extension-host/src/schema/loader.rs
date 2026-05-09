//! YAML loader for `lex_extension::Schema`.
//!
//! Reads one schema per `.yaml`/`.yml` file. Deserialisation is strict:
//! unknown fields are rejected (see `Schema`'s `deny_unknown_fields`
//! attribute). After deserialisation, the loader runs a validator pass for
//! the invariants serde can't enforce on its own — see the `validate`
//! function below for the canonical list.
//!
//! All errors carry the offending `path` so that a directory-load failure
//! points the user at the right file, not just the directory.

use std::fs;
use std::path::{Path, PathBuf};

use lex_extension::schema::{HandlerTransport, ParamType, Schema};

/// One schema file failed to load. Variants distinguish *post-deserialise*
/// validation failures by class so callers can pattern-match on the cause.
/// The deserialise step is one variant — `Parse` — because serde_yaml
/// reports missing required fields, wrong-typed fields, and unknown fields
/// (rejected by `deny_unknown_fields`) all through the same error path
/// with line/column attribution baked into the message.
#[derive(Debug)]
#[non_exhaustive]
pub enum SchemaError {
    /// Reading the file (or, for `load_dir`, listing the directory or one
    /// of its entries) failed at the OS level.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    /// The YAML body did not deserialise into a [`Schema`]. The message
    /// is whatever serde_yaml produced — which carries line/column
    /// information when attribution is possible. Reasons covered by
    /// this single variant: missing required field, wrong-typed field,
    /// unknown field rejected by `deny_unknown_fields`, malformed YAML.
    Parse { path: PathBuf, message: String },

    /// `schema_version` is set to a value the loader doesn't support.
    /// Currently only `1` is recognised; future versions land with
    /// dedicated migration paths, not a permissive accept.
    UnsupportedSchemaVersion {
        path: PathBuf,
        label: String,
        version: u32,
    },

    /// `attaches_to` referenced a node kind outside the closed set
    /// `{paragraph, definition, session, annotation, list_item, verbatim}`.
    UnknownNodeKind {
        path: PathBuf,
        label: String,
        kind: String,
    },

    /// A param declared `type: enum` but its `values` list is empty.
    EmptyEnumValues {
        path: PathBuf,
        label: String,
        param: String,
    },

    /// Two `EnumValue` entries on the same param share a name.
    DuplicateEnumValue {
        path: PathBuf,
        label: String,
        param: String,
        value: String,
    },

    /// An `EnumValue` was declared with an empty `name`. The empty
    /// string isn't a useful identifier and almost always indicates a
    /// schema typo (`- name:` with no value).
    EmptyEnumValueName {
        path: PathBuf,
        label: String,
        param: String,
    },

    /// `verbatim_label: true` was set on a label that can't legally appear
    /// as a verbatim block closing — typically because it contains
    /// whitespace or the verbatim-marker sequence `::`.
    InvalidVerbatimLabel {
        path: PathBuf,
        label: String,
        reason: String,
    },

    /// `handler.transport: wasm` is reserved for a future release. The
    /// loader rejects it with a clear deferral message rather than the
    /// generic "unknown variant" error so users get an actionable hint.
    WasmTransportDeferred { path: PathBuf, label: String },

    /// `handler.transport: subprocess` declared without a non-empty
    /// `command` array — the subprocess transport has nothing to spawn.
    EmptySubprocessCommand { path: PathBuf, label: String },

    /// `handler.transport` is a value the loader doesn't understand.
    /// `HandlerTransport` is `#[non_exhaustive]` upstream; reaching this
    /// branch means a future variant slipped past serde without being
    /// taught to the validator. Surfacing it as an error rather than a
    /// silent accept keeps lockstep with `lex-extension`.
    UnsupportedTransport {
        path: PathBuf,
        label: String,
        transport: String,
    },
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaError::Io { path, source } => {
                write!(f, "{}: io error: {source}", path.display())
            }
            SchemaError::Parse { path, message } => {
                write!(f, "{}: schema parse error: {message}", path.display())
            }
            SchemaError::UnsupportedSchemaVersion {
                path,
                label,
                version,
            } => write!(
                f,
                "{}: schema for `{label}` declares schema_version: {version} (this loader supports only version 1)",
                path.display()
            ),
            SchemaError::UnknownNodeKind { path, label, kind } => write!(
                f,
                "{}: schema for `{label}` lists unknown node kind `{kind}` in attaches_to (allowed: paragraph, definition, session, annotation, list_item, verbatim)",
                path.display()
            ),
            SchemaError::EmptyEnumValues { path, label, param } => write!(
                f,
                "{}: schema for `{label}` declares param `{param}` as enum but provides no values",
                path.display()
            ),
            SchemaError::DuplicateEnumValue {
                path,
                label,
                param,
                value,
            } => write!(
                f,
                "{}: schema for `{label}` lists duplicate enum value `{value}` on param `{param}`",
                path.display()
            ),
            SchemaError::EmptyEnumValueName { path, label, param } => write!(
                f,
                "{}: schema for `{label}` has an empty enum value name on param `{param}`",
                path.display()
            ),
            SchemaError::InvalidVerbatimLabel {
                path,
                label,
                reason,
            } => write!(
                f,
                "{}: schema for `{label}` sets verbatim_label: true but the label is not legal as a verbatim closing ({reason})",
                path.display()
            ),
            SchemaError::WasmTransportDeferred { path, label } => write!(
                f,
                "{}: schema for `{label}` declares transport: wasm — the WASM transport is deferred for v1; use subprocess or native",
                path.display()
            ),
            SchemaError::EmptySubprocessCommand { path, label } => write!(
                f,
                "{}: schema for `{label}` declares transport: subprocess but provides an empty command array",
                path.display()
            ),
            SchemaError::UnsupportedTransport {
                path,
                label,
                transport,
            } => write!(
                f,
                "{}: schema for `{label}` declares unsupported transport `{transport}`",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SchemaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SchemaError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Loader for schema YAML files. Stateless — the type exists for
/// namespacing and future-proofing (caching, schema-version negotiation
/// could grow into instance state).
pub struct SchemaLoader;

impl SchemaLoader {
    /// Read and validate one schema file.
    pub fn load_file(path: impl AsRef<Path>) -> Result<Schema, SchemaError> {
        let path = path.as_ref();
        let body = fs::read_to_string(path).map_err(|source| SchemaError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let schema: Schema = serde_yaml::from_str(&body).map_err(|err| SchemaError::Parse {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        validate(&schema, path)?;
        Ok(schema)
    }

    /// Read and validate every `.yaml`/`.yml` file in a directory
    /// (non-recursive). Files are visited in sorted order so the caller
    /// gets a deterministic vector. One bad file fails the whole load
    /// with the offending path in the error.
    pub fn load_dir(path: impl AsRef<Path>) -> Result<Vec<Schema>, SchemaError> {
        let path = path.as_ref();
        let entries = fs::read_dir(path).map_err(|source| SchemaError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        // Per-entry errors (permission denied, transient FS hiccup) are
        // propagated rather than silently filtered: an incomplete schema
        // set is worse than a hard failure with a precise message.
        let mut yaml_paths: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| SchemaError::Io {
                path: path.to_path_buf(),
                source,
            })?;
            let p = entry.path();
            if p.is_file()
                && p.extension().and_then(|s| s.to_str()).is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml")
                })
            {
                yaml_paths.push(p);
            }
        }
        yaml_paths.sort();

        let mut schemas = Vec::with_capacity(yaml_paths.len());
        for p in yaml_paths {
            schemas.push(Self::load_file(&p)?);
        }
        Ok(schemas)
    }
}

/// Schema-format versions this loader recognises. Currently only `1`.
/// New versions land with explicit migration paths, not a permissive
/// accept.
const SUPPORTED_SCHEMA_VERSIONS: &[u32] = &[1];

/// Allowed values of `attaches_to`. Closed set per *Extending Lex* §13.2.
const ALLOWED_NODE_KINDS: &[&str] = &[
    "paragraph",
    "definition",
    "session",
    "annotation",
    "list_item",
    "verbatim",
];

fn validate(schema: &Schema, path: &Path) -> Result<(), SchemaError> {
    // schema_version: only the recognised versions are accepted.
    if !SUPPORTED_SCHEMA_VERSIONS.contains(&schema.schema_version) {
        return Err(SchemaError::UnsupportedSchemaVersion {
            path: path.to_path_buf(),
            label: schema.label.clone(),
            version: schema.schema_version,
        });
    }

    // Params: enum-typed values are non-empty, individual names are
    // non-empty, and all names are unique within the param.
    for (name, spec) in &schema.params {
        if spec.ty == ParamType::Enum {
            if spec.values.is_empty() {
                return Err(SchemaError::EmptyEnumValues {
                    path: path.to_path_buf(),
                    label: schema.label.clone(),
                    param: name.clone(),
                });
            }
            let mut seen = std::collections::HashSet::with_capacity(spec.values.len());
            for v in &spec.values {
                if v.name.is_empty() {
                    return Err(SchemaError::EmptyEnumValueName {
                        path: path.to_path_buf(),
                        label: schema.label.clone(),
                        param: name.clone(),
                    });
                }
                if !seen.insert(v.name.as_str()) {
                    return Err(SchemaError::DuplicateEnumValue {
                        path: path.to_path_buf(),
                        label: schema.label.clone(),
                        param: name.clone(),
                        value: v.name.clone(),
                    });
                }
            }
        }
    }

    // attaches_to: every entry must be a known node kind.
    for kind in &schema.attaches_to {
        if !ALLOWED_NODE_KINDS.contains(&kind.as_str()) {
            return Err(SchemaError::UnknownNodeKind {
                path: path.to_path_buf(),
                label: schema.label.clone(),
                kind: kind.clone(),
            });
        }
    }

    // verbatim_label: label must be syntactically legal as a verbatim
    // closing token.
    if schema.verbatim_label {
        if let Err(reason) = check_verbatim_label(&schema.label) {
            return Err(SchemaError::InvalidVerbatimLabel {
                path: path.to_path_buf(),
                label: schema.label.clone(),
                reason: reason.into(),
            });
        }
    }

    // handler: transport-specific shape rules.
    if let Some(handler) = &schema.handler {
        match handler.transport {
            HandlerTransport::Wasm => {
                return Err(SchemaError::WasmTransportDeferred {
                    path: path.to_path_buf(),
                    label: schema.label.clone(),
                });
            }
            HandlerTransport::Subprocess => {
                if handler.command.is_empty() {
                    return Err(SchemaError::EmptySubprocessCommand {
                        path: path.to_path_buf(),
                        label: schema.label.clone(),
                    });
                }
            }
            HandlerTransport::Native => {}
            // HandlerTransport is #[non_exhaustive] for forward-compat
            // across lex-extension major versions. Reject unknown
            // variants explicitly: a future variant slipping through
            // serde without being taught to the validator would
            // otherwise be silently accepted, which contradicts the
            // strict-by-default loader contract.
            other => {
                return Err(SchemaError::UnsupportedTransport {
                    path: path.to_path_buf(),
                    label: schema.label.clone(),
                    transport: format!("{other:?}").to_lowercase(),
                });
            }
        }
    }

    Ok(())
}

/// A label is legal as a verbatim closing if it forms a single token in
/// the verbatim header line `:: label ... ::`. We enforce the minimum
/// invariants the lexer relies on:
///
/// - non-empty,
/// - no whitespace (would split it into multiple tokens),
/// - no `::` substring (collides with the closing marker).
fn check_verbatim_label(label: &str) -> Result<(), &'static str> {
    if label.is_empty() {
        return Err("label is empty");
    }
    if label.chars().any(char::is_whitespace) {
        return Err("label contains whitespace");
    }
    if label.contains("::") {
        return Err("label contains the verbatim-marker sequence `::`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_extension::schema::{
        BodyKind, BodyShape, EnumValue, HandlerSpec, HookSet, ParamSpec, RenderHook,
    };
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn write_yaml(dir: &TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, body).expect("write fixture");
        path
    }

    const COMMENT_SCHEMA_YAML: &str = r#"
schema_version: 1
label: acme.commenting
description: A comment thread.
params:
  role:
    type: enum
    required: true
    values:
      - name: author
      - name: editor
attaches_to: [paragraph, session]
body:
  kind: lex
  presence: required
verbatim_label: false
hooks:
  validate: true
  hover: true
  render: [html, markdown]
handler:
  transport: subprocess
  command: [acme-comment-handler]
  timeout_ms: 2000
"#;

    #[test]
    fn loads_valid_schema_with_all_features() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(&dir, "comment.yaml", COMMENT_SCHEMA_YAML);

        let schema = SchemaLoader::load_file(&path).expect("loads cleanly");
        assert_eq!(schema.label, "acme.commenting");
        assert_eq!(schema.attaches_to, vec!["paragraph", "session"]);
        assert_eq!(schema.body.kind, BodyKind::Lex);
        assert!(schema.hooks.validate);
        assert_eq!(
            schema.hooks.render,
            vec![RenderHook::new("html"), RenderHook::new("markdown")]
        );
        let handler = schema.handler.as_ref().expect("handler present");
        assert_eq!(handler.transport, HandlerTransport::Subprocess);
        assert_eq!(handler.command, vec!["acme-comment-handler".to_string()]);
        assert_eq!(handler.timeout_ms, Some(2000));
    }

    #[test]
    fn loads_minimal_schema_with_defaults() {
        // Only the required fields. Body defaults to {kind: none,
        // presence: optional}; hooks default to all-off; capabilities
        // default to {fs: false, net: false}; no handler.
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "min.yaml",
            r#"
schema_version: 1
label: ns.bare
"#,
        );
        let schema = SchemaLoader::load_file(&path).expect("loads cleanly");
        assert_eq!(schema.label, "ns.bare");
        assert!(schema.params.is_empty());
        assert!(schema.attaches_to.is_empty());
        assert_eq!(schema.body.kind, BodyKind::None);
        assert!(!schema.verbatim_label);
        assert!(schema.handler.is_none());
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "unknown.yaml",
            r#"
schema_version: 1
label: ns.x
mystery_field: 42
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        assert!(matches!(err, SchemaError::Parse { .. }));
        assert!(
            err.to_string().contains("mystery_field"),
            "error must name the offending field, got: {err}"
        );
    }

    #[test]
    fn unknown_field_inside_handler_is_rejected() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "unknown_nested.yaml",
            r#"
schema_version: 1
label: ns.x
handler:
  transport: subprocess
  command: [run]
  bogus: true
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        assert!(matches!(err, SchemaError::Parse { .. }));
        assert!(err.to_string().contains("bogus"));
    }

    #[test]
    fn missing_required_field_is_a_parse_error() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(&dir, "no_label.yaml", "schema_version: 1\n");
        let err = SchemaLoader::load_file(&path).unwrap_err();
        assert!(matches!(err, SchemaError::Parse { .. }));
        assert!(err.to_string().contains("label"));
    }

    #[test]
    fn unknown_param_type_is_a_parse_error() {
        // ParamType is a closed enum; serde rejects unknown variants.
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "weird_type.yaml",
            r#"
schema_version: 1
label: ns.x
params:
  count:
    type: integer
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        assert!(matches!(err, SchemaError::Parse { .. }));
    }

    #[test]
    fn unknown_node_kind_in_attaches_to() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "bad_attach.yaml",
            r#"
schema_version: 1
label: ns.x
attaches_to: [paragraph, fragment]
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        match err {
            SchemaError::UnknownNodeKind { kind, label, .. } => {
                assert_eq!(kind, "fragment");
                assert_eq!(label, "ns.x");
            }
            other => panic!("expected UnknownNodeKind, got: {other}"),
        }
    }

    #[test]
    fn enum_values_must_be_non_empty() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "empty_enum.yaml",
            r#"
schema_version: 1
label: ns.x
params:
  role:
    type: enum
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        match err {
            SchemaError::EmptyEnumValues { param, label, .. } => {
                assert_eq!(param, "role");
                assert_eq!(label, "ns.x");
            }
            other => panic!("expected EmptyEnumValues, got: {other}"),
        }
    }

    #[test]
    fn empty_enum_value_name_rejected() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "empty_name.yaml",
            r#"
schema_version: 1
label: ns.x
params:
  role:
    type: enum
    values:
      - name: ""
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        match err {
            SchemaError::EmptyEnumValueName { param, label, .. } => {
                assert_eq!(param, "role");
                assert_eq!(label, "ns.x");
            }
            other => panic!("expected EmptyEnumValueName, got: {other}"),
        }
    }

    #[test]
    fn unsupported_schema_version_rejected() {
        // schema_version: 2 deserialises fine but the validator
        // refuses it because this loader only recognises version 1.
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "v2.yaml",
            r#"
schema_version: 2
label: ns.x
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        match err {
            SchemaError::UnsupportedSchemaVersion { version, label, .. } => {
                assert_eq!(version, 2);
                assert_eq!(label, "ns.x");
            }
            other => panic!("expected UnsupportedSchemaVersion, got: {other}"),
        }
    }

    #[test]
    fn duplicate_enum_values_rejected() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "dup_enum.yaml",
            r#"
schema_version: 1
label: ns.x
params:
  role:
    type: enum
    values:
      - name: author
      - name: author
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        match err {
            SchemaError::DuplicateEnumValue { value, .. } => {
                assert_eq!(value, "author");
            }
            other => panic!("expected DuplicateEnumValue, got: {other}"),
        }
    }

    #[test]
    fn verbatim_label_with_whitespace_is_invalid() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "ws.yaml",
            r#"
schema_version: 1
label: "ns.has space"
verbatim_label: true
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        match err {
            SchemaError::InvalidVerbatimLabel { reason, .. } => {
                assert!(reason.contains("whitespace"), "got: {reason}");
            }
            other => panic!("expected InvalidVerbatimLabel, got: {other}"),
        }
    }

    #[test]
    fn verbatim_label_with_double_colon_is_invalid() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "colon.yaml",
            r#"
schema_version: 1
label: "ns::bad"
verbatim_label: true
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        assert!(matches!(err, SchemaError::InvalidVerbatimLabel { .. }));
    }

    #[test]
    fn verbatim_label_false_does_not_validate_label_shape() {
        // Whitespace in the label is silly but only fatal when
        // verbatim_label: true claims to use the label as a verbatim
        // closing.
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "ws_off.yaml",
            r#"
schema_version: 1
label: "ns.has space"
verbatim_label: false
"#,
        );
        SchemaLoader::load_file(&path).expect("verbatim_label: false skips the legality check");
    }

    #[test]
    fn wasm_transport_is_deferred_with_clear_error() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "wasm.yaml",
            r#"
schema_version: 1
label: ns.x
handler:
  transport: wasm
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        assert!(matches!(err, SchemaError::WasmTransportDeferred { .. }));
        assert!(err.to_string().contains("deferred"));
    }

    #[test]
    fn subprocess_transport_requires_non_empty_command() {
        let dir = TempDir::new().unwrap();
        let path = write_yaml(
            &dir,
            "sub.yaml",
            r#"
schema_version: 1
label: ns.x
handler:
  transport: subprocess
  command: []
"#,
        );
        let err = SchemaLoader::load_file(&path).unwrap_err();
        assert!(matches!(err, SchemaError::EmptySubprocessCommand { .. }));
    }

    #[test]
    fn missing_file_yields_io_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("does-not-exist.yaml");
        let err = SchemaLoader::load_file(&path).unwrap_err();
        assert!(matches!(err, SchemaError::Io { .. }));
    }

    #[test]
    fn load_dir_collects_all_yaml_files() {
        let dir = TempDir::new().unwrap();
        write_yaml(&dir, "a.yaml", COMMENT_SCHEMA_YAML);
        write_yaml(
            &dir,
            "b.yml",
            r#"
schema_version: 1
label: ns.y
"#,
        );
        // Non-yaml file is ignored.
        write_yaml(&dir, "readme.md", "ignore me");

        let schemas = SchemaLoader::load_dir(dir.path()).expect("loads dir");
        assert_eq!(schemas.len(), 2);
        // Sorted by file name → a.yaml before b.yml.
        assert_eq!(schemas[0].label, "acme.commenting");
        assert_eq!(schemas[1].label, "ns.y");
    }

    #[test]
    fn load_dir_fails_with_offending_path_on_one_bad_file() {
        let dir = TempDir::new().unwrap();
        write_yaml(&dir, "ok.yaml", COMMENT_SCHEMA_YAML);
        let bad_path = write_yaml(
            &dir,
            "broken.yaml",
            "schema_version: 1\nlabel: ns.x\nattaches_to: [bogus]\n",
        );

        let err = SchemaLoader::load_dir(dir.path()).unwrap_err();
        match &err {
            SchemaError::UnknownNodeKind { path, .. } => {
                assert_eq!(path, &bad_path, "error must name the offending file");
            }
            other => panic!("expected UnknownNodeKind from the bad file, got: {other}"),
        }
        // And the error formats with that file path.
        assert!(err.to_string().contains("broken.yaml"));
    }

    #[test]
    fn load_dir_on_missing_directory_yields_io_error() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");
        let err = SchemaLoader::load_dir(&missing).unwrap_err();
        assert!(matches!(err, SchemaError::Io { .. }));
    }

    /// Round-trip: a hand-built `Schema` serialises to YAML and reloads
    /// through `load_file` to an equal value. This is the property test
    /// the issue asks for, in concrete form.
    #[test]
    fn round_trip_schema_through_yaml_is_identity() {
        let mut params = BTreeMap::new();
        params.insert(
            "limit".into(),
            ParamSpec {
                ty: ParamType::Int,
                required: false,
                default: Some(serde_json::json!(10)),
                description: Some("Max items".into()),
                pattern: None,
                values: Vec::new(),
            },
        );
        params.insert(
            "kind".into(),
            ParamSpec {
                ty: ParamType::Enum,
                required: true,
                default: None,
                description: None,
                pattern: None,
                values: vec![
                    EnumValue {
                        name: "small".into(),
                        description: None,
                    },
                    EnumValue {
                        name: "large".into(),
                        description: Some("the big one".into()),
                    },
                ],
            },
        );
        let original = Schema {
            schema_version: 1,
            label: "demo.thing".into(),
            description: Some("Demo schema".into()),
            params,
            attaches_to: vec!["paragraph".into(), "annotation".into()],
            body: BodyShape {
                kind: BodyKind::Text,
                presence: lex_extension::schema::BodyPresence::Required,
                description: None,
            },
            verbatim_label: false,
            capabilities: lex_extension::schema::Capabilities {
                fs: false,
                net: false,
            },
            hooks: HookSet {
                validate: true,
                render: vec![RenderHook::new("html")],
                ..HookSet::default()
            },
            handler: Some(HandlerSpec {
                transport: HandlerTransport::Native,
                command: Vec::new(),
                timeout_ms: None,
            }),
        };

        let yaml = serde_yaml::to_string(&original).expect("serialises");
        let dir = TempDir::new().unwrap();
        let path = write_yaml(&dir, "rt.yaml", &yaml);
        let reloaded = SchemaLoader::load_file(&path).expect("reloads");
        assert_eq!(reloaded, original);
    }
}
