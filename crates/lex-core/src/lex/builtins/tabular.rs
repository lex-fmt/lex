//! Schema for the `lex.tabular.*` family of verbatim labels.
//!
//! Today the family has a single member: `lex.tabular.table` — the
//! canonical pipe-table verbatim. The `on_ir_build` body lives in
//! [`crate::lex::builtins::resolve_tabular_table`] and produces a
//! typed [`WireNode::Table`] with per-column alignment
//! (`column_aligns`) preserved losslessly under `wire_version: 2`.
//!
//! Pre-#615 this dispatch lived on `on_resolve`; the unified registry
//! surface (#615) moved it to the IR-build lifecycle hook so the
//! "verbatim hydration" responsibility doesn't share its hook with
//! AST-substitution (the `lex.include` lifecycle).

use lex_extension::schema::{
    BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, ParamSpec, ParamType, Schema,
};
use lex_extension::wire::{Position, Range, WireInline, WireNode, WireRow, WireTableCell};
use std::collections::BTreeMap;

/// Fully-qualified label for the canonical tabular table.
pub const LEX_TABULAR_TABLE: &str = "lex.tabular.table";

/// Parse markdown-style pipe-table source text into a typed
/// [`WireNode::Table`].
///
/// Input shape: header row, alignment row (`|---|---|` or
/// `|:---:|---:|`), then one body row per remaining non-blank line.
/// Cells are split on `|`; leading/trailing pipes are optional.
///
/// Alignment markers in the separator row map to per-column entries
/// in `column_aligns`: `:---:` → `"center"`, `---:` → `"right"`,
/// `:---` → `"left"`, otherwise `""`. `column_aligns.length` equals
/// the widest row (the `wire_version: 2` invariant).
pub fn parse_pipe_table_to_wire(content: &str) -> WireNode {
    let lines: Vec<&str> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    let default_range = Range {
        start: Position(0, 0),
        end: Position(0, 0),
    };

    if lines.is_empty() {
        return WireNode::Table {
            range: default_range,
            origin: None,
            caption: String::new(),
            header_rows: 0,
            column_aligns: Vec::new(),
            rows: Vec::new(),
            footnotes: Vec::new(),
        };
    }

    let mut rows: Vec<WireRow> = Vec::new();
    let mut column_aligns: Vec<String> = Vec::new();

    // Header row (first line)
    let header_cells = parse_pipe_row(lines[0]);
    rows.push(WireRow {
        cells: header_cells
            .into_iter()
            .map(|c| WireTableCell {
                inlines: vec![WireInline::Text { text: c }],
                colspan: 1,
                rowspan: 1,
            })
            .collect(),
    });
    let header_rows: u32 = 1;

    // Alignment row (second line). Skipped as a data row; populates
    // `column_aligns`.
    let mut body_start_idx = 1;
    if lines.len() > 1 {
        let separator = lines[1];
        if separator.contains(['-', '|']) {
            for part in parse_pipe_row(separator) {
                let trimmed = part.trim();
                column_aligns.push(match (trimmed.starts_with(':'), trimmed.ends_with(':')) {
                    (true, true) => "center".to_string(),
                    (false, true) => "right".to_string(),
                    (true, false) => "left".to_string(),
                    (false, false) => String::new(),
                });
            }
            body_start_idx = 2;
        }
    }

    // Body rows
    for line in lines.iter().skip(body_start_idx) {
        let cells = parse_pipe_row(line);
        rows.push(WireRow {
            cells: cells
                .into_iter()
                .map(|c| WireTableCell {
                    inlines: vec![WireInline::Text { text: c }],
                    colspan: 1,
                    rowspan: 1,
                })
                .collect(),
        });
    }

    // Ensure `column_aligns.length` matches the widest row (the wire
    // spec invariant). Pad with `""` if the separator row is shorter
    // (or absent).
    let widest = rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
    while column_aligns.len() < widest {
        column_aligns.push(String::new());
    }

    WireNode::Table {
        range: default_range,
        origin: None,
        caption: String::new(),
        header_rows,
        column_aligns,
        rows,
        footnotes: Vec::new(),
    }
}

/// Split a `|`-delimited row into per-cell strings. Leading and
/// trailing `|` are optional and stripped; cells are individually
/// trimmed.
fn parse_pipe_row(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(|s| s.trim().to_string()).collect()
}

pub fn lex_tabular_table_schema() -> Schema {
    Schema {
        schema_version: 1,
        label: LEX_TABULAR_TABLE.into(),
        description: Some(
            "Pipe-table verbatim. The verbatim body uses markdown-style pipe-table syntax \
             (`| col1 | col2 |\\n|------|------|\\n| ... |`). The schema's `on_ir_build` \
             hook parses it into a typed `WireNode::Table` consumed by the IR builder \
             (#615 unified registry surface)."
                .into(),
        ),
        params: {
            // Organizational-hint params carried by the native-table
            // marker form (`:: table align=lr header=1 ::`). Optional —
            // a bare `:: table ::` (or the historical verbatim form) is
            // still valid. Declaring them gives type-checking +
            // completion instead of silent pass-through.
            let mut params = BTreeMap::new();
            params.insert(
                "align".to_string(),
                ParamSpec {
                    ty: ParamType::String,
                    required: false,
                    default: None,
                    description: Some(
                        "Per-column alignment hint, one char per column (`l`/`c`/`r`).".into(),
                    ),
                    pattern: None,
                    values: Vec::new(),
                },
            );
            params.insert(
                "header".to_string(),
                ParamSpec {
                    ty: ParamType::Int,
                    required: false,
                    default: None,
                    description: Some("Number of leading header rows.".into()),
                    pattern: None,
                    values: Vec::new(),
                },
            );
            params
        },
        attaches_to: vec!["table".into(), "verbatim".into()],
        body: BodyShape {
            kind: BodyKind::Text,
            presence: BodyPresence::Optional,
            description: Some(
                "Pipe-table source: header row, alignment row, then one row per body line. \
                 Absent for the native-table marker form (`:: table align=… ::`)."
                    .into(),
            ),
        },
        verbatim_label: true,
        capabilities: Capabilities::default(),
        hooks: HookSet {
            ir_build: true,
            ..HookSet::default()
        },
        handler: None,
        diagnostics: Vec::new(),
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
        // The native organizational-hint form attaches to a `table`;
        // the historical verbatim form (still emitted by babel's
        // `to_lex`) attaches to a `verbatim`. Both must be accepted.
        assert!(schema.attaches_to.contains(&"table".to_string()));
        assert!(schema.attaches_to.contains(&"verbatim".to_string()));
        assert_eq!(schema.body.kind, BodyKind::Text);
        // The native marker form (`:: table align=… ::`) has no body,
        // so presence is optional.
        assert_eq!(schema.body.presence, BodyPresence::Optional);
    }

    #[test]
    fn tabular_schema_declares_ir_build_hook() {
        // #615: `lex.tabular.table` declares the IR-build lifecycle hook
        // (verbatim hydration produces typed `WireNode::Table` for the
        // IR builder). `on_resolve` is no longer used by this label —
        // that hook is for AST substitution (`lex.include`), a
        // different lifecycle.
        let schema = lex_tabular_table_schema();
        assert!(schema.hooks.ir_build);
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
