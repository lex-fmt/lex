//! Schema types — the read-only structs a YAML loader produces.
//!
//! The loader itself lives in `lex-extension-host` (PR 4); this module
//! defines the types both the loader and consumers (registry, host, editors)
//! share. The types are `serde`-derived so they can also be hand-built in
//! Rust code without a YAML round-trip (useful for embedders).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::wire::DiagnosticSeverity;

/// One label's schema. Mirrors the YAML format documented in the *Extending
/// Lex* proposal §13.2.
///
/// Schemas are strict on deserialise: unknown fields are rejected. Forward
/// compatibility lives at the `wire_version` axis, not at the schema-format
/// level — a schema with a field this version doesn't know about is
/// malformed by definition. The schema loader (`lex-extension-host`)
/// surfaces this as a precise `SchemaError`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    /// Schema-format version. Currently `1`.
    pub schema_version: u32,
    /// Fully-qualified label, e.g. `"acme.commenting"`.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Declared parameters, keyed by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamSpec>,
    /// Permitted host node kinds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attaches_to: Vec<String>,
    /// Body shape when the label is used as an annotation.
    #[serde(default)]
    pub body: BodyShape,
    /// Whether the label is also legal as a verbatim block closing.
    #[serde(default)]
    pub verbatim_label: bool,
    /// Declared OS-level capabilities the handler needs. Honoured once
    /// sandboxing is in place; see proposal §8.
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Hooks the label participates in.
    #[serde(default)]
    pub hooks: HookSet,
    /// Optional handler delivery info. Schema-only labels (validation +
    /// editor UX from the schema alone) omit this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler: Option<HandlerSpec>,
    /// Diagnostic codes this label's handler can emit. Declaring them
    /// lets the host schema-validate `[diagnostics.rules]` entries
    /// against the resolved registry — a `<namespace>.<code>` rule
    /// whose `<code>` matches nothing declared here is a dead letter
    /// the host can flag — and lets `config`/editor tooling surface the
    /// available codes with their descriptions and default severity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<DiagnosticDecl>,
}

/// One diagnostic code a namespace's handler can emit, declared in the
/// label's schema.
///
/// The [`code`](Self::code) is the bare leaf (e.g.
/// `task-due-date-missing`) — exactly what a handler stamps on the
/// `code` field of an emitted `Diagnostic`. Combined with the owning
/// namespace it forms the on-the-wire `<namespace>.<code>` key the user
/// writes under `[diagnostics.rules]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticDecl {
    /// Bare leaf code, matching `Diagnostic.code` set by the handler.
    pub code: String,
    /// Human-readable summary, surfaced in config templates and editor
    /// hover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Declared intrinsic severity. This is *declaration metadata* —
    /// surfaced by config-generation and editor tooling so authors and
    /// users see a code's intended level. It is **not** yet read by the
    /// runtime diagnostic pipeline: a handler-emitted diagnostic's own
    /// `Diagnostic.severity` still determines its intrinsic severity,
    /// and `[diagnostics.rules]` overrides apply on top of that.
    /// Defaults to `warning`.
    ///
    /// Parsed strictly (unlike the permissive wire
    /// [`DiagnosticSeverity`] deserializer): an unknown value is a
    /// schema error, consistent with the schema loader's
    /// `deny_unknown_fields` contract, rather than silently degrading to
    /// `info`.
    #[serde(
        default = "default_decl_severity",
        deserialize_with = "deserialize_strict_severity"
    )]
    pub default_severity: DiagnosticSeverity,
}

fn default_decl_severity() -> DiagnosticSeverity {
    DiagnosticSeverity::Warning
}

/// Strict `default_severity` parser: accepts exactly the four known
/// severities and rejects anything else, so a typo (`warn`, `erorr`)
/// fails the schema load instead of deserialising to `info` the way the
/// wire [`DiagnosticSeverity`] deserializer intentionally does for
/// forward-compatible handler payloads.
fn deserialize_strict_severity<'de, D>(deserializer: D) -> Result<DiagnosticSeverity, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{Error, Unexpected};
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "error" => Ok(DiagnosticSeverity::Error),
        "warning" => Ok(DiagnosticSeverity::Warning),
        "info" => Ok(DiagnosticSeverity::Info),
        "hint" => Ok(DiagnosticSeverity::Hint),
        _ => Err(D::Error::invalid_value(
            Unexpected::Str(&s),
            &"one of: error, warning, info, hint",
        )),
    }
}

/// One parameter declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamSpec {
    #[serde(rename = "type")]
    pub ty: ParamType,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Allowed values when `ty == Enum`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<EnumValue>,
}

/// Allowed parameter types.
///
/// Forward compatibility: unlike the wire-format enums, schema loaders
/// *reject* unknown types — schema-format versioning is independent of
/// `wire_version` and a schema with an unknown `type` is malformed by
/// definition. The `#[non_exhaustive]` attribute keeps adding new variants
/// non-breaking on the Rust side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ParamType {
    String,
    Bool,
    Int,
    Float,
    Enum,
}

/// One legal value of an enum-typed parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnumValue {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Body shape for annotation-form usage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyShape {
    #[serde(default = "BodyKind::default_kind")]
    pub kind: BodyKind,
    #[serde(default)]
    pub presence: BodyPresence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Default for BodyShape {
    fn default() -> Self {
        Self {
            kind: BodyKind::None,
            presence: BodyPresence::Optional,
            description: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum BodyKind {
    None,
    Text,
    Lex,
}

impl BodyKind {
    fn default_kind() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum BodyPresence {
    Optional,
    Required,
}

impl Default for BodyPresence {
    fn default() -> Self {
        Self::Optional
    }
}

/// Declared capabilities. The subprocess transport will sandbox the handler
/// to honour these once OS-level enforcement ships (see proposal §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    #[serde(default)]
    pub fs: bool,
    #[serde(default)]
    pub net: bool,
}

impl Capabilities {
    /// True when the handler declares no privileged capabilities — the
    /// "pure handler" classification used by the trust matrix in
    /// proposal §8.
    ///
    /// Implementation note: this is exact equality with
    /// [`Capabilities::default`] rather than an explicit
    /// `!self.fs && !self.net`. As future capability fields are added
    /// (e.g., `exec`, scoped network, …), they default to `false` and
    /// participate in this check automatically — there is no second
    /// place to remember to update.
    pub fn is_pure(&self) -> bool {
        *self == Self::default()
    }
}

/// Hook participation. Each field defaults to "not implemented".
///
/// `resolve` and `ir_build` form the two lifecycle-phase hooks for
/// content-substitution: `resolve` runs during the resolve phase and
/// splices the returned wire node into the host AST (the canonical
/// example is `lex.include`). `ir_build` runs while the host constructs
/// its in-memory IR and produces a typed wire node consumed in IR-build
/// position only — the canonical examples are `lex.tabular.table` and
/// `lex.media.*`. Pair `ir_build` with `render` on the same schema to
/// give one label both an IR shape and per-format serialization through
/// one registration (the unified registry surface for #615).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookSet {
    #[serde(default)]
    pub label: bool,
    #[serde(default)]
    pub validate: bool,
    #[serde(default)]
    pub resolve: bool,
    /// IR-build participation. When `true`, the host invokes
    /// [`LexHandler::on_ir_build`](crate::handler::LexHandler::on_ir_build)
    /// during IR construction (the verbatim/IR-hydration lifecycle).
    /// Distinct from `resolve` (AST-substitution lifecycle) so a schema
    /// can declare exactly the lifecycle phase it participates in.
    #[serde(default)]
    pub ir_build: bool,
    #[serde(default)]
    pub hover: bool,
    #[serde(default)]
    pub completion: bool,
    #[serde(default)]
    pub code_action: bool,
    /// Render hooks declare which target formats they produce. An empty
    /// vector means the label does not participate in rendering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub render: Vec<RenderHook>,
}

/// One render-format the label can produce.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RenderHook(pub String);

impl RenderHook {
    pub fn new(format: impl Into<String>) -> Self {
        Self(format.into())
    }
}

/// Handler delivery info.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandlerSpec {
    pub transport: HandlerTransport,
    /// Argv for the subprocess transport. Variables in the form `${NAME}`
    /// are expanded at spawn time. Ignored for native and WASM transports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    /// Per-request timeout. Defaults to 2000 ms in subprocess hosts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum HandlerTransport {
    Native,
    Subprocess,
    Wasm,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment_schema() -> Schema {
        let mut params = BTreeMap::new();
        params.insert(
            "role".into(),
            ParamSpec {
                ty: ParamType::Enum,
                required: true,
                default: None,
                description: None,
                pattern: None,
                values: vec![
                    EnumValue {
                        name: "author".into(),
                        description: None,
                    },
                    EnumValue {
                        name: "editor".into(),
                        description: None,
                    },
                ],
            },
        );
        Schema {
            schema_version: 1,
            label: "acme.commenting".into(),
            description: Some("A comment thread.".into()),
            params,
            attaches_to: vec!["paragraph".into(), "session".into()],
            body: BodyShape {
                kind: BodyKind::Lex,
                presence: BodyPresence::Required,
                description: None,
            },
            verbatim_label: false,
            capabilities: Capabilities {
                fs: false,
                net: false,
            },
            hooks: HookSet {
                validate: true,
                hover: true,
                render: vec![RenderHook::new("html"), RenderHook::new("markdown")],
                ..HookSet::default()
            },
            handler: Some(HandlerSpec {
                transport: HandlerTransport::Subprocess,
                command: vec!["acme-comment-handler".into()],
                timeout_ms: Some(2000),
            }),
            diagnostics: vec![
                DiagnosticDecl {
                    code: "unresolved-thread".into(),
                    description: Some("A comment thread has no resolution.".into()),
                    default_severity: DiagnosticSeverity::Warning,
                },
                DiagnosticDecl {
                    code: "missing-author".into(),
                    description: None,
                    default_severity: DiagnosticSeverity::Error,
                },
            ],
        }
    }

    #[test]
    fn schema_round_trips_through_json() {
        let s = comment_schema();
        let serialised = serde_json::to_string(&s).unwrap();
        let back: Schema = serde_json::from_str(&serialised).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn capabilities_is_pure_for_zero_fs_zero_net() {
        assert!(Capabilities::default().is_pure());
        assert!(!Capabilities {
            fs: true,
            net: false
        }
        .is_pure());
        assert!(!Capabilities {
            fs: false,
            net: true
        }
        .is_pure());
    }

    #[test]
    fn hookset_default_is_all_off() {
        let hs = HookSet::default();
        assert!(!hs.validate);
        assert!(!hs.resolve);
        assert!(!hs.ir_build);
        assert!(hs.render.is_empty());
    }

    /// `ir_build` is a new field added with #615 (unified registry
    /// surface). Make sure it round-trips through JSON like every other
    /// hook flag, and that the default-omitted form deserialises with
    /// `ir_build = false` (back-compat for existing schemas authored
    /// before the field existed).
    #[test]
    fn hookset_ir_build_round_trips_through_json() {
        let hs = HookSet {
            ir_build: true,
            ..HookSet::default()
        };
        let serialised = serde_json::to_string(&hs).unwrap();
        assert!(
            serialised.contains("\"ir_build\":true"),
            "ir_build must serialise: {serialised}"
        );
        let back: HookSet = serde_json::from_str(&serialised).unwrap();
        assert!(back.ir_build);

        // Older schema JSON without the field deserialises to false —
        // the back-compat contract.
        let legacy = r#"{"label":false,"validate":false,"resolve":false,"hover":false,"completion":false,"code_action":false}"#;
        let parsed: HookSet = serde_json::from_str(legacy).unwrap();
        assert!(
            !parsed.ir_build,
            "legacy JSON must default ir_build to false"
        );
    }

    #[test]
    fn body_shape_default_is_none_optional() {
        let bs = BodyShape::default();
        assert_eq!(bs.kind, BodyKind::None);
        assert_eq!(bs.presence, BodyPresence::Optional);
    }

    #[test]
    fn schema_without_diagnostics_field_loads_empty() {
        // Schemas that don't declare diagnostics still load — the field
        // defaults to an empty vec, not an error.
        let s: Schema =
            serde_json::from_str(r#"{"schema_version": 1, "label": "acme.task"}"#).unwrap();
        assert!(s.diagnostics.is_empty());
    }

    #[test]
    fn diagnostic_decl_default_severity_is_warning() {
        // `default_severity` is optional; omitting it yields `warning`,
        // matching the doc contract.
        let s: Schema = serde_json::from_str(
            r#"{"schema_version": 1, "label": "acme.task",
                "diagnostics": [{"code": "due-date-missing"}]}"#,
        )
        .unwrap();
        assert_eq!(s.diagnostics.len(), 1);
        assert_eq!(s.diagnostics[0].code, "due-date-missing");
        assert_eq!(s.diagnostics[0].description, None);
        assert_eq!(
            s.diagnostics[0].default_severity,
            DiagnosticSeverity::Warning
        );
    }

    #[test]
    fn diagnostic_decl_explicit_severity_parses() {
        let s: Schema = serde_json::from_str(
            r#"{"schema_version": 1, "label": "acme.task",
                "diagnostics": [{"code": "due-date-missing",
                                 "description": "Task lacks a due date.",
                                 "default_severity": "error"}]}"#,
        )
        .unwrap();
        assert_eq!(s.diagnostics[0].default_severity, DiagnosticSeverity::Error);
        assert_eq!(
            s.diagnostics[0].description.as_deref(),
            Some("Task lacks a due date.")
        );
    }

    #[test]
    fn diagnostic_decl_rejects_unknown_field() {
        assert!(serde_json::from_str::<Schema>(
            r#"{"schema_version": 1, "label": "acme.task",
                "diagnostics": [{"code": "due-date-missing", "severty": "warn"}]}"#,
        )
        .is_err());
    }

    #[test]
    fn diagnostic_decl_rejects_unknown_severity_value() {
        // Strict, unlike the permissive wire deserializer: a typo'd
        // severity (`warn` instead of `warning`) is a schema error, not
        // a silent downgrade to `info`.
        for bad in [r#""warn""#, r#""erorr""#, r#""fatal""#] {
            let src = format!(
                r#"{{"schema_version": 1, "label": "acme.task",
                    "diagnostics": [{{"code": "x", "default_severity": {bad}}}]}}"#
            );
            assert!(
                serde_json::from_str::<Schema>(&src).is_err(),
                "expected `{bad}` to be rejected"
            );
        }
    }
}
