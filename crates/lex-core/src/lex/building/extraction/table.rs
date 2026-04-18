//! Table Data Extraction
//!
//! This module handles the extraction of table content from line tokens,
//! reusing the verbatim block's outer structure handling (mode detection,
//! indentation wall stripping) and adding pipe-row parsing on top.
//!
//! # Pipeline
//!
//! 1. Reuse verbatim extraction for mode detection and wall stripping
//! 2. Parse wall-stripped lines into pipe rows
//! 3. Detect separator lines (cosmetic, discarded)
//! 4. Detect multi-line mode (blank lines between pipe groups)
//! 5. Resolve merge markers (>> for colspan, ^^ for rowspan)
//! 6. Apply header/align hints from closing annotation parameters

use super::verbatim::extract_verbatim_block_data;
use crate::lex::ast::elements::verbatim::VerbatimBlockMode;
use crate::lex::escape::split_respecting_escape_with_ranges;
use crate::lex::token::LineToken;
use std::borrow::Cow;
use std::ops::Range as ByteRange;

/// Extracted data for a single table cell (pre-AST).
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct TableCellData {
    pub text: String,
    /// Raw text preserving indentation (for block content parsing in multi-line mode)
    pub raw_text: Option<String>,
    pub byte_range: ByteRange<usize>,
    pub colspan: usize,
    pub rowspan: usize,
    pub is_header: bool,
    /// Block content parsed from multi-line cell text, if any.
    pub block_content: Option<Vec<crate::lex::ast::elements::typed_content::ContentElement>>,
}

/// Extracted data for a single table row (pre-AST).
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct TableRowData {
    pub cells: Vec<TableCellData>,
    pub byte_range: ByteRange<usize>,
}

/// Extracted data for a footnote line (pre-AST).
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct FootnoteLineData {
    pub marker: String,
    pub text: String,
    pub byte_range: ByteRange<usize>,
}

/// Extracted data for building a Table AST node.
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct TableData {
    pub subject_text: String,
    pub subject_byte_range: ByteRange<usize>,
    pub header_rows: Vec<TableRowData>,
    pub body_rows: Vec<TableRowData>,
    pub footnotes: Vec<FootnoteLineData>,
    pub mode: VerbatimBlockMode,
}

/// Extract table data from the verbatim-like outer structure.
///
/// Reuses verbatim extraction for wall stripping, then parses pipe rows.
pub(in crate::lex::building) fn extract_table_data(
    subject_token: &LineToken,
    content_tokens: &[LineToken],
    source: &str,
) -> TableData {
    // Reuse verbatim extraction for mode detection + wall stripping
    let verbatim_data = extract_verbatim_block_data(subject_token, content_tokens, source);
    let mode = verbatim_data.mode;

    // Tables don't support groups (no multiple subject lines) — use only the first group
    let group = verbatim_data
        .groups
        .into_iter()
        .next()
        .expect("Table must have at least one group");

    let subject_text = group.subject_text;
    let subject_byte_range = group.subject_byte_range;

    // Parse the wall-stripped content lines into rows and extract footnotes
    let classified = classify_lines(&group.content_lines);
    let is_multiline = detect_multiline(&classified);
    let raw_rows = if is_multiline {
        parse_multiline_rows(&classified)
    } else {
        parse_compact_rows(&classified)
    };
    let footnotes = extract_footnote_lines(&classified);

    // Resolve merge markers
    let mut rows = resolve_merges(raw_rows);

    // Parse block content in multi-line cells
    if is_multiline {
        for row in &mut rows {
            for cell in &mut row.cells {
                let parse_text = cell.raw_text.as_deref().unwrap_or(&cell.text);
                cell.block_content = parse_cell_content(parse_text);
            }
        }
    }

    // Default split: first row is header. The assembly stage may re-split
    // based on :: table :: annotation parameters (header=N).
    let header_count = 1;
    let (header_rows, body_rows) = split_header_body(&mut rows, header_count);

    TableData {
        subject_text,
        subject_byte_range,
        header_rows,
        body_rows,
        footnotes,
        mode,
    }
}

/// Parse wall-stripped content lines into raw table rows (test helper).
#[cfg(test)]
fn parse_pipe_rows(content_lines: &[(String, ByteRange<usize>)]) -> Vec<TableRowData> {
    // First, classify lines and check for multi-line mode
    let classified = classify_lines(content_lines);
    let is_multiline = detect_multiline(&classified);

    if is_multiline {
        parse_multiline_rows(&classified)
    } else {
        parse_compact_rows(&classified)
    }
}

/// A classified content line.
#[derive(Debug)]
enum LineKind<'a> {
    /// A pipe row with parsed cell texts
    PipeRow {
        /// Trimmed cell texts (for merge markers, compact mode).
        /// `Cow::Owned` when backslash escapes were stripped (e.g. `\|`), otherwise borrowed.
        cells: Vec<Cow<'a, str>>,
        /// Raw cell texts preserving leading whitespace (for block content in multi-line mode)
        raw_cells: Vec<Cow<'a, str>>,
        /// Per-cell byte ranges (of trimmed text) in the source
        cell_ranges: Vec<ByteRange<usize>>,
        line_range: &'a ByteRange<usize>,
    },
    /// A blank line (row group separator in multi-line mode)
    Blank,
    /// A non-pipe, non-blank line (potential footnote)
    Other {
        text: &'a str,
        line_range: &'a ByteRange<usize>,
    },
}

/// Classify all content lines into pipe rows, blanks, or other.
///
/// Pipe-row parsing honors the Lex structural escape convention:
/// - `\|` inside a cell is a literal pipe (not a cell boundary); the backslash
///   is stripped in the returned cell text.
/// - Content inside balanced backticks (`` `...` ``) is a literal region:
///   pipes inside it do not split, and backslashes are passed through verbatim
///   (so that code spans like `` `a|b` `` survive as a single cell).
///
/// Byte ranges point to source positions of the trimmed segment text,
/// regardless of whether the cell text had backslashes stripped.
fn classify_lines<'a>(content_lines: &'a [(String, ByteRange<usize>)]) -> Vec<LineKind<'a>> {
    content_lines
        .iter()
        .map(|(text, range)| {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                LineKind::Blank
            } else if trimmed.starts_with('|') && !is_separator_line(trimmed) {
                // Offset of trimmed text within the full line text
                let trim_offset = text.len() - text.trim_start().len();

                // Escape-aware split: respects `\|` and backtick literal regions.
                let segments = split_respecting_escape_with_ranges(trimmed, '|', Some('`'));

                // Leading/trailing fully-empty segments come from the `|` at row start/end.
                let start = if segments
                    .first()
                    .is_some_and(|(cow, _)| cow.trim().is_empty())
                {
                    1
                } else {
                    0
                };
                let end = if segments
                    .last()
                    .is_some_and(|(cow, _)| cow.trim().is_empty())
                {
                    segments.len() - 1
                } else {
                    segments.len()
                };

                let mut cell_ranges = Vec::with_capacity(end.saturating_sub(start));
                let mut cells: Vec<Cow<'a, str>> = Vec::with_capacity(end.saturating_sub(start));
                let mut raw_cells: Vec<Cow<'a, str>> =
                    Vec::with_capacity(end.saturating_sub(start));

                for (i, (cow, seg_range)) in segments.into_iter().enumerate() {
                    if i < start || i >= end {
                        continue;
                    }
                    // Byte range computation uses the ORIGINAL source segment (pre-strip),
                    // so diagnostic spans always point to real source positions.
                    let src_seg = &trimmed[seg_range.clone()];
                    let lead_ws = src_seg.len() - src_seg.trim_start().len();
                    let trail_ws = src_seg.len() - src_seg.trim_end().len();
                    let cell_start = range.start + trim_offset + seg_range.start + lead_ws;
                    let cell_end = range.start + trim_offset + seg_range.end - trail_ws;
                    cell_ranges.push(cell_start..cell_end);

                    let (cell, raw) = trim_cell_pair(cow);
                    cells.push(cell);
                    raw_cells.push(raw);
                }

                LineKind::PipeRow {
                    cells,
                    raw_cells,
                    cell_ranges,
                    line_range: range,
                }
            } else {
                LineKind::Other {
                    text: trimmed,
                    line_range: range,
                }
            }
        })
        .collect()
}

/// Consume a segment `Cow` and produce both `(trimmed, trim_end_only)` variants.
///
/// Borrowed inputs produce two sub-slices — zero allocations. Owned inputs reuse
/// the existing `String` whenever a trim would be a no-op, allocating only for
/// the variants that actually differ.
fn trim_cell_pair<'a>(cow: Cow<'a, str>) -> (Cow<'a, str>, Cow<'a, str>) {
    match cow {
        Cow::Borrowed(s) => (Cow::Borrowed(s.trim()), Cow::Borrowed(s.trim_end())),
        Cow::Owned(s) => {
            let full_noop = s.trim().len() == s.len();
            let end_noop = s.trim_end().len() == s.len();
            match (full_noop, end_noop) {
                (true, true) => {
                    // Neither trim modifies the string; share via one clone.
                    let clone = s.clone();
                    (Cow::Owned(s), Cow::Owned(clone))
                }
                (true, false) => {
                    let te = s.trim_end().to_string();
                    (Cow::Owned(s), Cow::Owned(te))
                }
                (false, true) => {
                    let t = s.trim().to_string();
                    (Cow::Owned(t), Cow::Owned(s))
                }
                (false, false) => {
                    let t = s.trim().to_string();
                    let te = s.trim_end().to_string();
                    (Cow::Owned(t), Cow::Owned(te))
                }
            }
        }
    }
}

/// Detect multi-line mode: true if any blank line appears between two pipe rows.
fn detect_multiline(lines: &[LineKind]) -> bool {
    let mut seen_pipe = false;
    let mut seen_blank_after_pipe = false;

    for line in lines {
        match line {
            LineKind::PipeRow { .. } => {
                if seen_blank_after_pipe {
                    return true;
                }
                seen_pipe = true;
            }
            LineKind::Blank => {
                if seen_pipe {
                    seen_blank_after_pipe = true;
                }
            }
            LineKind::Other { .. } => {}
        }
    }
    false
}

/// Compact mode: each pipe line is an independent row.
fn parse_compact_rows(lines: &[LineKind]) -> Vec<TableRowData> {
    lines
        .iter()
        .filter_map(|line| {
            if let LineKind::PipeRow {
                cells,
                cell_ranges,
                line_range,
                ..
            } = line
            {
                let cell_data = cells
                    .iter()
                    .zip(cell_ranges.iter())
                    .map(|(text, cell_range)| TableCellData {
                        text: text.to_string(),
                        raw_text: None,
                        byte_range: cell_range.clone(),
                        colspan: 1,
                        rowspan: 1,
                        is_header: false,
                        block_content: None,
                    })
                    .collect();
                Some(TableRowData {
                    cells: cell_data,
                    byte_range: (*line_range).clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Multi-line mode: blank lines delimit row groups. Consecutive pipe lines
/// within a group form a single row with multi-line cell content.
fn parse_multiline_rows(lines: &[LineKind]) -> Vec<TableRowData> {
    let mut rows = Vec::new();
    let mut current_group: Vec<&LineKind> = Vec::new();

    for line in lines {
        match line {
            LineKind::Blank => {
                if !current_group.is_empty() {
                    if let Some(row) = merge_group(&current_group) {
                        rows.push(row);
                    }
                    current_group.clear();
                }
            }
            LineKind::PipeRow { .. } => {
                current_group.push(line);
            }
            LineKind::Other { .. } => {}
        }
    }

    // Flush final group
    if !current_group.is_empty() {
        if let Some(row) = merge_group(&current_group) {
            rows.push(row);
        }
    }

    rows
}

/// Merge a group of pipe lines into a single row.
///
/// The first line establishes the cells. Continuation lines append to the
/// corresponding cell's text (joined with newline). Whitespace-only
/// continuation cells represent blank lines (paragraph separators) when the
/// cell already has content.
///
/// Two texts are built per cell:
/// - `merged_texts`: trimmed cell content (for inline use and merge markers)
/// - `merged_raw`: raw cell content preserving leading whitespace (for block parsing)
fn merge_group(group: &[&LineKind]) -> Option<TableRowData> {
    if group.is_empty() {
        return None;
    }

    // Extract the first line's cells as the base
    let LineKind::PipeRow {
        cells: first_cells,
        raw_cells: first_raw,
        cell_ranges: first_cell_ranges,
        line_range: first_range,
    } = group[0]
    else {
        return None;
    };

    let mut merged_texts: Vec<String> = first_cells.iter().map(|s| s.to_string()).collect();
    let mut merged_raw: Vec<String> = first_raw.iter().map(|s| s.to_string()).collect();
    let merged_cell_ranges: Vec<ByteRange<usize>> = first_cell_ranges.clone();
    let mut row_range = (*first_range).clone();

    // Append continuation lines
    for line in &group[1..] {
        let LineKind::PipeRow {
            cells: cont_cells,
            raw_cells: cont_raw,
            line_range: cont_range,
            ..
        } = line
        else {
            continue;
        };

        // Extend the row's byte range to cover all continuation lines
        row_range = row_range.start..cont_range.end;

        for (col, cell_text) in cont_cells.iter().enumerate() {
            if col < merged_texts.len() {
                if !cell_text.is_empty() {
                    // Non-empty continuation: append with newline
                    if !merged_texts[col].is_empty() {
                        merged_texts[col].push('\n');
                    }
                    merged_texts[col].push_str(cell_text);
                } else if !merged_texts[col].is_empty() {
                    // Whitespace-only continuation on a non-empty cell: blank line
                    // (This creates paragraph separators for block content)
                    merged_raw[col].push('\n');
                }
            }

            // Build raw text for all columns
            if col < merged_raw.len() {
                let raw: &str = cont_raw.get(col).map(|c| c.as_ref()).unwrap_or("");
                if !raw.trim().is_empty() {
                    merged_raw[col].push('\n');
                    merged_raw[col].push_str(raw);
                } else if !merged_raw[col].is_empty() {
                    // Blank line separator in raw text
                    merged_raw[col].push('\n');
                }
            }
        }
    }

    // Dedent raw text: compute per-cell indentation baseline and strip it
    for raw in &mut merged_raw {
        dedent_cell_text(raw);
    }

    let cells = merged_texts
        .into_iter()
        .zip(merged_raw)
        .enumerate()
        .map(|(i, (text, raw))| TableCellData {
            text,
            raw_text: if group.len() > 1 { Some(raw) } else { None },
            byte_range: merged_cell_ranges
                .get(i)
                .cloned()
                .unwrap_or_else(|| row_range.clone()),
            colspan: 1,
            rowspan: 1,
            is_header: false,
            block_content: None,
        })
        .collect();

    Some(TableRowData {
        cells,
        byte_range: row_range,
    })
}

/// Dedent cell text by stripping the common leading whitespace from all non-empty lines.
fn dedent_cell_text(text: &mut String) {
    let baseline = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);

    if baseline > 0 {
        let dedented: String = text
            .lines()
            .map(|line| {
                if line.trim().is_empty() {
                    ""
                } else if line.len() > baseline {
                    &line[baseline..]
                } else {
                    line.trim_start()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        *text = dedented;
    }
}

/// Parse cell text to detect block-level content.
///
/// For multi-line cells, re-parses the merged text as a standalone document.
/// Returns `None` if the cell is inline-only (single paragraph matching original text).
/// Returns `Some(children)` if the cell contains block content.
fn parse_cell_content(
    text: &str,
) -> Option<Vec<crate::lex::ast::elements::typed_content::ContentElement>> {
    use crate::lex::ast::elements::typed_content::ContentElement;
    use crate::lex::ast::ContentItem;
    use crate::lex::parsing::parse_document;

    // Single-line cells are always inline
    if !text.contains('\n') {
        return None;
    }

    // Try to parse the cell text as a standalone document
    let cell_source = format!("{text}\n");
    let mut doc = parse_document(&cell_source).ok()?;

    let items: Vec<&ContentItem> = doc
        .root
        .children
        .iter()
        .filter(|item| !matches!(item, ContentItem::BlankLineGroup(_)))
        .collect();

    // If it's a single paragraph, check if it matches the original text
    if items.len() == 1 {
        if let ContentItem::Paragraph(p) = items[0] {
            let para_text = p.text();
            if para_text.trim() == text.trim() {
                return None; // Just a plain paragraph — stay inline
            }
        }
    }

    // Include document-level annotations (these end up outside root.children)
    let mut all_items: Vec<ContentItem> = doc
        .annotations
        .drain(..)
        .map(ContentItem::Annotation)
        .collect();
    all_items.extend(std::mem::take(doc.root.children.as_mut_vec()));

    // Convert to ContentElement (filtering out blank lines and sessions)
    let children: Vec<ContentElement> = all_items
        .into_iter()
        .filter_map(|item| ContentElement::try_from(item).ok())
        .filter(|item| !matches!(item, ContentElement::BlankLineGroup(_)))
        .collect();

    if children.is_empty() {
        None
    } else {
        Some(children)
    }
}

/// Check if a line is a separator (cosmetic) line.
///
/// Separator lines contain only pipes, dashes, colons, pluses, and spaces.
/// Examples: `|---|---|`, `|:---:|---:|`, `+---+---+`
fn is_separator_line(line: &str) -> bool {
    line.chars()
        .all(|c| matches!(c, '|' | '-' | ':' | '+' | ' ' | '='))
}

/// Extract footnote lines from trailing non-pipe content.
///
/// Footnotes are non-pipe lines that appear after the last pipe row.
/// They follow the pattern `N. text` (numbered list items).
/// Blank lines between the last pipe row and footnotes are skipped.
fn extract_footnote_lines(lines: &[LineKind]) -> Vec<FootnoteLineData> {
    // Find the last pipe row index
    let last_pipe_idx = lines
        .iter()
        .rposition(|l| matches!(l, LineKind::PipeRow { .. }));

    let Some(last_pipe_idx) = last_pipe_idx else {
        return Vec::new();
    };

    let mut footnotes = Vec::new();
    for line in &lines[last_pipe_idx + 1..] {
        if let LineKind::Other { text, line_range } = line {
            if let Some(footnote) = parse_footnote_line(text, line_range) {
                footnotes.push(footnote);
            }
        }
    }
    footnotes
}

/// Try to parse a line as a footnote item (e.g. "1. Some text").
fn parse_footnote_line(text: &str, range: &ByteRange<usize>) -> Option<FootnoteLineData> {
    // Match pattern: digits followed by . or ) then space and text
    let text = text.trim();
    let marker_end = text.find(|c: char| !c.is_ascii_digit())?;
    if marker_end == 0 {
        return None;
    }
    let rest = &text[marker_end..];
    let separator = if rest.starts_with(". ") {
        "."
    } else if rest.starts_with(") ") {
        ")"
    } else {
        return None;
    };
    let marker = format!("{}{}", &text[..marker_end], separator);
    let body = rest[2..].to_string(); // skip separator + space
    Some(FootnoteLineData {
        marker,
        text: body,
        byte_range: range.clone(),
    })
}

/// Resolve merge markers (`>>` for colspan, `^^` for rowspan).
///
/// - `>>` means "this cell is absorbed by the cell to the left" (colspan)
/// - `^^` means "this cell is absorbed by the cell above" (rowspan)
///
/// After resolution, absorbed cells are removed and the spanning cell's
/// colspan/rowspan is incremented.
fn resolve_merges(mut rows: Vec<TableRowData>) -> Vec<TableRowData> {
    // First pass: resolve colspan (>> markers)
    for row in &mut rows {
        let mut i = 0;
        while i < row.cells.len() {
            if row.cells[i].text == ">>" && i > 0 {
                // Find the cell to the left that isn't a merge marker
                let mut target = i - 1;
                while target > 0 && row.cells[target].text == ">>" {
                    target -= 1;
                }
                row.cells[target].colspan += 1;
                row.cells.remove(i);
            } else {
                i += 1;
            }
        }
    }

    // Second pass: resolve rowspan (^^ markers)
    for row_idx in 0..rows.len() {
        let mut col_idx = 0;
        while col_idx < rows[row_idx].cells.len() {
            if rows[row_idx].cells[col_idx].text == "^^" && row_idx > 0 {
                // Find the cell above in the same column position
                if col_idx < rows[row_idx - 1].cells.len() {
                    rows[row_idx - 1].cells[col_idx].rowspan += 1;
                }
                rows[row_idx].cells.remove(col_idx);
            } else {
                col_idx += 1;
            }
        }
    }

    rows
}

/// Split rows into header and body based on header count.
fn split_header_body(
    rows: &mut Vec<TableRowData>,
    header_count: usize,
) -> (Vec<TableRowData>, Vec<TableRowData>) {
    let split_at = header_count.min(rows.len());
    let body_rows = rows.split_off(split_at);
    let mut header_rows = std::mem::take(rows);

    // Mark header cells
    for row in &mut header_rows {
        for cell in &mut row.cells {
            cell.is_header = true;
        }
    }

    (header_rows, body_rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_separator_line() {
        assert!(is_separator_line("|---|---|"));
        assert!(is_separator_line("|:---:|---:|"));
        assert!(is_separator_line("+---+---+"));
        assert!(is_separator_line("| --- | --- |"));
        assert!(!is_separator_line("| hello | world |"));
        assert!(!is_separator_line("| --- | data |"));
    }

    #[test]
    fn test_classify_lines_cell_splitting() {
        let lines = vec![("| a | b | c |".to_string(), 0..13)];
        let classified = classify_lines(&lines);
        if let LineKind::PipeRow { cells, .. } = &classified[0] {
            assert_eq!(cells, &["a", "b", "c"]);
        } else {
            panic!("Expected PipeRow");
        }
    }

    #[test]
    fn test_classify_lines_escaped_pipe_in_cell() {
        // `\|` should not split the cell; the backslash is stripped in the cell text.
        let lines = vec![(r"| a\|b | c |".to_string(), 0..12)];
        let classified = classify_lines(&lines);
        if let LineKind::PipeRow {
            cells, cell_ranges, ..
        } = &classified[0]
        {
            assert_eq!(cells.len(), 2, "expected 2 cells, got {cells:?}");
            assert_eq!(cells[0], "a|b");
            assert_eq!(cells[1], "c");
            // Byte range of cell 0 spans "a\|b" in source (4 bytes), pointing at the
            // trimmed content inside the segment, not the stripped cell text.
            assert_eq!(cell_ranges[0].end - cell_ranges[0].start, 4);
        } else {
            panic!("Expected PipeRow");
        }
    }

    #[test]
    fn test_classify_lines_backtick_protects_pipe() {
        // Pipes inside balanced backticks must not split cells.
        let lines = vec![("| a | `x|y|z` | c |".to_string(), 0..19)];
        let classified = classify_lines(&lines);
        if let LineKind::PipeRow { cells, .. } = &classified[0] {
            assert_eq!(cells.len(), 3, "expected 3 cells, got {cells:?}");
            assert_eq!(cells[0], "a");
            assert_eq!(cells[1], "`x|y|z`");
            assert_eq!(cells[2], "c");
        } else {
            panic!("Expected PipeRow");
        }
    }

    #[test]
    fn test_classify_lines_multiple_escaped_pipes() {
        let lines = vec![(r"| a\|b\|c | d |".to_string(), 0..15)];
        let classified = classify_lines(&lines);
        if let LineKind::PipeRow { cells, .. } = &classified[0] {
            assert_eq!(cells.len(), 2);
            assert_eq!(cells[0], "a|b|c");
            assert_eq!(cells[1], "d");
        } else {
            panic!("Expected PipeRow");
        }
    }

    #[test]
    fn test_classify_lines_double_backslash_then_pipe_splits() {
        // `\\|` = literal backslash + structural pipe (even backslashes → not escaped).
        let lines = vec![(r"| a\\|b |".to_string(), 0..9)];
        let classified = classify_lines(&lines);
        if let LineKind::PipeRow { cells, .. } = &classified[0] {
            assert_eq!(cells.len(), 2, "expected 2 cells, got {cells:?}");
            assert_eq!(cells[0], r"a\\");
            assert_eq!(cells[1], "b");
        } else {
            panic!("Expected PipeRow");
        }
    }

    #[test]
    fn test_classify_lines_escape_inside_backticks_preserved() {
        // Inside backtick literal region, backslashes pass through verbatim —
        // no split AND no stripping.
        let lines = vec![(r"| a | `code\|here` | b |".to_string(), 0..24)];
        let classified = classify_lines(&lines);
        if let LineKind::PipeRow { cells, .. } = &classified[0] {
            assert_eq!(cells.len(), 3);
            assert_eq!(cells[0], "a");
            assert_eq!(cells[1], r"`code\|here`");
            assert_eq!(cells[2], "b");
        } else {
            panic!("Expected PipeRow");
        }
    }

    #[test]
    fn test_classify_lines_no_trailing_pipe() {
        let lines = vec![("| a | b | c".to_string(), 0..11)];
        let classified = classify_lines(&lines);
        if let LineKind::PipeRow { cells, .. } = &classified[0] {
            assert_eq!(cells.len(), 3);
            assert_eq!(cells[2], "c");
        } else {
            panic!("Expected PipeRow");
        }
    }

    #[test]
    fn test_classify_lines_empty_cells() {
        let lines = vec![("| a | | c |".to_string(), 0..11)];
        let classified = classify_lines(&lines);
        if let LineKind::PipeRow { cells, .. } = &classified[0] {
            assert_eq!(cells.len(), 3);
            assert_eq!(cells[1], "");
        } else {
            panic!("Expected PipeRow");
        }
    }

    #[test]
    fn test_resolve_merges_colspan() {
        let rows = vec![TableRowData {
            cells: vec![
                TableCellData {
                    text: "wide".to_string(),
                    byte_range: 0..4,
                    colspan: 1,
                    rowspan: 1,
                    is_header: false,
                    raw_text: None,
                    block_content: None,
                },
                TableCellData {
                    text: ">>".to_string(),
                    byte_range: 0..2,
                    colspan: 1,
                    rowspan: 1,
                    is_header: false,
                    raw_text: None,
                    block_content: None,
                },
                TableCellData {
                    text: "normal".to_string(),
                    byte_range: 0..6,
                    colspan: 1,
                    rowspan: 1,
                    is_header: false,
                    raw_text: None,
                    block_content: None,
                },
            ],
            byte_range: 0..20,
        }];

        let resolved = resolve_merges(rows);
        assert_eq!(resolved[0].cells.len(), 2);
        assert_eq!(resolved[0].cells[0].text, "wide");
        assert_eq!(resolved[0].cells[0].colspan, 2);
        assert_eq!(resolved[0].cells[1].text, "normal");
    }

    #[test]
    fn test_resolve_merges_rowspan() {
        let rows = vec![
            TableRowData {
                cells: vec![
                    TableCellData {
                        text: "tall".to_string(),
                        raw_text: None,
                        byte_range: 0..4,
                        colspan: 1,
                        rowspan: 1,
                        is_header: false,
                        block_content: None,
                    },
                    TableCellData {
                        text: "b".to_string(),
                        raw_text: None,
                        byte_range: 0..1,
                        colspan: 1,
                        rowspan: 1,
                        is_header: false,
                        block_content: None,
                    },
                ],
                byte_range: 0..10,
            },
            TableRowData {
                cells: vec![
                    TableCellData {
                        text: "^^".to_string(),
                        raw_text: None,
                        byte_range: 0..2,
                        colspan: 1,
                        rowspan: 1,
                        is_header: false,
                        block_content: None,
                    },
                    TableCellData {
                        text: "d".to_string(),
                        raw_text: None,
                        byte_range: 0..1,
                        colspan: 1,
                        rowspan: 1,
                        is_header: false,
                        block_content: None,
                    },
                ],
                byte_range: 0..10,
            },
        ];

        let resolved = resolve_merges(rows);
        assert_eq!(resolved[0].cells[0].rowspan, 2);
        assert_eq!(resolved[1].cells.len(), 1);
        assert_eq!(resolved[1].cells[0].text, "d");
    }

    #[test]
    fn test_parse_pipe_rows_skips_separators() {
        let lines = vec![
            ("| a | b |".to_string(), 0..9),
            ("|---|---|".to_string(), 10..18),
            ("| c | d |".to_string(), 19..28),
        ];
        let rows = parse_pipe_rows(&lines);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn test_parse_pipe_rows_blanks_trigger_multiline() {
        // Blank between pipe rows → multi-line mode, but each group has 1 line → 2 rows
        let lines = vec![
            ("| a | b |".to_string(), 0..9),
            ("".to_string(), 10..10),
            ("| c | d |".to_string(), 11..20),
        ];
        let rows = parse_pipe_rows(&lines);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].cells[0].text, "a");
        assert_eq!(rows[1].cells[0].text, "c");
    }

    #[test]
    fn test_detect_multiline_false_for_compact() {
        let lines = vec![
            ("| a | b |".to_string(), 0..9),
            ("| c | d |".to_string(), 10..19),
        ];
        let classified = classify_lines(&lines);
        assert!(!detect_multiline(&classified));
    }

    #[test]
    fn test_detect_multiline_true_with_blanks() {
        let lines = vec![
            ("| a | b |".to_string(), 0..9),
            ("".to_string(), 10..10),
            ("| c | d |".to_string(), 11..20),
        ];
        let classified = classify_lines(&lines);
        assert!(detect_multiline(&classified));
    }

    #[test]
    fn test_merge_group_single_line() {
        let lines = vec![("| x | y |".to_string(), 0..9)];
        let classified = classify_lines(&lines);
        let refs: Vec<&LineKind> = classified.iter().collect();
        let row = merge_group(&refs).unwrap();
        assert_eq!(row.cells.len(), 2);
        assert_eq!(row.cells[0].text, "x");
        assert_eq!(row.cells[1].text, "y");
    }

    #[test]
    fn test_merge_group_continuation() {
        let lines = vec![
            ("| hello | world |".to_string(), 0..17),
            ("|       | again |".to_string(), 18..35),
        ];
        let classified = classify_lines(&lines);
        let refs: Vec<&LineKind> = classified.iter().collect();
        let row = merge_group(&refs).unwrap();
        assert_eq!(row.cells.len(), 2);
        assert_eq!(row.cells[0].text, "hello");
        assert_eq!(row.cells[1].text, "world\nagain");
    }

    #[test]
    fn test_merge_group_whitespace_only_continuation_ignored() {
        let lines = vec![
            ("| base | val |".to_string(), 0..14),
            ("|      |     |".to_string(), 15..29),
        ];
        let classified = classify_lines(&lines);
        let refs: Vec<&LineKind> = classified.iter().collect();
        let row = merge_group(&refs).unwrap();
        assert_eq!(row.cells[0].text, "base");
        assert_eq!(row.cells[1].text, "val");
    }

    #[test]
    fn test_multiline_rows_grouping() {
        let lines = vec![
            ("| H1 | H2 |".to_string(), 0..11),
            ("".to_string(), 12..12),
            ("| a  | line1 |".to_string(), 13..27),
            ("|    | line2 |".to_string(), 28..42),
            ("".to_string(), 43..43),
            ("| b  | single |".to_string(), 44..59),
        ];
        let rows = parse_pipe_rows(&lines);
        assert_eq!(rows.len(), 3); // header group, row group 1, row group 2
        assert_eq!(rows[0].cells[0].text, "H1");
        assert_eq!(rows[1].cells[0].text, "a");
        assert_eq!(rows[1].cells[1].text, "line1\nline2");
        assert_eq!(rows[2].cells[0].text, "b");
        assert_eq!(rows[2].cells[1].text, "single");
    }

    #[test]
    fn test_parse_footnote_line_numbered_period() {
        let f = parse_footnote_line("1. Some text here", &(0..17)).unwrap();
        assert_eq!(f.marker, "1.");
        assert_eq!(f.text, "Some text here");
    }

    #[test]
    fn test_parse_footnote_line_numbered_paren() {
        let f = parse_footnote_line("2) Another note", &(0..15)).unwrap();
        assert_eq!(f.marker, "2)");
        assert_eq!(f.text, "Another note");
    }

    #[test]
    fn test_parse_footnote_line_not_a_footnote() {
        assert!(parse_footnote_line("Just plain text", &(0..15)).is_none());
        assert!(parse_footnote_line("- a dash item", &(0..13)).is_none());
    }

    #[test]
    fn test_extract_footnote_lines_from_classified() {
        let lines = vec![
            ("| a | b |".to_string(), 0..9),
            ("| c | d |".to_string(), 10..19),
            ("".to_string(), 20..20),
            ("1. First note".to_string(), 21..34),
            ("2. Second note".to_string(), 35..49),
        ];
        let classified = classify_lines(&lines);
        let footnotes = extract_footnote_lines(&classified);
        assert_eq!(footnotes.len(), 2);
        assert_eq!(footnotes[0].marker, "1.");
        assert_eq!(footnotes[0].text, "First note");
        assert_eq!(footnotes[1].marker, "2.");
        assert_eq!(footnotes[1].text, "Second note");
    }

    #[test]
    fn test_extract_footnote_lines_none_when_no_trailing() {
        let lines = vec![
            ("| a | b |".to_string(), 0..9),
            ("| c | d |".to_string(), 10..19),
        ];
        let classified = classify_lines(&lines);
        let footnotes = extract_footnote_lines(&classified);
        assert!(footnotes.is_empty());
    }

    // Alignment extraction test moved to apply_table_config assembly stage
}
