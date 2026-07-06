//! Pipe-table rendering for the Lex serializer.
//!
//! Projects a structural [`Table`] onto a rectangular column grid — re-deriving
//! the `>>`/`^^` merge markers the parser consumed — and emits it as a padded,
//! markdown-style pipe table. Driven from `LexSerializer::visit_table`.

use super::LexSerializer;
use lex_core::lex::ast::{Table, TableCell, TableCellAlignment, TableRow};

/// What occupies a single grid column in a single row.
pub(super) enum Slot<'a> {
    /// The top-left (originating) cell of a span; carries its content.
    Origin(&'a TableCell),
    /// A column absorbed by a colspan to its left — re-emits `>>` (lex#683).
    Colspan,
    /// A column absorbed by a rowspan above it — re-emits `^^` (lex#694).
    Rowspan,
    /// Padding for a column a short row never reaches but that sits *before* a
    /// rowspan-covered column further right; keeps later `^^` markers in place.
    Empty,
}

impl Slot<'_> {
    /// The literal a slot renders as, for width and emission.
    pub(super) fn text(&self) -> &str {
        match self {
            Slot::Origin(cell) => cell.content.as_string().trim(),
            Slot::Colspan => ">>",
            Slot::Rowspan => "^^",
            Slot::Empty => "",
        }
    }
}

/// Project the table's ragged rows onto a rectangular column grid, re-deriving
/// the merge markers the parser consumed: `>>` for each column a colspan
/// absorbed (within a row) and `^^` for each column a rowspan absorbed (from the
/// row above). The parser removes absorbed cells and bumps the spanning cell's
/// colspan/rowspan, so without this reconstruction the markers — and the spans —
/// are lost on a re-format (lex#683, lex#694).
pub(super) fn build_grid<'a>(rows: &[&'a TableRow]) -> Vec<Vec<Slot<'a>>> {
    // `carry[col]` = remaining continuation rows still covered by a rowspan that
    // originated above. Grows on demand since the column count emerges here.
    let mut carry: Vec<usize> = Vec::new();
    let mut grid: Vec<Vec<Slot>> = Vec::with_capacity(rows.len());

    for row in rows {
        let mut slots: Vec<Slot> = Vec::new();
        let mut cells = row.cells.iter();
        let mut col = 0;
        loop {
            if carry.get(col).copied().unwrap_or(0) > 0 {
                // This column is mid-rowspan from a cell above.
                carry[col] -= 1;
                slots.push(Slot::Rowspan);
                col += 1;
            } else if let Some(cell) = cells.next() {
                let span = cell.colspan.max(1);
                if cell.rowspan > 1 {
                    // Reserve the continuation rows across the cell's full width.
                    for c in col..col + span {
                        if carry.len() <= c {
                            carry.resize(c + 1, 0);
                        }
                        carry[c] += cell.rowspan - 1;
                    }
                }
                slots.push(Slot::Origin(cell));
                for _ in 1..span {
                    slots.push(Slot::Colspan);
                }
                col += span;
            } else if carry.iter().skip(col).any(|&r| r > 0) {
                // Cells are exhausted but a rowspan still covers a column further
                // right; pad this hole so that column's `^^` is still emitted and
                // its `carry` is consumed in *this* row rather than leaking down.
                slots.push(Slot::Empty);
                col += 1;
            } else {
                // No more cells and nothing covered ahead: row is done.
                break;
            }
        }
        grid.push(slots);
    }
    grid
}

/// Emit a structural Table as a markdown-style pipe table, padded for
/// column alignment. The column count is the max-width row; shorter
/// rows pad with empty cells. Alignment follows the per-cell `align`
/// attribute, which the parser sets from the markdown alignment row
/// (`:---`, `:---:`, `---:`).
pub(super) fn emit_pipe_table(serializer: &mut LexSerializer, table: &Table) {
    let all_rows: Vec<&TableRow> = table
        .header_rows
        .iter()
        .chain(table.body_rows.iter())
        .collect();
    if all_rows.is_empty() {
        return;
    }

    let grid = build_grid(&all_rows);
    let col_count = grid.iter().map(Vec::len).max().unwrap_or(0);
    if col_count == 0 {
        return;
    }

    // Compute per-column alignment (first explicit cell wins) and widths.
    let aligns = compute_column_aligns(&grid, col_count);
    let widths = compute_column_widths(&grid, col_count, &aligns);

    let header_count = table.header_rows.len();
    for (idx, slots) in grid.iter().enumerate() {
        serializer.write_line(&format_grid_row(slots, &widths));
        // Separator row sits between the header rows and the body.
        if header_count > 0 && idx + 1 == header_count {
            serializer.write_line(&format_separator_row(&widths, &aligns));
        }
    }
}

fn compute_column_aligns(grid: &[Vec<Slot>], col_count: usize) -> Vec<TableCellAlignment> {
    let mut aligns = vec![TableCellAlignment::None; col_count];
    for slots in grid {
        for (col, slot) in slots.iter().enumerate() {
            if let Slot::Origin(cell) = slot {
                if aligns[col] == TableCellAlignment::None && cell.align != TableCellAlignment::None
                {
                    aligns[col] = cell.align;
                }
            }
        }
    }
    aligns
}

fn compute_column_widths(
    grid: &[Vec<Slot>],
    col_count: usize,
    aligns: &[TableCellAlignment],
) -> Vec<usize> {
    let mut widths = vec![0usize; col_count];
    for slots in grid {
        for (col, slot) in slots.iter().enumerate() {
            widths[col] = widths[col].max(slot.text().chars().count());
        }
    }
    // Separator widths need a minimum of 3 (`---`) plus 1 for each
    // colon a `:left`/`right:`/`:center:` marker adds. Round up so the
    // separator row's `---` segment is at least as wide as the content.
    for (col, w) in widths.iter_mut().enumerate() {
        let min = match aligns.get(col).copied().unwrap_or(TableCellAlignment::None) {
            TableCellAlignment::Center => 5,                           // `:---:`
            TableCellAlignment::Left | TableCellAlignment::Right => 4, // `:---` / `---:`
            TableCellAlignment::None => 3,
        };
        if *w < min {
            *w = min;
        }
    }
    widths
}

/// Append one `| text |`-style cell padded to `width` to `out` (the leading
/// `|` of the row, and the `|` after each cell, are emitted here).
fn push_cell(out: &mut String, text: &str, width: usize) {
    out.push(' ');
    out.push_str(text);
    for _ in text.chars().count()..width {
        out.push(' ');
    }
    out.push(' ');
    out.push('|');
}

/// Emit a row's cells, one per slot the grid produced for it.
///
/// A short (ragged) row emits exactly its own cells — it is *not* padded out to
/// the table's column count, since a phantom trailing cell would re-parse as a
/// real empty cell and change the row's cell count (a faithfulness break,
/// lex#792). `build_grid` has already inserted `Slot::Empty` for the columns a
/// short row must still render (those sitting before a rowspan-covered column
/// further right), so the slot list is exactly what should be emitted.
fn format_grid_row(slots: &[Slot], widths: &[usize]) -> String {
    let mut out = String::from("|");
    for (col, slot) in slots.iter().enumerate() {
        let width = widths.get(col).copied().unwrap_or(0);
        push_cell(&mut out, slot.text(), width);
    }
    out
}

fn format_separator_row(widths: &[usize], aligns: &[TableCellAlignment]) -> String {
    let mut out = String::from("|");
    for (i, &w) in widths.iter().enumerate() {
        out.push(' ');
        let align = aligns.get(i).copied().unwrap_or(TableCellAlignment::None);
        match align {
            TableCellAlignment::Left => {
                out.push(':');
                for _ in 1..w {
                    out.push('-');
                }
            }
            TableCellAlignment::Right => {
                for _ in 0..w.saturating_sub(1) {
                    out.push('-');
                }
                out.push(':');
            }
            TableCellAlignment::Center => {
                out.push(':');
                for _ in 0..w.saturating_sub(2) {
                    out.push('-');
                }
                out.push(':');
            }
            TableCellAlignment::None => {
                for _ in 0..w {
                    out.push('-');
                }
            }
        }
        out.push(' ');
        out.push('|');
    }
    out
}
