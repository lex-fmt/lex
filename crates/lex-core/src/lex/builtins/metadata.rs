//! Schemas for the `lex.metadata.*` family of document-level labels.
//!
//! These are the registry-shaped replacements for the hardcoded
//! frontmatter-promotion whitelist in
//! [`crate::ir::from_lex`](../../../../lex-babel/src/ir/from_lex.rs)
//! (`author`, `title`, `date`, `tags`, `category`, `template`,
//! `publishing-date`, `front-matter`). Issue
//! [#570](https://github.com/lex-fmt/lex/issues/570) tracks the multi-phase
//! migration.
//!
//! # Phase 1 status
//!
//! Schemas only — no hooks are declared yet. The legacy frontmatter
//! promotion in `lex-babel` continues to own the IR work. The schemas
//! exist as registration targets for the Phase 2 parse-time auto-rewrite
//! (`:: title ::` → `:: lex.metadata.title ::`); Phase 3 retires the
//! legacy IR path; Phase 4 wires `on_render`/`on_format` so these
//! labels emit `<title>` / `<meta>` in HTML.
//!
//! Every schema in this module shares the same shape — only the
//! fully-qualified label string differs — so [`metadata_schema`] does
//! the heavy lifting and each public function is a one-liner.

use lex_extension::schema::{BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, Schema};
use std::collections::BTreeMap;

/// The eight metadata labels, in the order they appear in the legacy
/// whitelist (`crates/lex-babel/src/ir/from_lex.rs`). Exposed so the
/// Phase 2 auto-rewrite can iterate the canonical set without
/// re-declaring it.
pub const METADATA_LABELS: &[&str] = &[
    "lex.metadata.title",
    "lex.metadata.author",
    "lex.metadata.date",
    "lex.metadata.tags",
    "lex.metadata.category",
    "lex.metadata.template",
    "lex.metadata.publishing-date",
    "lex.metadata.front-matter",
];

/// Build a metadata schema. Every `lex.metadata.*` label attaches to a
/// document, carries its value either as the annotation body (text) or
/// as untyped parameters (the legacy whitelist accepted both), and
/// declares no privileged capabilities. Hooks are deliberately empty in
/// Phase 1 — they fill in across Phases 3 + 4.
fn metadata_schema(label: &'static str, description: &'static str) -> Schema {
    Schema {
        schema_version: 1,
        label: label.into(),
        description: Some(description.into()),
        params: BTreeMap::new(),
        attaches_to: vec!["document".into()],
        body: BodyShape {
            kind: BodyKind::Text,
            presence: BodyPresence::Optional,
            description: Some(
                "Annotation body (single-line text) carries the metadata value when no \
                 explicit parameter is supplied."
                    .into(),
            ),
        },
        verbatim_label: false,
        capabilities: Capabilities::default(),
        hooks: HookSet::default(),
        handler: None,
    }
}

pub fn lex_metadata_title_schema() -> Schema {
    metadata_schema(
        "lex.metadata.title",
        "Document title. Renders as `<title>` and `<meta name=\"title\">` in HTML output.",
    )
}

pub fn lex_metadata_author_schema() -> Schema {
    metadata_schema(
        "lex.metadata.author",
        "Document author. Renders as `<meta name=\"author\">` in HTML output.",
    )
}

pub fn lex_metadata_date_schema() -> Schema {
    metadata_schema(
        "lex.metadata.date",
        "Document date. Renders as `<meta name=\"date\">` in HTML output.",
    )
}

pub fn lex_metadata_tags_schema() -> Schema {
    metadata_schema(
        "lex.metadata.tags",
        "Document tags (comma-separated). Renders as `<meta name=\"keywords\">` in HTML output.",
    )
}

pub fn lex_metadata_category_schema() -> Schema {
    metadata_schema(
        "lex.metadata.category",
        "Document category. Renders as `<meta name=\"category\">` in HTML output.",
    )
}

pub fn lex_metadata_template_schema() -> Schema {
    metadata_schema(
        "lex.metadata.template",
        "Template hint for renderers that select a layout per document.",
    )
}

pub fn lex_metadata_publishing_date_schema() -> Schema {
    metadata_schema(
        "lex.metadata.publishing-date",
        "Publishing date (distinct from authoring `date`). Renders as \
         `<meta name=\"publishing-date\">` in HTML output.",
    )
}

pub fn lex_metadata_front_matter_schema() -> Schema {
    metadata_schema(
        "lex.metadata.front-matter",
        "Catch-all front-matter container for renderer-specific extensions.",
    )
}

/// All `lex.metadata.*` schemas, in declaration order.
pub fn all_schemas() -> Vec<Schema> {
    vec![
        lex_metadata_title_schema(),
        lex_metadata_author_schema(),
        lex_metadata_date_schema(),
        lex_metadata_tags_schema(),
        lex_metadata_category_schema(),
        lex_metadata_template_schema(),
        lex_metadata_publishing_date_schema(),
        lex_metadata_front_matter_schema(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_labels_match_schema_outputs() {
        let labels: Vec<String> = all_schemas().into_iter().map(|s| s.label).collect();
        let expected: Vec<String> = METADATA_LABELS.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(
            labels, expected,
            "all_schemas() ordering must mirror METADATA_LABELS so the Phase 2 \
             auto-rewrite has a single source of truth for the label set"
        );
    }

    #[test]
    fn every_metadata_schema_attaches_to_document() {
        for schema in all_schemas() {
            assert_eq!(
                schema.attaches_to,
                vec!["document".to_string()],
                "{} must declare document-scope attachment",
                schema.label
            );
            assert!(
                !schema.verbatim_label,
                "{} is an annotation label, not a verbatim label",
                schema.label
            );
        }
    }

    #[test]
    fn metadata_schemas_declare_no_hooks_in_phase_1() {
        // Phase 1's contract: schemas register but don't intercept. Hook
        // activation comes in Phases 3 + 4. If a hook flag flips on
        // unexpectedly, that's a signal a later-phase change leaked
        // back into the metadata family.
        for schema in all_schemas() {
            assert!(
                !schema.hooks.resolve,
                "{} resolve hook must stay off in Phase 1",
                schema.label
            );
            assert!(
                !schema.hooks.validate,
                "{} validate hook must stay off in Phase 1",
                schema.label
            );
            assert!(
                schema.hooks.render.is_empty(),
                "{} render hook must stay off in Phase 1",
                schema.label
            );
        }
    }

    #[test]
    fn metadata_schemas_round_trip_through_json() {
        // Schema is serde-derived; making sure each label survives the
        // round trip guards against accidental non-serializable
        // additions to the shape (`Capabilities`, `HookSet`, etc.).
        for schema in all_schemas() {
            let json = serde_json::to_string(&schema).expect("serialize");
            let back: Schema = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, schema, "round trip for {}", schema.label);
        }
    }
}
