//! Wire AST: the cross-version representation of Lex content.
//!
//! [`WireNode`] is a tagged enum (kind discriminator) covering all block
//! kinds. Inline content uses [`WireInline`](crate::wire::WireInline).
//!
//! # Forward compatibility
//!
//! Adding a new `kind` to the wire format is a *breaking* change: the
//! `wire_version` integer bumps, and old hosts/handlers reject the
//! mismatched protocol at the `initialize` handshake. Within a single
//! `wire_version`, the set of node kinds is closed.
//!
//! This is deliberately stricter than the per-string-enum forward-compat
//! policy used for [`DiagnosticSeverity`](crate::wire::DiagnosticSeverity)
//! and similar (which fall back to a documented default on unknown
//! values). Block AST is structural; treating an unknown `kind` as
//! "ignore me" silently drops document content, which is worse than
//! refusing the document with a clear protocol-version diagnostic.
//!
//! On the Rust side, [`WireNode`] is `#[non_exhaustive]` so that adding a
//! new variant in a future major-version release of this crate is not a
//! breaking source-level change for downstream `match` consumers.

use serde::{Deserialize, Serialize};

use super::inline::WireInline;
use super::range::Range;

/// A block-level wire AST node. Wire form is a tagged object with `"kind"`
/// selecting the variant, plus shared `range` and optional `origin` fields.
///
/// See the module-level docs for the forward-compatibility contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum WireNode {
    Document {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        children: Vec<WireNode>,
    },
    Session {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        marker: Option<String>,
        children: Vec<WireNode>,
    },
    Definition {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        subject: String,
        children: Vec<WireNode>,
    },
    Paragraph {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        inlines: Vec<WireInline>,
    },
    List {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        marker_style: String,
        items: Vec<WireListItem>,
    },
    Verbatim {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        label: String,
        params: serde_json::Value,
        body_text: String,
        /// The verbatim block's subject (the lead-in line, e.g.
        /// `Code:`). Empty string for verbatim blocks that have no
        /// subject (or for the placeholder shape used to flag
        /// unsupported variants).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        subject: String,
        /// Rendering mode: `"inflow"` (content indented relative to
        /// subject) or `"fullwidth"` (content at column 2). Defaults
        /// to `"inflow"` on deserialise — matching the parser's
        /// default mode — when the field is absent from the wire
        /// payload.
        #[serde(default = "default_verbatim_mode")]
        mode: String,
    },
    Table {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        caption: String,
        header_rows: u32,
        /// Per-column alignment. One entry per column; values are
        /// `"left"`, `"center"`, `"right"`, or `""` (no alignment).
        /// `column_aligns.length` defines the table's column count
        /// and MUST equal the longest row in `rows`; rows MUST NOT
        /// exceed this length. See `lex-extension-wire.lex` §2.2.
        ///
        /// Replaces the single whole-table `align: String` from
        /// `wire_version: 1`. The old shape collapsed mixed-alignment
        /// tables (e.g. a markdown pipe-table with `| :--- | :---: |`)
        /// to a single alignment on the reverse codec.
        column_aligns: Vec<String>,
        rows: Vec<WireRow>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        footnotes: Vec<WireFootnote>,
    },
    /// Image media node. Produced by `on_resolve` for
    /// `lex.media.image`-class verbatim labels; carries the same
    /// data the host would otherwise flatten into `verbatim.params`.
    /// New in `wire_version: 2` — see `lex-extension-wire.lex` §2.2.
    Image {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        src: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        alt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// Video media node. New in `wire_version: 2`.
    Video {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        src: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        poster: Option<String>,
    },
    /// Audio media node. New in `wire_version: 2`.
    Audio {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        src: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    Annotation {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        label: String,
        params: serde_json::Value,
        /// `null` for marker-form annotations, a string for opaque-text
        /// bodies, an object `{ "kind": "block", "children": [...] }` for
        /// parsed-Lex bodies. See [`AnnotationBody`](super::ctx::AnnotationBody)
        /// for the corresponding [`LabelCtx`](super::ctx::LabelCtx) shape.
        body: serde_json::Value,
    },
    Blank {
        range: Range,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
    },
}

impl WireNode {
    /// The byte range this node spans in its origin source. Useful for
    /// diagnostic attribution when the host only has the wire node
    /// (e.g., the `on_format` dispatch path doesn't carry a separate
    /// `NodeRef`).
    pub fn range(&self) -> Range {
        match self {
            Self::Document { range, .. }
            | Self::Session { range, .. }
            | Self::Definition { range, .. }
            | Self::Paragraph { range, .. }
            | Self::List { range, .. }
            | Self::Verbatim { range, .. }
            | Self::Table { range, .. }
            | Self::Image { range, .. }
            | Self::Video { range, .. }
            | Self::Audio { range, .. }
            | Self::Annotation { range, .. }
            | Self::Blank { range, .. } => *range,
        }
    }
}

/// One item inside a [`WireNode::List`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireListItem {
    pub range: Range,
    pub inlines: Vec<WireInline>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<WireNode>,
}

/// One row in a [`WireNode::Table`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireRow {
    pub cells: Vec<WireTableCell>,
}

/// One cell in a [`WireRow`]. `inlines` holds the cell's content; merge
/// markers (`>>`, `^^`) are surfaced as `colspan` / `rowspan` for downstream
/// renderers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireTableCell {
    pub inlines: Vec<WireInline>,
    #[serde(default = "one")]
    pub colspan: u32,
    #[serde(default = "one")]
    pub rowspan: u32,
}

fn one() -> u32 {
    1
}

fn default_verbatim_mode() -> String {
    "inflow".to_string()
}

/// One footnote attached to a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireFootnote {
    pub marker: String,
    pub inlines: Vec<WireInline>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::range::Position;

    fn r(s_l: u32, s_c: u32, e_l: u32, e_c: u32) -> Range {
        Range::new(Position::new(s_l, s_c), Position::new(e_l, e_c))
    }

    #[test]
    fn paragraph_round_trips() {
        let p = WireNode::Paragraph {
            range: r(0, 0, 0, 5),
            origin: None,
            inlines: vec![WireInline::Text {
                text: "hello".into(),
            }],
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: WireNode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn paragraph_kind_in_serialized_form() {
        let p = WireNode::Paragraph {
            range: r(0, 0, 0, 5),
            origin: None,
            inlines: vec![],
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains(r#""kind":"paragraph""#));
    }

    #[test]
    fn document_with_children() {
        let d = WireNode::Document {
            range: r(0, 0, 10, 0),
            origin: Some("doc.lex".into()),
            children: vec![WireNode::Paragraph {
                range: r(0, 0, 0, 3),
                origin: None,
                inlines: vec![WireInline::Text { text: "x".into() }],
            }],
        };
        let s = serde_json::to_string(&d).unwrap();
        assert!(s.contains(r#""origin":"doc.lex""#));
        let back: WireNode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn annotation_with_lex_body() {
        let a = WireNode::Annotation {
            range: r(3, 0, 6, 0),
            origin: None,
            label: "acme.commenting".into(),
            params: serde_json::json!({"role": "editor"}),
            body: serde_json::json!({
                "kind": "block",
                "children": []
            }),
        };
        let s = serde_json::to_string(&a).unwrap();
        let back: WireNode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn verbatim_carries_label_and_body_text() {
        let v = WireNode::Verbatim {
            range: r(0, 0, 4, 0),
            origin: None,
            label: "rust".into(),
            params: serde_json::json!({}),
            body_text: "fn main() {}".into(),
            subject: "Code:".into(),
            mode: "inflow".into(),
        };
        let s = serde_json::to_string(&v).unwrap();
        let back: WireNode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn verbatim_mode_field_defaults_to_inflow_on_deserialise() {
        // A wire payload missing `mode` should round-trip as
        // "inflow" — the documented default for older producers
        // that don't emit the field.
        let payload = r#"{
            "kind":"verbatim",
            "range":{"start":[0,0],"end":[4,0]},
            "label":"rust",
            "params":{},
            "body_text":"x"
        }"#;
        let v: WireNode = serde_json::from_str(payload).unwrap();
        match v {
            WireNode::Verbatim {
                ref mode,
                ref subject,
                ..
            } => {
                assert_eq!(mode, "inflow");
                assert_eq!(subject, "");
            }
            _ => panic!("expected Verbatim"),
        }
    }

    #[test]
    fn table_round_trips_with_per_column_aligns() {
        // Wire_version 2 (#583): `column_aligns` carries per-column
        // alignment as a `Vec<String>` instead of the single-string
        // whole-table summary of v1. Test the round-trip preserves
        // mixed alignments — that was the regression that motivated
        // the bump.
        let t = WireNode::Table {
            range: r(0, 0, 3, 0),
            origin: None,
            caption: "Demo".into(),
            header_rows: 1,
            column_aligns: vec!["left".into(), "center".into(), "right".into()],
            rows: vec![
                WireRow {
                    cells: vec![
                        WireTableCell {
                            inlines: vec![WireInline::Text { text: "h1".into() }],
                            colspan: 1,
                            rowspan: 1,
                        },
                        WireTableCell {
                            inlines: vec![WireInline::Text { text: "h2".into() }],
                            colspan: 1,
                            rowspan: 1,
                        },
                        WireTableCell {
                            inlines: vec![WireInline::Text { text: "h3".into() }],
                            colspan: 1,
                            rowspan: 1,
                        },
                    ],
                },
                WireRow {
                    cells: vec![
                        WireTableCell {
                            inlines: vec![WireInline::Text { text: "c1".into() }],
                            colspan: 1,
                            rowspan: 1,
                        },
                        WireTableCell {
                            inlines: vec![WireInline::Text { text: "c2".into() }],
                            colspan: 1,
                            rowspan: 1,
                        },
                        WireTableCell {
                            inlines: vec![WireInline::Text { text: "c3".into() }],
                            colspan: 1,
                            rowspan: 1,
                        },
                    ],
                },
            ],
            footnotes: vec![],
        };
        let s = serde_json::to_string(&t).unwrap();
        assert!(
            s.contains(r#""column_aligns":["left","center","right"]"#),
            "column_aligns must serialize as an array of per-column strings, got: {s}"
        );
        let back: WireNode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn image_round_trips() {
        let i = WireNode::Image {
            range: r(2, 0, 2, 30),
            origin: None,
            src: "chart.png".into(),
            alt: "Q4 chart".into(),
            title: Some("Quarter".into()),
        };
        let s = serde_json::to_string(&i).unwrap();
        assert!(s.contains(r#""kind":"image""#));
        assert!(s.contains(r#""src":"chart.png""#));
        let back: WireNode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, i);
    }

    #[test]
    fn image_marker_form_omits_empty_alt() {
        let i = WireNode::Image {
            range: r(0, 0, 0, 0),
            origin: None,
            src: "x.png".into(),
            alt: String::new(),
            title: None,
        };
        let s = serde_json::to_string(&i).unwrap();
        assert!(!s.contains("alt"), "empty alt must be omitted: {s}");
        assert!(!s.contains("title"), "None title must be omitted: {s}");
    }

    #[test]
    fn video_round_trips_with_poster() {
        let v = WireNode::Video {
            range: r(0, 0, 0, 0),
            origin: None,
            src: "demo.mp4".into(),
            title: Some("Demo".into()),
            poster: Some("frame.png".into()),
        };
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains(r#""kind":"video""#));
        assert!(s.contains(r#""poster":"frame.png""#));
        let back: WireNode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn audio_round_trips() {
        let a = WireNode::Audio {
            range: r(0, 0, 0, 0),
            origin: None,
            src: "track.mp3".into(),
            title: None,
        };
        let s = serde_json::to_string(&a).unwrap();
        assert!(s.contains(r#""kind":"audio""#));
        let back: WireNode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn list_with_items() {
        let l = WireNode::List {
            range: r(0, 0, 2, 0),
            origin: None,
            marker_style: "dash".into(),
            items: vec![
                WireListItem {
                    range: r(0, 0, 0, 5),
                    inlines: vec![WireInline::Text { text: "a".into() }],
                    children: vec![],
                },
                WireListItem {
                    range: r(1, 0, 1, 5),
                    inlines: vec![WireInline::Text { text: "b".into() }],
                    children: vec![],
                },
            ],
        };
        let s = serde_json::to_string(&l).unwrap();
        let back: WireNode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, l);
    }
}
