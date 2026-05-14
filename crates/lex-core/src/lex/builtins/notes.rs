//! Schema for the `lex.notes` annotation label.
//!
//! `lex.notes` is a marker annotation that attaches to a list, signaling
//! that the list's items are footnote definitions. Heavily used inside
//! `lex-analysis` (`utils::collect_footnote_definitions`,
//! `diagnostics::validate_references`, `hover::footnote_content`) and
//! `lex-lsp-core` to identify footnote-definition lists.
//!
//! Blessed as a one-segment shortcut in `comms/specs/general.lex` §4.2
//! — users author `:: notes ::` and the parser resolves to this
//! canonical via `NormalizeLabels`. PR 2 of #584 added the entry.

use lex_extension::schema::{BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, Schema};
use std::collections::BTreeMap;

/// Fully-qualified label for the `notes` marker annotation.
pub const LEX_NOTES: &str = "lex.notes";

pub fn lex_notes_schema() -> Schema {
    Schema {
        schema_version: 1,
        label: LEX_NOTES.into(),
        description: Some(
            "Marker annotation attached to a list whose items define footnotes. Items \
             with numeric markers (1., 2., ...) define numbered footnotes referenced by \
             `[1]`, `[2]`; items with labeled markers (`[^name]:`) define labeled \
             footnotes referenced by `[^name]`. `lex.notes` may attach at the document \
             root (footnotes visible globally) or inside a session (scoped to that \
             session)."
                .into(),
        ),
        params: BTreeMap::new(),
        attaches_to: vec!["list".into()],
        body: BodyShape {
            kind: BodyKind::None,
            presence: BodyPresence::Optional,
            description: Some(
                "Marker annotation; no body. The list it attaches to carries the \
                 footnote definitions."
                    .into(),
            ),
        },
        verbatim_label: false,
        capabilities: Capabilities::default(),
        hooks: HookSet::default(),
        handler: None,
    }
}

/// Single-entry helper so `register_into` can splice notes alongside
/// the other families.
pub fn all_schemas() -> Vec<Schema> {
    vec![lex_notes_schema()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_is_an_annotation_label() {
        let schema = lex_notes_schema();
        assert_eq!(schema.label, LEX_NOTES);
        assert!(
            !schema.verbatim_label,
            "notes is an annotation, not verbatim"
        );
        assert_eq!(schema.attaches_to, vec!["list".to_string()]);
    }

    #[test]
    fn notes_takes_no_body() {
        let schema = lex_notes_schema();
        assert_eq!(schema.body.kind, BodyKind::None);
        // BodyPresence::Optional is as restrictive as the schema enum
        // goes today — semantic enforcement of "no body allowed" lives
        // in the analysis stage's validator.
        assert_eq!(schema.body.presence, BodyPresence::Optional);
    }

    #[test]
    fn notes_declares_no_hooks() {
        let schema = lex_notes_schema();
        assert!(!schema.hooks.resolve);
        assert!(!schema.hooks.validate);
        assert!(schema.hooks.render.is_empty());
    }

    #[test]
    fn notes_schema_round_trips_through_json() {
        let schema = lex_notes_schema();
        let json = serde_json::to_string(&schema).expect("serialize");
        let back: Schema = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, schema);
    }
}
