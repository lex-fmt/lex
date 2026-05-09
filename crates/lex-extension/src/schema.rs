//! Schema types — the read-only structs a YAML loader produces.
//!
//! The loader itself lives in `lex-extension-host` (PR 4); this module
//! defines the types both the loader and consumers (registry, host, editors)
//! share. The types are `serde`-derived so they can also be hand-built in
//! Rust code without a YAML round-trip (useful for embedders).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookSet {
    #[serde(default)]
    pub label: bool,
    #[serde(default)]
    pub validate: bool,
    #[serde(default)]
    pub resolve: bool,
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
        assert!(hs.render.is_empty());
    }

    #[test]
    fn body_shape_default_is_none_optional() {
        let bs = BodyShape::default();
        assert_eq!(bs.kind, BodyKind::None);
        assert_eq!(bs.presence, BodyPresence::Optional);
    }
}
