//! Wire-format names for AST node kinds that can host a label
//! invocation (annotation header or verbatim block closing).
//!
//! This is the canonical source for two consumers that previously
//! kept their own copies of the list:
//!
//! - The schema loader (`lex-extension-host`): every entry in a
//!   schema's `attaches_to` list must parse as one of these names.
//! - The analysis / render walkers (`lex-analysis::label_dispatch`,
//!   `lex-babel::render_dispatch`): each labelled node is reported
//!   with one of these names as its `attached_to` kind.
//!
//! Keeping the list in one place prevents the two consumers from
//! drifting. A variant present in the walker but missing from the
//! loader's whitelist would cause valid schemas to fail
//! pre-validation; a typo in the walker would let invalid
//! attachments slip through. Both classes of bug were observed in
//! the original PR 4/7/8 implementations and are what motivated
//! this shared type.
//!
//! # Stability
//!
//! Wire string forms are stable within a `WIRE_VERSION`. Adding a
//! new kind is non-breaking on the host side (older schemas/walkers
//! simply don't reference it); removing or renaming a kind is a
//! `WIRE_VERSION` bump. The Rust enum is `#[non_exhaustive]` so new
//! variants don't break exhaustive matches at consumer build time.

use serde::{Deserialize, Serialize};

/// One of the AST node kinds a label can attach to. See module-level
/// docs for the rationale behind centralising this list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HostNodeKind {
    Document,
    Session,
    Definition,
    Paragraph,
    List,
    ListItem,
    Verbatim,
    Table,
    Annotation,
}

impl HostNodeKind {
    /// All variants in declaration order. The schema loader uses
    /// this for "allowed kinds" error messages; tests use it to
    /// exercise every variant.
    pub const ALL: &'static [HostNodeKind] = &[
        HostNodeKind::Document,
        HostNodeKind::Session,
        HostNodeKind::Definition,
        HostNodeKind::Paragraph,
        HostNodeKind::List,
        HostNodeKind::ListItem,
        HostNodeKind::Verbatim,
        HostNodeKind::Table,
        HostNodeKind::Annotation,
    ];

    /// Canonical wire string. Stable within `WIRE_VERSION = 1`.
    pub const fn as_str(self) -> &'static str {
        match self {
            HostNodeKind::Document => "document",
            HostNodeKind::Session => "session",
            HostNodeKind::Definition => "definition",
            HostNodeKind::Paragraph => "paragraph",
            HostNodeKind::List => "list",
            HostNodeKind::ListItem => "list_item",
            HostNodeKind::Verbatim => "verbatim",
            HostNodeKind::Table => "table",
            HostNodeKind::Annotation => "annotation",
        }
    }

    /// Parse a wire kind name. Returns `None` for unknown names; the
    /// schema loader uses this to reject `attaches_to` entries the
    /// host doesn't understand.
    pub fn parse(s: &str) -> Option<HostNodeKind> {
        Self::ALL.iter().copied().find(|k| k.as_str() == s)
    }

    /// Canonical comma-separated list of allowed names — used in
    /// schema-loader error messages and any other surface that
    /// wants to enumerate the allowed set.
    pub fn allowed_list() -> String {
        Self::ALL
            .iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl std::fmt::Display for HostNodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_through_str() {
        for k in HostNodeKind::ALL {
            assert_eq!(HostNodeKind::parse(k.as_str()), Some(*k));
        }
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert_eq!(HostNodeKind::parse("fragment"), None);
        assert_eq!(HostNodeKind::parse(""), None);
        assert_eq!(HostNodeKind::parse("Document"), None); // case-sensitive
    }

    #[test]
    fn serialises_as_snake_case_string() {
        let k = HostNodeKind::ListItem;
        let s = serde_json::to_string(&k).unwrap();
        assert_eq!(s, r#""list_item""#);
        let back: HostNodeKind = serde_json::from_str(&s).unwrap();
        assert_eq!(back, k);
    }

    #[test]
    fn allowed_list_includes_every_variant() {
        let list = HostNodeKind::allowed_list();
        for k in HostNodeKind::ALL {
            assert!(list.contains(k.as_str()));
        }
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(HostNodeKind::Paragraph.to_string(), "paragraph");
    }
}
