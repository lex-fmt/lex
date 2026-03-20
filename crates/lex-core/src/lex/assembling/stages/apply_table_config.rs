//! Table configuration stage
//!
//! After annotation attachment, this stage processes tables to apply configuration
//! from their :: table :: annotations. It handles:
//! - `header=N`: Re-splits rows into header/body based on the header count
//! - `align=lcr`: Applies column alignment to all cells
//!
//! This runs after AttachAnnotations so that both internal annotations (inside the
//! table block) and external annotations (attached by proximity rules) are available.

use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::{Document, Table, TableCellAlignment};
use crate::lex::transforms::{Runnable, TransformError};

/// Apply configuration from :: table :: annotations to table elements.
pub struct ApplyTableConfig;

impl ApplyTableConfig {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ApplyTableConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Runnable<Document, Document> for ApplyTableConfig {
    fn run(&self, mut input: Document) -> Result<Document, TransformError> {
        apply_config_in_children(input.root.children.as_mut_vec());
        Ok(input)
    }
}

fn apply_config_in_children(children: &mut [ContentItem]) {
    for item in children.iter_mut() {
        if let ContentItem::Table(table) = item {
            apply_table_config(table);
        }
        if let Some(nested) = item.children_mut() {
            apply_config_in_children(nested);
        }
    }
}

/// Apply configuration from :: table :: annotations to a single table.
fn apply_table_config(table: &mut Table) {
    // Find the first annotation with label "table"
    let config = table
        .annotations
        .iter()
        .find(|a| a.data.label.value == "table");

    let config = match config {
        Some(c) => c,
        None => return, // No config annotation — keep defaults
    };

    // Extract header count
    let header_count = config
        .data
        .parameters
        .iter()
        .find(|p| p.key == "header")
        .and_then(|p| p.value.parse::<usize>().ok());

    // Extract alignment
    let alignments = config
        .data
        .parameters
        .iter()
        .find(|p| p.key == "align")
        .map(|p| parse_alignment_string(&p.value));

    // Apply header count: re-split rows if needed
    if let Some(count) = header_count {
        resplit_header_body(table, count);
    }

    // Apply alignment to all cells
    if let Some(aligns) = alignments {
        apply_alignments(table, &aligns);
    }
}

/// Re-split a table's rows into header and body based on the given header count.
fn resplit_header_body(table: &mut Table, header_count: usize) {
    // Merge all rows back together
    let mut all_rows = std::mem::take(&mut table.header_rows);
    all_rows.append(&mut table.body_rows);

    // Unmark all cells as non-header first
    for row in &mut all_rows {
        for cell in &mut row.cells {
            cell.header = false;
        }
    }

    // Split at the requested count
    let split_at = header_count.min(all_rows.len());
    let body_rows = all_rows.split_off(split_at);
    let mut header_rows = all_rows;

    // Mark header cells
    for row in &mut header_rows {
        for cell in &mut row.cells {
            cell.header = true;
        }
    }

    table.header_rows = header_rows;
    table.body_rows = body_rows;
}

/// Apply column alignments to all cells in the table.
fn apply_alignments(table: &mut Table, alignments: &[TableCellAlignment]) {
    for row in table.header_rows.iter_mut().chain(table.body_rows.iter_mut()) {
        for (col_idx, cell) in row.cells.iter_mut().enumerate() {
            if let Some(align) = alignments.get(col_idx) {
                cell.align = *align;
            }
        }
    }
}

/// Parse an alignment string like "lcr" into a vector of TableCellAlignment.
fn parse_alignment_string(s: &str) -> Vec<TableCellAlignment> {
    s.chars()
        .map(|c| match c {
            'l' => TableCellAlignment::Left,
            'c' => TableCellAlignment::Center,
            'r' => TableCellAlignment::Right,
            _ => TableCellAlignment::None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_alignment_string() {
        let aligns = parse_alignment_string("lcr");
        assert_eq!(
            aligns,
            vec![
                TableCellAlignment::Left,
                TableCellAlignment::Center,
                TableCellAlignment::Right
            ]
        );
    }

    #[test]
    fn test_parse_alignment_string_empty() {
        let aligns = parse_alignment_string("");
        assert!(aligns.is_empty());
    }
}
