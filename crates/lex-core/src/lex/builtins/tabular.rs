//! Schema for the `lex.tabular.*` family of verbatim labels.
//!
//! Today the family has a single member: `lex.tabular.table`, the
//! registry-shaped replacement for the legacy `doc.table` handler in
//! `lex-babel` (`crates/lex-babel/src/common/verbatim/table.rs`).
//! Issue [#570](https://github.com/lex-fmt/lex/issues/570) tracks the
//! multi-phase migration.
//!
//! # Phase 1 status
//!
//! Schema only — no `on_resolve` body yet. The legacy `VerbatimRegistry`
//! still parses pipe-table content into a typed `DocNode::Table`.
//! Phase 2's parse-time auto-rewrite retargets `:: doc.table ::` at
//! this label; Phase 3 deletes the legacy lookup and moves the
//! parsing logic into the handler's `on_resolve`.

use lex_extension::schema::{BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, Schema};
use std::collections::BTreeMap;

/// Fully-qualified label for the canonical tabular table.
pub const LEX_TABULAR_TABLE: &str = "lex.tabular.table";

pub fn lex_tabular_table_schema() -> Schema {
    Schema {
        schema_version: 1,
        label: LEX_TABULAR_TABLE.into(),
        description: Some(
            "Pipe-table verbatim. The verbatim body uses markdown-style pipe-table syntax \
             (`| col1 | col2 |\\n|------|------|\\n| ... |`) which the resolve hook \
             parses into a typed table AST node."
                .into(),
        ),
        params: BTreeMap::new(),
        attaches_to: vec!["verbatim".into()],
        body: BodyShape {
            kind: BodyKind::Text,
            presence: BodyPresence::Required,
            description: Some(
                "Pipe-table source: header row, alignment row, then one row per body \
                 line. Empty body is rejected at resolve time."
                    .into(),
            ),
        },
        verbatim_label: true,
        capabilities: Capabilities::default(),
        hooks: HookSet::default(),
        handler: None,
    }
}

/// All `lex.tabular.*` schemas, in declaration order.
pub fn all_schemas() -> Vec<Schema> {
    vec![lex_tabular_table_schema()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabular_table_is_a_verbatim_label() {
        let schema = lex_tabular_table_schema();
        assert_eq!(schema.label, LEX_TABULAR_TABLE);
        assert!(
            schema.verbatim_label,
            "tabular.table must be a verbatim label"
        );
        assert_eq!(schema.attaches_to, vec!["verbatim".to_string()]);
        assert_eq!(schema.body.kind, BodyKind::Text);
        assert_eq!(schema.body.presence, BodyPresence::Required);
    }

    #[test]
    fn tabular_schema_declares_no_hooks_in_phase_1() {
        let schema = lex_tabular_table_schema();
        assert!(!schema.hooks.resolve);
        assert!(!schema.hooks.validate);
        assert!(schema.hooks.render.is_empty());
    }

    #[test]
    fn tabular_schema_round_trips_through_json() {
        let schema = lex_tabular_table_schema();
        let json = serde_json::to_string(&schema).expect("serialize");
        let back: Schema = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, schema);
    }
}
