//! Inline-content wire types.
//!
//! Inlines appear inside paragraphs and list items. The serde derives produce
//! the JSON shapes documented in wire spec §2.3.

use serde::{Deserialize, Serialize};

/// One inline element. Wire form is a tagged object with `"kind"` selecting
/// the variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireInline {
    /// Plain text.
    Text { text: String },
    /// `*bold*` content.
    Bold { children: Vec<WireInline> },
    /// `_italic_` content.
    Italic { children: Vec<WireInline> },
    /// `` `code` `` content. Literal — no nested inlines.
    Code { text: String },
    /// `#math#` content. Literal — no nested inlines.
    Math { text: String },
    /// `[reference]` content. The kind sub-discriminator (`url`, `citation`,
    /// `footnote`, …) selects the semantic meaning of `target`.
    Reference {
        ref_kind: RefKind,
        target: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
}

/// Sub-kind of an inline `reference`.
///
/// Forward compatibility: handlers must treat unknown values as
/// [`RefKind::General`] (per the wire spec's "handlers must treat unknown
/// `ref_kind` values as `general`" rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Url,
    Citation,
    Footnote,
    Session,
    File,
    Placeholder,
    Unsure,
    General,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_inline_round_trips() {
        let i = WireInline::Text {
            text: "hello".into(),
        };
        let s = serde_json::to_string(&i).unwrap();
        assert_eq!(s, r#"{"kind":"text","text":"hello"}"#);
        let back: WireInline = serde_json::from_str(&s).unwrap();
        assert_eq!(back, i);
    }

    #[test]
    fn bold_with_nested_text() {
        let i = WireInline::Bold {
            children: vec![WireInline::Text {
                text: "loud".into(),
            }],
        };
        let s = serde_json::to_string(&i).unwrap();
        assert_eq!(
            s,
            r#"{"kind":"bold","children":[{"kind":"text","text":"loud"}]}"#
        );
    }

    #[test]
    fn reference_url_round_trips() {
        let i = WireInline::Reference {
            ref_kind: RefKind::Url,
            target: "https://example.com".into(),
            label: None,
        };
        let s = serde_json::to_string(&i).unwrap();
        assert_eq!(
            s,
            r#"{"kind":"reference","ref_kind":"url","target":"https://example.com"}"#
        );
        let back: WireInline = serde_json::from_str(&s).unwrap();
        assert_eq!(back, i);
    }

    #[test]
    fn reference_with_label() {
        let i = WireInline::Reference {
            ref_kind: RefKind::Footnote,
            target: "1".into(),
            label: Some("note one".into()),
        };
        let s = serde_json::to_string(&i).unwrap();
        let back: WireInline = serde_json::from_str(&s).unwrap();
        assert_eq!(back, i);
    }
}
