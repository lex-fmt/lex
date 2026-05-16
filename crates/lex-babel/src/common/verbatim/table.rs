use crate::ir::nodes::{DocNode, InlineContent, Table, TableCell, TableCellAlignment};

pub(crate) fn serialize_pipe_table(table: &Table) -> String {
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
                InlineContent::Reference { raw, .. } => format!("[{raw}]"),
                InlineContent::Link { text, href } => format!("{text} [{href}]"),
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
            InlineContent::Reference { raw, .. } => format!("[{raw}]"),
            InlineContent::Link { text, href } => format!("{text} [{href}]"),
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
