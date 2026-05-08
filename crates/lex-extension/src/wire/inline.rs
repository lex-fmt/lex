//! Inline-content wire types.
//!
//! Inlines appear inside paragraphs and list items. The serde derives produce
//! the JSON shapes documented in wire spec §2.3.
//!
//! Forward compatibility: as with [`WireNode`](super::ast::WireNode), adding
//! a new inline `kind` is a breaking wire-format change (bumps
//! `wire_version`); within a `wire_version` the set is closed. The Rust
//! enum is `#[non_exhaustive]` so a future major release can add a variant
//! without breaking downstream `match` arms.

use serde::{Deserialize, Serialize};

/// One inline element. Wire form is a tagged object with `"kind"` selecting
/// the variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
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
/// Forward compatibility is implemented in the [`Deserialize`] impl: any
/// string that doesn't match a known variant deserialises as
/// [`RefKind::General`], matching the wire spec's "handlers must treat
/// unknown `ref_kind` values as `general`" rule. The `#[non_exhaustive]`
/// attribute makes adding new variants a non-breaking Rust change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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

impl<'de> Deserialize<'de> for RefKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "url" => Self::Url,
            "citation" => Self::Citation,
            "footnote" => Self::Footnote,
            "session" => Self::Session,
            "file" => Self::File,
            "placeholder" => Self::Placeholder,
            "unsure" => Self::Unsure,
            _ => Self::General,
        })
    }
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

    #[test]
    fn unknown_ref_kind_falls_back_to_general() {
        let kind: RefKind = serde_json::from_str(r#""acronym""#).unwrap();
        assert_eq!(kind, RefKind::General);
    }
}
