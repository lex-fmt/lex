//! Wire types for the `on_format` reverse hook (#570 Phase 4).
//!
//! See the spec at
//! `comms/specs/proposals/lex-extension-wire.lex` §4.8 (`on_format`)
//! for the full contract. The hook is the inverse of `on_resolve`:
//! given a typed AST subtree previously produced by `on_resolve`, the
//! handler returns the Lex-source representation as a [`LexAnnotationOut`].
//!
//! Two types live here:
//!
//! - [`FormatCtx`] — the request payload. Mirrors [`LabelCtx`] but
//!   carries the *typed* `WireNode` the handler must serialize back,
//!   along with the originating label/params so a namespace with
//!   several labels driving the same node kind can route on the label.
//! - [`LexAnnotationOut`] — the structured response. Describes the
//!   label, parameters, body, and verbatim-flag the host needs to
//!   emit Lex source. Returning `None` (i.e. result `{ "annotation":
//!   null }`) lets the host fall back to its built-in formatter.
//!
//! [`LabelCtx`]: super::LabelCtx

use serde::{Deserialize, Serialize};

use super::ast::WireNode;

/// Request payload for [`LexHandler::on_format`](crate::handler::LexHandler::on_format).
///
/// The handler receives the originating `label` and `params` (lifted
/// from the AST node that the prior `on_resolve` pass produced this
/// typed `node` from), the typed `WireNode` to serialize, and an
/// optional `format_options` object whose shape is namespace-defined.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormatCtx {
    /// Fully-qualified label of the schema that owns this format pass,
    /// e.g. `"lex.tabular.table"`.
    pub label: String,
    /// Originating parameters, in the (key, value) order the host
    /// deserialized them. Quoting and escaping decisions are left to
    /// the host on emission.
    pub params: Vec<(String, String)>,
    /// The typed wire subtree to serialize back as Lex source.
    pub node: WireNode,
    /// Optional, namespace-defined options object. Hosts pass `None`
    /// when no options are configured; the wire form is
    /// `"format_options": null` (omitted from the JSON when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_options: Option<serde_json::Value>,
}

/// Response payload from [`LexHandler::on_format`](crate::handler::LexHandler::on_format).
///
/// Returned wrapped in `Option`: `Some(LexAnnotationOut)` carries the
/// structured serialization; `None` lets the host fall back to its
/// built-in formatter for the underlying node kind.
///
/// `verbatim_label: true` selects the verbatim closing form
/// (subject-line content + `:: label ::` closer); `false` selects the
/// inline annotation form (`:: label :: text` or `:: label ::` plus
/// indented content).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LexAnnotationOut {
    /// Canonical fully-qualified label, e.g. `"lex.tabular.table"`.
    pub label: String,
    /// `(key, value)` pairs emitted in `key=value` order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<(String, String)>,
    /// Verbatim or inline text body. Empty for marker-form annotations.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    /// `true` for verbatim closing form, `false` for inline annotation
    /// form. Defaults to `false` on the wire — omitted entirely from
    /// the serialized JSON when `false` so marker-form annotations get
    /// the compact `{ "label": "..." }` shape.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verbatim_label: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::range::{Position, Range};

    fn r(s_l: u32, s_c: u32, e_l: u32, e_c: u32) -> Range {
        Range::new(Position::new(s_l, s_c), Position::new(e_l, e_c))
    }

    fn sample_node() -> WireNode {
        WireNode::Paragraph {
            range: r(0, 0, 0, 5),
            origin: None,
            inlines: vec![],
        }
    }

    #[test]
    fn format_ctx_round_trips_through_json() {
        let c = FormatCtx {
            label: "lex.tabular.table".into(),
            params: vec![("align".into(), "lcr".into())],
            node: sample_node(),
            format_options: Some(serde_json::json!({ "max_width": 80 })),
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: FormatCtx = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn format_ctx_omits_options_when_none() {
        let c = FormatCtx {
            label: "lex.media.image".into(),
            params: vec![("src".into(), "x.png".into())],
            node: sample_node(),
            format_options: None,
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(
            !s.contains("format_options"),
            "format_options must be omitted when None, got: {s}"
        );
        let back: FormatCtx = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn lex_annotation_out_round_trips_through_json() {
        let a = LexAnnotationOut {
            label: "lex.tabular.table".into(),
            params: vec![("header".into(), "1".into())],
            body: "| a | b |\n|---|---|\n| 1 | 2 |".into(),
            verbatim_label: true,
        };
        let s = serde_json::to_string(&a).unwrap();
        let back: LexAnnotationOut = serde_json::from_str(&s).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn lex_annotation_out_minimal_form_omits_defaults() {
        // Marker-form annotation: empty params, empty body, not verbatim.
        // The serialized form must skip every default field so the
        // wire shape collapses to `{ "label": "..." }`.
        let a = LexAnnotationOut {
            label: "lex.metadata.author".into(),
            params: vec![],
            body: String::new(),
            verbatim_label: false,
        };
        let s = serde_json::to_string(&a).unwrap();
        assert!(!s.contains("params"), "params must be omitted: {s}");
        assert!(!s.contains("body"), "body must be omitted: {s}");
        assert!(
            !s.contains("verbatim_label"),
            "verbatim_label must be omitted when false: {s}"
        );
        let back: LexAnnotationOut = serde_json::from_str(&s).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn lex_annotation_out_deserializes_with_omitted_defaults() {
        // A handler that returns only `{ "label": "..." }` must be
        // accepted on the wire as a marker-form annotation.
        let json = r#"{"label":"lex.metadata.date"}"#;
        let a: LexAnnotationOut = serde_json::from_str(json).unwrap();
        assert_eq!(a.label, "lex.metadata.date");
        assert!(a.params.is_empty());
        assert!(a.body.is_empty());
        assert!(!a.verbatim_label);
    }
}
