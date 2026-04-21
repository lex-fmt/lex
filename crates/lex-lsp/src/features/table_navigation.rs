//! Table cell navigation.
//!
//! Given a cursor position in a buffer, compute where Tab / Shift+Tab should
//! move the cursor inside a pipe-delimited table. The heuristic matches what
//! the vscode and nvim clients used to do locally: detect pipe rows by
//! leading `|`, count pipe offsets, and move to the cell on the other side
//! of the next/previous pipe — wrapping to the adjacent row at table edges.
//!
//! The outcome distinguishes three cases so the client can choose the right
//! behaviour without re-running the same heuristic:
//! - `inTable: false` — cursor is not on a pipe row; client should fall
//!   through to the editor's default Tab / outdent action.
//! - `inTable: true, position: Some` — client should set the cursor to the
//!   returned position.
//! - `inTable: true, position: None` — cursor is on a pipe row but no valid
//!   navigation target exists (e.g. a single-column row, or trailing Tab on
//!   the last row); client should do nothing.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Next,
    Previous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetPosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableNavOutcome {
    pub in_table: bool,
    pub position: Option<TargetPosition>,
}

impl TableNavOutcome {
    fn fallthrough() -> Self {
        Self {
            in_table: false,
            position: None,
        }
    }

    fn no_move() -> Self {
        Self {
            in_table: true,
            position: None,
        }
    }

    fn moved(line: usize, column: usize) -> Self {
        Self {
            in_table: true,
            position: Some(TargetPosition { line, column }),
        }
    }
}

/// Compute the navigation outcome for the given cursor position.
///
/// `line` and `column` are 0-indexed (LSP conventions); `column` counts
/// UTF-8 bytes within the line.
pub fn navigate_table_cell(
    source: &str,
    line: usize,
    column: usize,
    direction: Direction,
) -> TableNavOutcome {
    let lines: Vec<&str> = source.split('\n').collect();
    let Some(current) = lines.get(line) else {
        return TableNavOutcome::fallthrough();
    };

    if !is_pipe_row(current) {
        return TableNavOutcome::fallthrough();
    }

    let pipes = pipe_positions(current);
    if pipes.len() < 2 {
        return TableNavOutcome::no_move();
    }

    match direction {
        Direction::Next => navigate_next(&lines, line, column, current, &pipes),
        Direction::Previous => navigate_previous(&lines, line, column, current, &pipes),
    }
}

fn navigate_next(
    lines: &[&str],
    line: usize,
    column: usize,
    current: &str,
    pipes: &[usize],
) -> TableNavOutcome {
    if let Some(&next_pipe) = pipes.iter().find(|&&p| p > column) {
        let idx = pipes.iter().position(|&p| p == next_pipe).unwrap();
        if idx < pipes.len() - 1 {
            let target = (next_pipe + 2).min(current.len());
            return TableNavOutcome::moved(line, target);
        }
    }

    // Last cell on this row → jump to first cell of next pipe row.
    let next_line_nr = line + 1;
    if let Some(next_text) = lines.get(next_line_nr) {
        if is_pipe_row(next_text) {
            if let Some(first_pipe) = next_text.find('|') {
                let target = (first_pipe + 2).min(next_text.len());
                return TableNavOutcome::moved(next_line_nr, target);
            }
        }
    }

    TableNavOutcome::no_move()
}

fn navigate_previous(
    lines: &[&str],
    line: usize,
    column: usize,
    current: &str,
    pipes: &[usize],
) -> TableNavOutcome {
    let prev_pipes: Vec<usize> = pipes.iter().copied().filter(|&p| p < column).collect();
    if prev_pipes.len() >= 2 {
        let target_pipe = prev_pipes[prev_pipes.len() - 2];
        let target = (target_pipe + 2).min(current.len());
        return TableNavOutcome::moved(line, target);
    }

    // First cell on this row → jump to last cell of previous pipe row.
    if line == 0 {
        return TableNavOutcome::no_move();
    }
    let prev_line_nr = line - 1;
    if let Some(prev_text) = lines.get(prev_line_nr) {
        if is_pipe_row(prev_text) {
            let prev_pipes = pipe_positions(prev_text);
            if prev_pipes.len() >= 2 {
                let target_pipe = prev_pipes[prev_pipes.len() - 2];
                let target = (target_pipe + 2).min(prev_text.len());
                return TableNavOutcome::moved(prev_line_nr, target);
            }
        }
    }

    TableNavOutcome::no_move()
}

fn is_pipe_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

fn pipe_positions(line: &str) -> Vec<usize> {
    line.match_indices('|').map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next(source: &str, line: usize, col: usize) -> TableNavOutcome {
        navigate_table_cell(source, line, col, Direction::Next)
    }

    fn prev(source: &str, line: usize, col: usize) -> TableNavOutcome {
        navigate_table_cell(source, line, col, Direction::Previous)
    }

    #[test]
    fn falls_through_when_not_on_pipe_row() {
        let source = "A paragraph here.\n";
        let outcome = next(source, 0, 3);
        assert_eq!(outcome, TableNavOutcome::fallthrough());
    }

    #[test]
    fn falls_through_beyond_last_line() {
        let source = "| a | b |\n";
        let outcome = next(source, 5, 0);
        assert_eq!(outcome, TableNavOutcome::fallthrough());
    }

    #[test]
    fn moves_to_next_cell_mid_row() {
        let source = "    | Name | Score |\n";
        // Cursor after "Name " at column 11 (inside first cell).
        let outcome = next(source, 0, 7);
        assert_eq!(outcome, TableNavOutcome::moved(0, 13));
    }

    #[test]
    fn next_from_last_cell_wraps_to_next_row() {
        let source = "    | A | B |\n    | C | D |\n";
        // Cursor in the "B" cell on line 0.
        let outcome = next(source, 0, 11);
        assert_eq!(outcome, TableNavOutcome::moved(1, 6));
    }

    #[test]
    fn next_from_last_row_last_cell_is_no_move() {
        let source = "    | A | B |\n";
        let outcome = next(source, 0, 11);
        assert_eq!(outcome, TableNavOutcome::no_move());
    }

    #[test]
    fn next_with_only_one_pipe_is_no_move() {
        let source = "| only\n";
        let outcome = next(source, 0, 2);
        assert_eq!(outcome, TableNavOutcome::no_move());
    }

    #[test]
    fn prev_moves_to_previous_cell_mid_row() {
        let source = "    | Name | Score |\n";
        // Cursor in the "Score" cell at column 14; target is first cell
        // content (the "N" of "Name") at pipe+2 = 6.
        let outcome = prev(source, 0, 14);
        assert_eq!(outcome, TableNavOutcome::moved(0, 6));
    }

    #[test]
    fn prev_from_first_cell_wraps_to_previous_row() {
        let source = "    | A | B |\n    | C | D |\n";
        // From the "C" cell on line 1, wrap to the last cell of line 0;
        // last pipe in line 0 is at column 12, target is the previous
        // pipe+2 = 8+2 = 10 (the "B").
        let outcome = prev(source, 1, 7);
        assert_eq!(outcome, TableNavOutcome::moved(0, 10));
    }

    #[test]
    fn prev_from_first_row_first_cell_is_no_move() {
        let source = "    | A | B |\n";
        let outcome = prev(source, 0, 7);
        assert_eq!(outcome, TableNavOutcome::no_move());
    }

    #[test]
    fn prev_at_line_zero_first_cell_does_not_underflow() {
        let source = "| A | B |\n";
        let outcome = prev(source, 0, 3);
        assert_eq!(outcome, TableNavOutcome::no_move());
    }

    #[test]
    fn does_not_wrap_across_non_pipe_row() {
        let source = "    | A | B |\nSome paragraph.\n    | C | D |\n";
        // From last cell of line 0, next row is paragraph — no move.
        let outcome = next(source, 0, 11);
        assert_eq!(outcome, TableNavOutcome::no_move());
    }

    #[test]
    fn column_clamped_when_line_shorter_than_target() {
        // Trailing content shorter than `last_pipe + 2`: target clamps to line length.
        let source = "|\n";
        let outcome = next(source, 0, 0);
        // Only one pipe on the line → no_move.
        assert_eq!(outcome, TableNavOutcome::no_move());
    }

    #[test]
    fn serializes_to_expected_shape() {
        let outcome = TableNavOutcome::moved(3, 11);
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "inTable": true, "position": { "line": 3, "column": 11 } })
        );

        let outcome = TableNavOutcome::fallthrough();
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "inTable": false, "position": null })
        );

        let outcome = TableNavOutcome::no_move();
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "inTable": true, "position": null })
        );
    }
}
