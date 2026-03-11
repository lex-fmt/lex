use super::VerbatimHandler;
use crate::ir::nodes::{
    DocNode, InlineContent, Paragraph, Table, TableCell, TableCellAlignment, TableRow,
};
use lex_core::lex::ast::Verbatim;
use std::collections::HashMap;

/// Handler for `doc.table` verbatim blocks.
///
/// Parses markdown-style pipe tables into `DocNode::Table` and serializes them back.
pub struct TableHandler;

impl VerbatimHandler for TableHandler {
    fn label(&self) -> &str {
        "doc.table"
    }

    fn to_ir(&self, content: &str, _params: &HashMap<String, String>) -> Option<DocNode> {
        Some(parse_pipe_table(content))
    }

    fn convert_from_ir(&self, node: &DocNode) -> Option<(String, HashMap<String, String>)> {
        if let DocNode::Table(table) = node {
            Some((serialize_pipe_table(table), HashMap::new()))
        } else {
            None
        }
    }

    fn format_content(
        &self,
        verbatim: &Verbatim,
    ) -> Result<Option<String>, crate::error::FormatError> {
        // Reconstruct content from lines
        let mut content = String::new();
        for item in &verbatim.children {
            if let lex_core::lex::ast::ContentItem::VerbatimLine(line) = item {
                content.push_str(line.content.as_string());
                content.push('\n');
            }
        }

        // Dedent content to ensure markdown parser sees a table, not a code block
        let lines: Vec<&str> = content.lines().collect();
        let min_indent = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);

        let dedented_content = lines
            .iter()
            .map(|line| {
                if line.len() >= min_indent {
                    &line[min_indent..]
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Parse the dedented content as markdown
        let doc = match crate::formats::markdown::parser::parse_from_markdown(&dedented_content) {
            Ok(d) => d,
            Err(e) => {
                // We return Ok(None) instead of Err because formatting failure shouldn't break serialization
                // It just means we can't format it nicely
                println!("TableHandler: Markdown parse failed: {e:?}");
                return Ok(None);
            }
        };

        // The markdown parser should have identified a table
        // It converts markdown tables to Lex VerbatimBlock with "table" label (or similar)
        // with the content already formatted (by serialize_pipe_table during conversion).
        // We just need to extract it.
        for child in &doc.root.children {
            if let lex_core::lex::ast::ContentItem::VerbatimBlock(verbatim) = child {
                // Reconstruct content from lines
                let mut formatted = String::new();
                for item in &verbatim.children {
                    if let lex_core::lex::ast::ContentItem::VerbatimLine(line) = item {
                        formatted.push_str(line.content.as_string());
                        formatted.push('\n');
                    }
                }
                return Ok(Some(formatted));
            }
        }
        Ok(None)
    }
}

fn parse_pipe_table(content: &str) -> DocNode {
    let mut header = Vec::new();
    let mut rows = Vec::new();
    let mut alignments = Vec::new();

    let lines: Vec<&str> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return DocNode::Table(Table {
            rows,
            header,
            caption: None,
        });
    }

    // Parse header
    if let Some(header_line) = lines.first() {
        let cells = parse_table_row(header_line);
        let mut header_row = TableRow { cells: Vec::new() };
        for cell_content in cells {
            header_row.cells.push(TableCell {
                content: vec![DocNode::Paragraph(Paragraph {
                    content: vec![InlineContent::Text(cell_content)],
                })],
                header: true,
                align: TableCellAlignment::None,
            });
        }
        header.push(header_row);
    }

    // Parse separator line to determine alignments
    if lines.len() > 1 {
        let separator = lines[1];
        if separator.contains(['-', '|']) {
            let parts = parse_table_row(separator);
            for part in parts {
                let trimmed = part.trim();
                if trimmed.starts_with(':') && trimmed.ends_with(':') {
                    alignments.push(TableCellAlignment::Center);
                } else if trimmed.ends_with(':') {
                    alignments.push(TableCellAlignment::Right);
                } else if trimmed.starts_with(':') {
                    alignments.push(TableCellAlignment::Left);
                } else {
                    alignments.push(TableCellAlignment::None);
                }
            }
        }
    }

    // Parse body rows
    for line in lines.iter().skip(2) {
        let cells = parse_table_row(line);
        let mut row = TableRow { cells: Vec::new() };
        for (i, cell_content) in cells.into_iter().enumerate() {
            let align = if i < alignments.len() {
                alignments[i]
            } else {
                TableCellAlignment::None
            };

            row.cells.push(TableCell {
                content: vec![DocNode::Paragraph(Paragraph {
                    content: vec![InlineContent::Text(cell_content)],
                })],
                header: false,
                align,
            });
        }
        rows.push(row);
    }

    // Apply alignments to header
    if !header.is_empty() {
        for (i, cell) in header[0].cells.iter_mut().enumerate() {
            if i < alignments.len() {
                cell.align = alignments[i];
            }
        }
    }

    DocNode::Table(Table {
        rows,
        header,
        caption: None,
    })
}

fn parse_table_row(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);

    line.split('|').map(|s| s.trim().to_string()).collect()
}

fn serialize_pipe_table(table: &Table) -> String {
    let mut output = String::new();

    // 1. Calculate column widths
    let mut col_widths = Vec::new();

    // Check header
    for row in &table.header {
        for (i, cell) in row.cells.iter().enumerate() {
            let width = cell_text_width(cell);
            if i >= col_widths.len() {
                col_widths.push(width);
            } else {
                col_widths[i] = col_widths[i].max(width);
            }
        }
    }

    // Check body
    for row in &table.rows {
        for (i, cell) in row.cells.iter().enumerate() {
            let width = cell_text_width(cell);
            if i >= col_widths.len() {
                col_widths.push(width);
            } else {
                col_widths[i] = col_widths[i].max(width);
            }
        }
    }

    // Ensure minimum width of 3 for alignment markers
    for width in &mut col_widths {
        *width = (*width).max(3);
    }

    // 2. Serialize Header
    for row in &table.header {
        output.push('|');
        for (i, cell) in row.cells.iter().enumerate() {
            let text = cell_text(cell);
            let width = col_widths.get(i).copied().unwrap_or(text.len());
            output.push_str(&format!(" {text:width$} |"));
        }
        output.push('\n');
    }

    // 3. Serialize Separator
    if !col_widths.is_empty() {
        output.push('|');
        for (i, width) in col_widths.iter().enumerate() {
            let align = table
                .header
                .first()
                .and_then(|row| row.cells.get(i))
                .map(|c| c.align)
                .unwrap_or(TableCellAlignment::None);

            let dashes = "-".repeat(width.saturating_sub(2));
            match align {
                TableCellAlignment::Left => output.push_str(&format!(" :{dashes}- |")),
                TableCellAlignment::Right => output.push_str(&format!(" -{dashes}: |")),
                TableCellAlignment::Center => output.push_str(&format!(" :{dashes}: |")),
                TableCellAlignment::None => output.push_str(&format!(" -{dashes}- |")),
            }
        }
        output.push('\n');
    }

    // 4. Serialize Body
    for row in &table.rows {
        output.push('|');
        for (i, cell) in row.cells.iter().enumerate() {
            let text = cell_text(cell);
            let width = col_widths.get(i).copied().unwrap_or(text.len());
            output.push_str(&format!(" {text:width$} |"));
        }
        output.push('\n');
    }

    output
}

fn cell_text(cell: &TableCell) -> String {
    // Simple extraction for now, similar to existing logic
    if let Some(DocNode::Paragraph(p)) = cell.content.first() {
        p.content
            .iter()
            .map(|ic| match ic {
                InlineContent::Text(t) => t.clone(),
                InlineContent::Bold(c) => format!("*{}*", inline_content_to_text(c)),
                InlineContent::Italic(c) => format!("_{}_", inline_content_to_text(c)),
                InlineContent::Code(c) => format!("`{c}`"),
                InlineContent::Math(c) => format!("${c}$"),
                InlineContent::Reference(c) => format!("[{c}]"),
                InlineContent::Marker(c) => c.clone(),
                InlineContent::Image(image) => {
                    let mut text = format!("![{}]({})", image.alt, image.src);
                    if let Some(title) = &image.title {
                        text.push_str(&format!(" \"{title}\""));
                    }
                    text
                }
            })
            .collect()
    } else {
        String::new()
    }
}

fn cell_text_width(cell: &TableCell) -> usize {
    cell_text(cell).len()
}

fn inline_content_to_text(content: &[InlineContent]) -> String {
    content
        .iter()
        .map(|ic| match ic {
            InlineContent::Text(t) => t.clone(),
            InlineContent::Bold(c) => format!("*{}*", inline_content_to_text(c)),
            InlineContent::Italic(c) => format!("_{}_", inline_content_to_text(c)),
            InlineContent::Code(c) => format!("`{c}`"),
            InlineContent::Math(c) => format!("${c}$"),
            InlineContent::Reference(c) => format!("[{c}]"),
            InlineContent::Marker(c) => c.clone(),
            InlineContent::Image(image) => {
                let mut text = format!("![{}]({})", image.alt, image.src);
                if let Some(title) = &image.title {
                    text.push_str(&format!(" \"{title}\""));
                }
                text
            }
        })
        .collect()
}
