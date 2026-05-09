//! [`LabelCtx`] and supporting types — the payload every hook event carries.

use serde::{Deserialize, Serialize};

use super::ast::WireNode;
use super::range::Range;

/// The payload every hook receives. Describes a single label invocation:
/// its parameters, body, and position in the source AST.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelCtx {
    /// Fully-qualified label name, e.g. `"acme.commenting"`.
    pub label: String,
    /// Param values, validated against the schema before dispatch. Always an
    /// object; defaults are filled in. (Stored as `serde_json::Value` rather
    /// than a typed map to keep the wire format generic.)
    pub params: serde_json::Value,
    /// Body content. Shape depends on the schema's `body.kind`.
    pub body: AnnotationBody,
    /// The host AST node the label is attached to.
    pub node: NodeRef,
}

/// Annotation body shape carried by [`LabelCtx::body`]. Wire form is
/// untagged: `null`, a JSON string, or `{ "kind": "block", "children": [...] }`.
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationBody {
    /// `body.kind: none` — marker-form annotation, no body.
    None,
    /// `body.kind: text` — opaque body. Verbatim usage always lands here.
    Text(String),
    /// `body.kind: lex` — parsed body. Children are fully-formed wire AST
    /// nodes the handler can walk directly.
    Lex { children: Vec<WireNode> },
}

impl AnnotationBody {
    /// Returns `true` for [`AnnotationBody::None`].
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

impl Serialize for AnnotationBody {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            Self::None => serializer.serialize_none(),
            Self::Text(s) => serializer.serialize_str(s),
            Self::Lex { children } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("kind", "block")?;
                map.serialize_entry("children", children)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for AnnotationBody {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Null => Ok(Self::None),
            serde_json::Value::String(s) => Ok(Self::Text(s)),
            serde_json::Value::Object(map) => {
                let kind = map.get("kind").and_then(|v| v.as_str());
                if kind != Some("block") {
                    return Err(serde::de::Error::custom(
                        "annotation body object must have kind: \"block\"",
                    ));
                }
                let children_value = map
                    .get("children")
                    .cloned()
                    .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
                let children: Vec<WireNode> =
                    serde_json::from_value(children_value).map_err(serde::de::Error::custom)?;
                Ok(Self::Lex { children })
            }
            _ => Err(serde::de::Error::custom(
                "annotation body must be null, a string, or an object",
            )),
        }
    }
}

/// A reference to the AST node a label is attached to. Carries position
/// metadata so handlers know where in the document the invocation sits;
/// the body of the host node is *not* shipped (handlers receive only the
/// label invocation's own body via [`LabelCtx::body`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeRef {
    /// The host node's wire kind, e.g. `"annotation"`, `"verbatim"`.
    pub kind: String,
    pub range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::range::Position;

    fn r(s_l: u32, s_c: u32, e_l: u32, e_c: u32) -> Range {
        Range::new(Position::new(s_l, s_c), Position::new(e_l, e_c))
    }

    fn nref() -> NodeRef {
        NodeRef {
            kind: "annotation".into(),
            range: r(0, 0, 0, 12),
            origin: None,
        }
    }

    #[test]
    fn none_body_serialises_as_null() {
        let c = LabelCtx {
            label: "foo".into(),
            params: serde_json::json!({}),
            body: AnnotationBody::None,
            node: nref(),
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains(r#""body":null"#));
        let back: LabelCtx = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn text_body_serialises_as_string() {
        let c = LabelCtx {
            label: "foo".into(),
            params: serde_json::json!({}),
            body: AnnotationBody::Text("raw".into()),
            node: nref(),
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains(r#""body":"raw""#));
        let back: LabelCtx = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn lex_body_serialises_as_block() {
        let c = LabelCtx {
            label: "foo".into(),
            params: serde_json::json!({"k": 1}),
            body: AnnotationBody::Lex { children: vec![] },
            node: nref(),
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains(r#""body":{"kind":"block","children":[]}"#));
        let back: LabelCtx = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
    }
}
