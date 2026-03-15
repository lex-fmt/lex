use lex_core::lex::ast::{ContentItem, Document, Position, Session, Table};

use super::formatting::TextEditSpan;

/// Format the table at the given cursor position, aligning columns.
///
/// Returns a single TextEditSpan replacing the pipe-row region of the table,
/// or `None` if no table is found at that position.
pub fn format_table_at(
    document: &Document,
    source: &str,
    position: Position,
) -> Option<TextEditSpan> {
    let table = find_table_at(document, position)?;
    format_table(table, source)
}

/// Format all tables in a document, returning edits in reverse order
/// (so they can be applied sequentially without shifting offsets).
pub fn format_all_tables(document: &Document, source: &str) -> Vec<TextEditSpan> {
    let tables = collect_tables(document);
    let mut edits: Vec<TextEditSpan> = tables
        .iter()
        .filter_map(|table| format_table(table, source))
        .collect();
    // Reverse order so later edits are applied first (no offset shifting needed)
    edits.sort_by(|a, b| b.start.cmp(&a.start));
    edits
}

/// Format a single table's pipe rows to have aligned columns.
fn format_table(table: &Table, source: &str) -> Option<TextEditSpan> {
    // Determine the byte range of the pipe-row region (from first row to last row)
    let all_rows: Vec<_> = table.all_rows().collect();
    if all_rows.is_empty() {
        return None;
    }

    let first_row = all_rows.first()?;
    let last_row = all_rows.last()?;
    let raw_start = first_row.location.span.start;
    let raw_end = last_row.location.span.end;

    if raw_start >= raw_end || raw_end > source.len() {
        return None;
    }

    // Expand to full lines: find line start before first row and line end after last row
    let region_start = source[..raw_start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let region_end = source[raw_end..]
        .find('\n')
        .map(|i| raw_end + i + 1)
        .unwrap_or(raw_end);

    // Extract source lines from the pipe region
    let region_text = &source[region_start..region_end];
    let lines: Vec<&str> = region_text.lines().collect();

    // Determine the indentation from the first line
    let indent = lines
        .first()
        .map(|line| {
            let trimmed = line.trim_start();
            &line[..line.len() - trimmed.len()]
        })
        .unwrap_or("");

    // Parse each line into cells or separator
    let mut parsed_lines: Vec<ParsedLine> = Vec::new();
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            parsed_lines.push(ParsedLine::Blank);
        } else if is_separator(trimmed) {
            parsed_lines.push(ParsedLine::Separator);
        } else if trimmed.starts_with('|') {
            parsed_lines.push(ParsedLine::Row(parse_cells(trimmed)));
        } else {
            // Footnote or other non-pipe line — preserve as-is
            parsed_lines.push(ParsedLine::Other(line.to_string()));
        }
    }

    // Compute column widths across all data rows
    let col_count = parsed_lines
        .iter()
        .filter_map(|l| match l {
            ParsedLine::Row(cells) => Some(cells.len()),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    if col_count == 0 {
        return None;
    }

    let mut col_widths = vec![1usize; col_count];
    for line in &parsed_lines {
        if let ParsedLine::Row(cells) = line {
            for (i, cell) in cells.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }
    }

    // Re-render lines with aligned columns
    let mut formatted = String::new();
    for (i, line) in parsed_lines.iter().enumerate() {
        if i > 0 {
            formatted.push('\n');
        }
        match line {
            ParsedLine::Row(cells) => {
                formatted.push_str(indent);
                formatted.push('|');
                for (j, cell) in cells.iter().enumerate() {
                    let width = col_widths.get(j).copied().unwrap_or(cell.len());
                    formatted.push(' ');
                    formatted.push_str(&format!("{cell:width$}"));
                    formatted.push_str(" |");
                }
                // Pad missing columns
                for j in cells.len()..col_count {
                    let width = col_widths.get(j).copied().unwrap_or(1);
                    formatted.push(' ');
                    formatted.push_str(&" ".repeat(width));
                    formatted.push_str(" |");
                }
            }
            ParsedLine::Separator => {
                formatted.push_str(indent);
                formatted.push('|');
                for width in &col_widths {
                    formatted.push_str(&format!("-{}-|", "-".repeat(*width)));
                }
            }
            ParsedLine::Blank => {
                // Preserve blank lines
            }
            ParsedLine::Other(text) => {
                formatted.push_str(text);
            }
        }
    }
    // Ensure trailing newline to match source convention
    formatted.push('\n');

    let formatted_region = formatted;

    // Check if there's actually a change
    let original_region = &source[region_start..region_end];
    if formatted_region.trim_end() == original_region.trim_end() {
        return None;
    }

    Some(TextEditSpan {
        start: region_start,
        end: region_end,
        new_text: formatted_region,
    })
}

#[derive(Debug)]
enum ParsedLine {
    Row(Vec<String>),
    Separator,
    Blank,
    Other(String),
}

fn parse_cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(|s| s.trim().to_string()).collect()
}

fn is_separator(line: &str) -> bool {
    line.starts_with('|')
        && line
            .chars()
            .all(|c| matches!(c, '|' | '-' | ':' | '+' | ' ' | '='))
}

fn find_table_at(document: &Document, position: Position) -> Option<&Table> {
    find_table_in_session(&document.root, position)
}

fn find_table_in_session(session: &Session, position: Position) -> Option<&Table> {
    for child in session.children.iter() {
        if let Some(table) = find_table_in_content(child, position) {
            return Some(table);
        }
    }
    None
}

fn find_table_in_content(item: &ContentItem, position: Position) -> Option<&Table> {
    match item {
        ContentItem::Table(table) => {
            if table.location.contains(position) {
                return Some(table);
            }
            None
        }
        ContentItem::Session(session) => find_table_in_session(session, position),
        ContentItem::Definition(def) => {
            for child in def.children.iter() {
                if let Some(t) = find_table_in_content(child, position) {
                    return Some(t);
                }
            }
            None
        }
        ContentItem::List(list) => {
            for entry in &list.items {
                if let ContentItem::ListItem(li) = entry {
                    for child in li.children.iter() {
                        if let Some(t) = find_table_in_content(child, position) {
                            return Some(t);
                        }
                    }
                }
            }
            None
        }
        ContentItem::Annotation(ann) => {
            for child in ann.children.iter() {
                if let Some(t) = find_table_in_content(child, position) {
                    return Some(t);
                }
            }
            None
        }
        _ => None,
    }
}

fn collect_tables(document: &Document) -> Vec<&Table> {
    let mut tables = Vec::new();
    collect_tables_in_session(&document.root, &mut tables);
    tables
}

fn collect_tables_in_session<'a>(session: &'a Session, out: &mut Vec<&'a Table>) {
    for child in session.children.iter() {
        collect_tables_in_content(child, out);
    }
}

fn collect_tables_in_content<'a>(item: &'a ContentItem, out: &mut Vec<&'a Table>) {
    match item {
        ContentItem::Table(table) => out.push(table),
        ContentItem::Session(session) => collect_tables_in_session(session, out),
        ContentItem::Definition(def) => {
            for child in def.children.iter() {
                collect_tables_in_content(child, out);
            }
        }
        ContentItem::List(list) => {
            for entry in &list.items {
                if let ContentItem::ListItem(li) = entry {
                    for child in li.children.iter() {
                        collect_tables_in_content(child, out);
                    }
                }
            }
        }
        ContentItem::Annotation(ann) => {
            for child in ann.children.iter() {
                collect_tables_in_content(child, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::parsing;

    fn parse(source: &str) -> Document {
        parsing::parse_document(source).expect("parse failed")
    }

    #[test]
    fn formats_unaligned_table() {
        let source = "Data:\n    | Name | Score |\n    | Alice | 95 |\n:: table ::\n";
        let doc = parse(source);
        let edits = format_all_tables(&doc, source);
        assert_eq!(edits.len(), 1);
        let mut result = source.to_string();
        for edit in &edits {
            result.replace_range(edit.start..edit.end, &edit.new_text);
        }
        // All rows should have same-width columns
        assert!(result.contains("| Name  | Score |"));
        assert!(result.contains("| Alice | 95    |"));
    }

    #[test]
    fn no_edit_when_already_aligned() {
        let source = "Data:\n    | A | B |\n    | 1 | 2 |\n:: table ::\n";
        let doc = parse(source);
        let edits = format_all_tables(&doc, source);
        assert!(edits.is_empty());
    }

    #[test]
    fn formats_table_at_cursor_position() {
        let source = "Data:\n    | Name | Score |\n    | Alice | 95 |\n:: table ::\n";
        let doc = parse(source);
        // Position inside the table (line 2, column 8 — inside the pipe row area)
        let pos = Position::new(1, 8);
        let edit = format_table_at(&doc, source, pos);
        assert!(edit.is_some());
    }

    #[test]
    fn preserves_separator_lines() {
        let source =
            "Data:\n    | Name | Score |\n    |---|---|\n    | Alice | 95 |\n:: table ::\n";
        let doc = parse(source);
        let edits = format_all_tables(&doc, source);
        if !edits.is_empty() {
            let mut result = source.to_string();
            for edit in &edits {
                result.replace_range(edit.start..edit.end, &edit.new_text);
            }
            // Separator should still be present
            assert!(result.contains("|---"));
        }
    }

    #[test]
    fn formats_table_with_merge_markers() {
        let source = "Data:\n    | Q1 | >> | Q2 |\n    | A | B | C |\n:: table ::\n";
        let doc = parse(source);
        let edits = format_all_tables(&doc, source);
        if !edits.is_empty() {
            let mut result = source.to_string();
            for edit in &edits {
                result.replace_range(edit.start..edit.end, &edit.new_text);
            }
            // Merge marker should be preserved
            assert!(result.contains(">>"));
        }
    }
}
