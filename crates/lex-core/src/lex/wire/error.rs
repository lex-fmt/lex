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

#[cfg(test)]
mod tests {
    //! `FromWireError` is what propagates out of the reverse codec
    //! when the wire input is malformed; its `Display` impl is what
    //! callers (handler dispatch, debug logs, panics in tests) end
    //! up showing humans, so pin the surface text down.
    use super::*;

    #[test]
    fn display_unsupported_kind_includes_kind_string() {
        let err = FromWireError::UnsupportedKind {
            kind: "lex.future.shape".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("unsupported kind"), "got: {msg}");
        assert!(msg.contains("lex.future.shape"), "got: {msg}");
    }

    #[test]
    fn display_malformed_field_includes_field_and_detail() {
        let err = FromWireError::MalformedField {
            field: "params",
            detail: "expected object".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("malformed field"), "got: {msg}");
        assert!(msg.contains("params"), "got: {msg}");
        assert!(msg.contains("expected object"), "got: {msg}");
    }

    #[test]
    fn display_deserialisation_failed_includes_detail() {
        let err = FromWireError::DeserialisationFailed("invalid type: integer `1`".into());
        let msg = err.to_string();
        assert!(msg.contains("deserialisation failed"), "got: {msg}");
        assert!(msg.contains("invalid type"), "got: {msg}");
    }

    /// `FromWireError` participates in `std::error::Error`. Confirm it
    /// can be boxed and re-formatted through that trait — the path
    /// dispatch code uses when bubbling up handler errors.
    #[test]
    fn implements_std_error_and_round_trips_via_dyn() {
        let err: Box<dyn std::error::Error> =
            Box::new(FromWireError::UnsupportedKind { kind: "x".into() });
        assert!(err.to_string().contains("unsupported kind"));
    }

    /// `Clone` and `PartialEq` are part of the public surface — tests
    /// that triage error-bearing return paths rely on them.
    #[test]
    fn clone_and_equality_hold() {
        let a = FromWireError::MalformedField {
            field: "f",
            detail: "d".into(),
        };
        let b = a.clone();
        assert_eq!(a, b);
        let c = FromWireError::MalformedField {
            field: "f",
            detail: "other".into(),
        };
        assert_ne!(a, c);
    }
}
