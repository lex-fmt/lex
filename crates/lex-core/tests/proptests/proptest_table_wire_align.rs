//! Property tests for the wire-codec preservation of table column
//! alignment, including the `colspan > 1` cases that motivated #585.
//!
//! Invariants under test:
//!
//! 1. `column_aligns.length` on the wire equals the table's column
//!    count (the max sum-of-colspans across all rows).
//!
//! 2. Every column's alignment as seen on the wire is preserved
//!    through `to_wire_node` → JSON → `from_wire_node`: the alignment
//!    of a body cell starting at column `c` round-trips back to the
//!    alignment at column `c` in the reconstructed table.
//!
//! Coverage that the targeted unit test alone missed: variable column
//! counts, multiple body rows, mixed alignments per column, and
//! spanning cells at arbitrary positions.

use lex_core::lex::ast::elements::table::{Table, TableCell, TableCellAlignment, TableRow};
use lex_core::lex::ast::elements::verbatim::VerbatimBlockMode;
use lex_core::lex::ast::{ContentItem, TextContent};
use lex_core::lex::wire::{from_wire_node, to_wire_node};
use lex_extension::wire::WireNode;
use proptest::prelude::*;

/// Random alignment marker.
fn alignment_strategy() -> impl Strategy<Value = TableCellAlignment> {
    prop_oneof![
        Just(TableCellAlignment::None),
        Just(TableCellAlignment::Left),
        Just(TableCellAlignment::Center),
        Just(TableCellAlignment::Right),
    ]
}

/// String representation of a wire-side alignment marker.
fn align_to_wire_string(a: TableCellAlignment) -> &'static str {
    match a {
        TableCellAlignment::Left => "left",
        TableCellAlignment::Center => "center",
        TableCellAlignment::Right => "right",
        TableCellAlignment::None => "",
    }
}

/// A table laid out by **column count and per-column alignment**, with
/// random body rows whose cells span 1–N columns chosen so the row's
/// total `sum(colspan)` equals the column count.
///
/// Returns `(column_count, per_column_alignments, table)`. The table
/// has one header row (single cells, no spans) and one or more body
/// rows; every body cell carries the alignment of the leftmost
/// column it covers (so the wire codec's "first non-None per column"
/// rule produces the per-column alignment vector deterministically).
fn table_with_colspans_strategy() -> impl Strategy<Value = (usize, Vec<TableCellAlignment>, Table)>
{
    (2usize..=5)
        .prop_flat_map(|cols| {
            let aligns = prop::collection::vec(alignment_strategy(), cols);
            let body_rows = prop::collection::vec(row_strategy(cols), 1..=3);
            (Just(cols), aligns, body_rows)
        })
        .prop_map(
            |(cols, aligns, body_rows): (usize, Vec<_>, Vec<Vec<(usize, TableCellAlignment)>>)| {
                // Header: one cell per column, no spans, alignment = column align.
                let header = TableRow::new(
                    aligns
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            TableCell::new(TextContent::from_string(format!("H{i}"), None))
                                .with_header(true)
                                .with_align(*a)
                        })
                        .collect(),
                );

                // Body rows: each cell's align is the leftmost-column's
                // alignment. That mirrors what a writer would put on the
                // source — and lets us assert tight equality after the
                // round trip.
                let body: Vec<TableRow> = body_rows
                    .into_iter()
                    .map(|row| {
                        let mut col = 0;
                        let cells = row
                            .into_iter()
                            .map(|(span, _)| {
                                let leftmost_col = col;
                                let cell = TableCell::new(TextContent::from_string(
                                    format!("c{leftmost_col}s{span}"),
                                    None,
                                ))
                                .with_span(span, 1)
                                .with_align(aligns[leftmost_col]);
                                col += span;
                                cell
                            })
                            .collect();
                        TableRow::new(cells)
                    })
                    .collect();

                let table = Table::new(
                    TextContent::from_string(String::new(), None),
                    vec![header],
                    body,
                    VerbatimBlockMode::Inflow,
                );
                (cols, aligns, table)
            },
        )
}

/// Strategy for a single body row: pick a sequence of `colspan`
/// values whose sum is exactly `cols`, plus dummy `TableCellAlignment`
/// per cell (unused — the table-level mapper overrides cell alignment
/// to match the column alignment of the leftmost covered column).
fn row_strategy(cols: usize) -> impl Strategy<Value = Vec<(usize, TableCellAlignment)>> {
    // Greedy random partition of `cols` into spans of 1..=cols.
    Just(cols).prop_flat_map(move |total| {
        partition_strategy(total).prop_map(|spans| {
            spans
                .into_iter()
                .map(|s| (s, TableCellAlignment::None))
                .collect()
        })
    })
}

/// Partition `n` into a `Vec<usize>` of positive parts. Each part is
/// at least 1; the parts sum to `n`.
fn partition_strategy(n: usize) -> impl Strategy<Value = Vec<usize>> {
    // Generate up to `n` parts, then trim/pad to make them sum to n.
    prop::collection::vec(1usize..=n.max(1), 1..=n.max(1)).prop_map(move |mut parts| {
        let mut total: usize = parts.iter().sum();
        while total > n && parts.len() > 1 {
            // Trim the last element until we don't overshoot.
            let last = parts.pop().unwrap();
            total -= last;
        }
        if total > n {
            // Single element overshot — clamp it.
            parts[0] = n;
        } else if total < n {
            // Pad with single-column cells.
            parts.extend(std::iter::repeat_n(1, n - total));
        }
        parts
    })
}

fn json_round_trip(node: &WireNode) -> WireNode {
    let s = serde_json::to_string(node).expect("serialize wire node");
    serde_json::from_str(&s).expect("deserialize wire node")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// `column_aligns.length` equals the column count of the source
    /// table, regardless of where colspan cells are placed.
    #[test]
    fn wire_column_aligns_width_matches_column_count(
        (cols, _aligns, table) in table_with_colspans_strategy(),
    ) {
        let item = ContentItem::Table(Box::new(table));
        let wire = to_wire_node(&item);
        if let WireNode::Table { column_aligns, .. } = &wire {
            prop_assert_eq!(
                column_aligns.len(),
                cols,
                "column_aligns must have one entry per column (got {} for {} cols)",
                column_aligns.len(),
                cols
            );
        } else {
            prop_assert!(false, "expected WireNode::Table");
        }
    }

    /// Per-column alignment is preserved on the wire under arbitrary
    /// colspan layouts: column `c` keeps its alignment regardless of
    /// which body cell happens to cover it.
    #[test]
    fn wire_column_aligns_values_match_column_alignments(
        (_cols, aligns, table) in table_with_colspans_strategy(),
    ) {
        let item = ContentItem::Table(Box::new(table));
        let wire = to_wire_node(&item);
        if let WireNode::Table { column_aligns, .. } = &wire {
            let expected: Vec<String> = aligns
                .iter()
                .map(|a| align_to_wire_string(*a).to_string())
                .collect();
            prop_assert_eq!(
                column_aligns,
                &expected,
                "column_aligns must carry the per-column alignment in order"
            );
        } else {
            prop_assert!(false, "expected WireNode::Table");
        }
    }

    /// After JSON round-trip and reverse codec, every body cell's
    /// alignment matches the alignment of the column it starts at.
    /// This is what would have caught the index-by-cell-position bug
    /// in `table_from_wire`: a colspan cell shifts subsequent cells
    /// to later columns, so the alignment lookup must track a column
    /// cursor.
    #[test]
    fn round_trip_preserves_cell_alignment_after_colspans(
        (_cols, aligns, table) in table_with_colspans_strategy(),
    ) {
        let item = ContentItem::Table(Box::new(table));
        let wire = to_wire_node(&item);
        let back = from_wire_node(&json_round_trip(&wire)).expect("from_wire ok");
        match &back[0] {
            ContentItem::Table(t) => {
                for row in &t.body_rows {
                    let mut col = 0usize;
                    for cell in &row.cells {
                        prop_assert_eq!(
                            cell.align,
                            aligns[col],
                            "cell starting at column {} must carry column alignment {:?}, got {:?}",
                            col,
                            aligns[col],
                            cell.align
                        );
                        col += cell.colspan.max(1);
                    }
                }
            }
            other => prop_assert!(false, "expected Table, got {:?}", other),
        }
    }
}
