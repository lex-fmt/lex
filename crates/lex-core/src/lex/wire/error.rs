//! Error types for the wire codec's reverse direction.

/// Errors the reverse codec can surface when converting a `WireNode` back
/// to a lex-core [`crate::lex::ast::ContentItem`].
///
/// Forward conversion (`to_wire_*`) is total — every lex-core AST shape
/// has a defined mapping to a wire form. Reverse conversion is fallible
/// because the wire input may be malformed or carry an unknown variant.
#[derive(Debug, Clone, PartialEq)]
pub enum FromWireError {
    /// The wire node carried an unknown structural placeholder (an
    /// `Unknown` variant added in a future wire version, or a kind the
    /// host's `WIRE_VERSION` does not recognise).
    UnsupportedKind { kind: String },
    /// A required field was missing or had the wrong shape — for
    /// example, a `WireNode::Annotation` whose `params` was not a
    /// JSON object.
    MalformedField { field: &'static str, detail: String },
    /// A nested `serde_json::from_value` conversion failed when
    /// destructuring an opaque field (e.g., `params` blob).
    DeserialisationFailed(String),
}

impl std::fmt::Display for FromWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FromWireError::UnsupportedKind { kind } => {
                write!(f, "wire AST: unsupported kind `{kind}`")
            }
            FromWireError::MalformedField { field, detail } => {
                write!(f, "wire AST: malformed field `{field}`: {detail}")
            }
            FromWireError::DeserialisationFailed(msg) => {
                write!(f, "wire AST: deserialisation failed: {msg}")
            }
        }
    }
}

impl std::error::Error for FromWireError {}
