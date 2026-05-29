use super::formatting_rules::FormattingRules;
use lex_core::lex::ast::{
    elements::{
        blank_line_group::BlankLineGroup, paragraph::TextLine, sequence_marker::Form,
        verbatim::VerbatimGroupItemRef, VerbatimLine,
    },
    traits::{AstNode, Visitor},
    Annotation, Definition, Document, List, ListItem, Paragraph, Session, Table,
    TableCellAlignment, TableRow, Verbatim,
};

use lex_core::lex::assembling::stages::normalize_labels::source_spelling;
use lex_core::lex::ast::elements::sequence_marker::DecorationStyle;

struct ListContext {
    index: usize,
    style: DecorationStyle,
    upper_case: bool,
    marker_form: Option<Form>,
}

fn format_marker_index(style: DecorationStyle, upper_case: bool, index: usize) -> String {
    match style {
        DecorationStyle::Plain => "-".to_string(),
        DecorationStyle::Numerical => index.to_string(),
        DecorationStyle::Alphabetical => {
            if upper_case {
                to_alpha_upper(index)
            } else {
                to_alpha_lower(index)
            }
        }
        DecorationStyle::Roman => to_roman_upper(index),
    }
}

fn to_alpha_lower(n: usize) -> String {
    if (1..=26).contains(&n) {
        char::from_u32((n as u32) + 96).unwrap().to_string()
    } else {
        n.to_string()
    }
}
fn to_alpha_upper(n: usize) -> String {
    if (1..=26).contains(&n) {
        char::from_u32((n as u32) + 64).unwrap().to_string()
    } else {
        n.to_string()
    }
}

fn to_roman_upper(n: usize) -> String {
    // Convert to Roman numerals (uppercase) for common values
    // Falls back to decimal for values > 20
    match n {
        1 => "I".to_string(),
        2 => "II".to_string(),
        3 => "III".to_string(),
        4 => "IV".to_string(),
        5 => "V".to_string(),
        6 => "VI".to_string(),
        7 => "VII".to_string(),
        8 => "VIII".to_string(),
        9 => "IX".to_string(),
        10 => "X".to_string(),
        11 => "XI".to_string(),
        12 => "XII".to_string(),
        13 => "XIII".to_string(),
        14 => "XIV".to_string(),
        15 => "XV".to_string(),
        16 => "XVI".to_string(),
        17 => "XVII".to_string(),
        18 => "XVIII".to_string(),
        19 => "XIX".to_string(),
        20 => "XX".to_string(),
        _ => n.to_string(), // Fallback to decimal for larger numbers
    }
}

pub struct LexSerializer {
    rules: FormattingRules,
    output: String,
    indent_level: usize,
    consecutive_newlines: usize,
    list_stack: Vec<ListContext>,
}

impl LexSerializer {
    pub fn new(rules: FormattingRules) -> Self {
        Self {
            rules,
            output: String::new(),
            indent_level: 0,
            consecutive_newlines: 2, // Start as if we have blank lines
            list_stack: Vec::new(),
        }
    }

    pub fn serialize(mut self, doc: &Document) -> Result<String, String> {
        // Output document title if present
        if let Some(title) = &doc.title {
            if title.subtitle.is_some() {
                // Title with subtitle: "Title:\nSubtitle\n"
                self.output.push_str(title.as_str());
                self.output.push_str(":\n");
            } else {
                self.output.push_str(title.as_str());
                self.output.push('\n');
            }
            if let Some(subtitle) = title.subtitle_str() {
                self.output.push_str(subtitle);
                self.output.push('\n');
            }
            self.consecutive_newlines = 1;
            // A blank line must separate the title from the body; otherwise the
            // first body line is absorbed into the title on reparse (lex#687).
            // Skip when there is no body (avoids a stray trailing blank).
            if !doc.root.children.is_empty() {
                self.ensure_blank_lines(1);
            }
        }
        doc.root.accept(&mut self);

        // Normalize trailing blank lines to a single newline unless configured
        // to preserve them. Block elements (annotations, verbatim) can leave a
        // structural trailing blank when they are the last node; trimming here
        // keeps the document tail idempotent.
        if !self.rules.preserve_trailing_blanks {
            let end = self.output.trim_end_matches('\n').len();
            self.output.truncate(end);
            if !self.output.is_empty() {
                self.output.push('\n');
            }
        }
        Ok(self.output)
    }

    fn indent(&self) -> String {
        self.rules.indent_string.repeat(self.indent_level)
    }

    fn write_line(&mut self, text: &str) {
        self.output.push_str(&self.indent());
        self.output.push_str(text);
        self.output.push('\n');
        self.consecutive_newlines = 1;
    }

    /// Build an extended marker from the full list stack hierarchy.
    /// Each level contributes its index formatted according to its marker type.
    /// Ancestor levels have already incremented their index, so use `index - 1`.
    /// The current (last) level has not yet incremented, so use `index` as-is.
    fn build_extended_marker(&self) -> String {
        let mut parts = Vec::new();
        let len = self.list_stack.len();
        for (i, ctx) in self.list_stack.iter().enumerate() {
            let idx = if i < len - 1 {
                // Ancestor: already incremented past current item
                ctx.index - 1
            } else {
                // Current level: not yet incremented
                ctx.index
            };
            parts.push(format_marker_index(ctx.style, ctx.upper_case, idx));
        }
        format!("{}.", parts.join("."))
    }

    /// Whether the most recently written line ended with a single `:` —
    /// i.e. a `Subject:`-style container opener (Definition subject,
    /// verbatim group subject, etc.). Used to decide whether to emit a
    /// blank line before a following verbatim subject: a blank line at
    /// column 0 between a Definition subject and its body would
    /// terminate the Definition, so the body's first verbatim must
    /// follow immediately. Annotation headers (`:: label ::` / `:: label`)
    /// end with `::` not a lone `:`, so this check correctly leaves them
    /// out — a verbatim after an annotation does want a leading blank.
    fn last_emission_ended_with_container_opener_colon(&self) -> bool {
        if self.consecutive_newlines != 1 {
            return false;
        }
        let trimmed = self.output.trim_end();
        trimmed.ends_with(':') && !trimmed.ends_with("::")
    }

    fn ensure_blank_lines(&mut self, count: usize) {
        let target_newlines = count + 1;
        while self.consecutive_newlines < target_newlines {
            self.output.push('\n');
            self.consecutive_newlines += 1;
        }
    }
}

impl Visitor for LexSerializer {
    fn visit_session(&mut self, session: &Session) {
        let title = session.title.as_string();
        if !title.is_empty() {
            self.ensure_blank_lines(self.rules.session_blank_lines_before);
            self.write_line(title);
            self.ensure_blank_lines(self.rules.session_blank_lines_after);
            self.indent_level += 1;
        }
    }

    fn leave_session(&mut self, session: &Session) {
        if !session.title.as_string().is_empty() {
            self.indent_level -= 1;
        }
    }

    fn visit_paragraph(&mut self, _paragraph: &Paragraph) {
        // Paragraphs are handled by visiting TextLines
        // TODO: Investigate why some paragraphs are skipped during traversal when indentation is mixed.
        // See: https://github.com/lex-project/lex/issues/new?title=Parser+drops+paragraphs+with+mixed+indentation
    }

    fn visit_text_line(&mut self, text_line: &TextLine) {
        let text = text_line.text().trim_end();
        self.write_line(text);
    }

    fn visit_blank_line_group(&mut self, group: &BlankLineGroup) {
        if group.count == 0 {
            return;
        }

        let count = if self.rules.max_blank_lines > 0 {
            std::cmp::min(group.count, self.rules.max_blank_lines)
        } else {
            group.count
        };
        self.ensure_blank_lines(count);
    }

    fn visit_list(&mut self, list: &List) {
        let (style, upper_case) = if let Some(marker) = &list.marker {
            let upper = marker.style == DecorationStyle::Alphabetical
                && marker
                    .as_str()
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_uppercase());
            (marker.style, upper)
        } else {
            (DecorationStyle::Plain, false)
        };

        let marker_form = list.marker.as_ref().map(|marker| marker.form);

        self.list_stack.push(ListContext {
            style,
            upper_case,
            marker_form,
            index: 1,
        });
    }

    fn leave_list(&mut self, _list: &List) {
        self.list_stack.pop();
    }

    fn visit_list_item(&mut self, list_item: &ListItem) {
        let is_extended = self
            .list_stack
            .iter()
            .any(|ctx| matches!(ctx.marker_form, Some(Form::Extended)));

        let marker = if self.rules.normalize_seq_markers {
            if is_extended {
                // Build hierarchical prefix from the full list stack
                self.build_extended_marker()
            } else {
                let context = self
                    .list_stack
                    .last()
                    .expect("List stack empty in list item");
                if context.style == DecorationStyle::Plain {
                    self.rules.unordered_seq_marker.to_string()
                } else {
                    format!(
                        "{}.",
                        format_marker_index(context.style, context.upper_case, context.index)
                    )
                }
            }
        } else {
            list_item.marker.as_string().to_string()
        };

        let context = self
            .list_stack
            .last_mut()
            .expect("List stack empty in list item");
        context.index += 1;

        // Use the first text content as the item line
        let text = if !list_item.text.is_empty() {
            list_item.text[0].as_string().trim_end()
        } else {
            ""
        };

        let line = if text.is_empty() {
            marker
        } else {
            format!("{marker} {text}")
        };

        self.write_line(&line);
        self.indent_level += 1;
    }

    fn leave_list_item(&mut self, _list_item: &ListItem) {
        self.indent_level -= 1;
    }

    fn visit_definition(&mut self, definition: &Definition) {
        let subject = definition.subject.as_string();
        self.write_line(&format!("{subject}:"));
        self.indent_level += 1;
    }

    fn leave_definition(&mut self, _definition: &Definition) {
        self.indent_level -= 1;
    }

    fn visit_annotation(&mut self, annotation: &Annotation) {
        let label = source_spelling(&annotation.data.label);
        let params = &annotation.data.parameters;

        let mut header = format!(":: {label}");
        if !params.is_empty() {
            for param in params {
                header.push(' ');
                header.push_str(&param.key);
                header.push('=');
                header.push_str(&param.value);
            }
        }

        // Always close the header with ` ::`. The open form (`:: label`) is not
        // valid annotation syntax — the parser drops it — so a block annotation
        // must be `:: label ::` followed by its indented body to round-trip
        // (lex#682).
        header.push_str(" ::");

        self.write_line(&header);

        if !annotation.children.is_empty() {
            self.indent_level += 1;
        }
    }

    fn leave_annotation(&mut self, annotation: &Annotation) {
        if !annotation.children.is_empty() {
            self.indent_level -= 1;
            // A block annotation's body is closed by a dedent; the parser
            // consumes the following blank line as part of that close (it is not
            // a `BlankLineGroup` in the AST, like the pre-verbatim blank in
            // lex#505), so without re-emitting it a following sibling is parsed
            // as part of the body. Emit it so the block round-trips (lex#682).
            self.ensure_blank_lines(1);
        }
    }

    fn visit_verbatim_block(&mut self, _verbatim: &Verbatim) {
        // Lex requires a blank line between a preceding paragraph and the
        // subject line that opens a verbatim block — without one, the
        // re-parser merges the subject into the preceding paragraph and
        // the verbatim is lost. The parser consumes that blank line as
        // part of the verbatim's preamble, so it isn't represented as a
        // `BlankLineGroup` in the AST and no other visitor emits it. See
        // lex#505.
        //
        // Suppress when the verbatim is the first child of a container
        // whose opener ends with `:` (Definition, list-item with colon
        // subject, etc.). A blank line at column 0 between a Definition
        // subject and its body would terminate the Definition, so the
        // body's first verbatim must follow immediately.
        if !self.last_emission_ended_with_container_opener_colon() {
            self.ensure_blank_lines(1);
        }
    }

    fn visit_verbatim_group(&mut self, group: &VerbatimGroupItemRef) {
        let subject = group.subject.as_string();
        self.write_line(&format!("{subject}:"));
        self.indent_level += 1;
    }

    fn leave_verbatim_group(&mut self, _group: &VerbatimGroupItemRef) {
        self.indent_level -= 1;
    }

    fn visit_verbatim_line(&mut self, verbatim_line: &VerbatimLine) {
        self.write_line(verbatim_line.content.as_string());
    }

    fn leave_verbatim_block(&mut self, verbatim: &Verbatim) {
        let label = source_spelling(&verbatim.closing_data.label);
        let mut footer = format!(":: {label}");
        if !verbatim.closing_data.parameters.is_empty() {
            for param in &verbatim.closing_data.parameters {
                footer.push(' ');
                footer.push_str(&param.key);
                footer.push('=');
                footer.push_str(&param.value);
            }
        }
        footer.push_str(" ::");
        self.write_line(&footer);
    }

    fn visit_table(&mut self, table: &Table) {
        // Tables share the outer verbatim shape: leading blank line,
        // subject line ending in `:`, indented body of pipe rows,
        // dedented `:: table ::` closer.
        if !self.last_emission_ended_with_container_opener_colon() {
            self.ensure_blank_lines(1);
        }

        let subject = table.subject.as_string();
        if !subject.is_empty() {
            self.write_line(&format!("{subject}:"));
        }

        self.indent_level += 1;
        emit_pipe_table(self, table);
        self.indent_level -= 1;

        // The closing `:: lex.tabular.table ::` annotation is part of
        // `table.annotations` — emitted by the standard annotation
        // walk after `leave_table` returns. Until form-preserving
        // emit lands (PR 3 of #584), the annotation walker emits
        // `label.value` verbatim; that's the canonical for now.
    }

    fn leave_table(&mut self, _table: &Table) {
        // No-op; annotations carry the closer.
    }
}

/// Emit a structural Table as a markdown-style pipe table, padded for
/// column alignment. The column count is the max-width row; shorter
/// rows pad with empty cells. Alignment follows the per-cell `align`
/// attribute, which the parser sets from the markdown alignment row
/// (`:---`, `:---:`, `---:`).
fn emit_pipe_table(serializer: &mut LexSerializer, table: &Table) {
    let all_rows: Vec<&TableRow> = table
        .header_rows
        .iter()
        .chain(table.body_rows.iter())
        .collect();
    if all_rows.is_empty() {
        return;
    }

    // Determine column count from the widest row.
    let col_count = all_rows
        .iter()
        .map(|r| r.cells.iter().map(|c| c.colspan).sum::<usize>())
        .max()
        .unwrap_or(0);
    if col_count == 0 {
        return;
    }

    // Compute per-column alignment: first non-`None` cell wins.
    let aligns = compute_column_aligns(&all_rows, col_count);

    // Compute per-column widths from cell text content. The
    // separator-row cells use `---` (or longer to match content
    // width), so include their natural width in the calc too.
    let widths = compute_column_widths(&all_rows, col_count, &aligns);

    // Emit header rows.
    for row in &table.header_rows {
        serializer.write_line(&format_pipe_row(row, &widths, col_count));
    }
    // Emit separator row (between header and body).
    if !table.header_rows.is_empty() {
        serializer.write_line(&format_separator_row(&widths, &aligns));
    }
    // Emit body rows.
    for row in &table.body_rows {
        serializer.write_line(&format_pipe_row(row, &widths, col_count));
    }
}

fn compute_column_aligns(rows: &[&TableRow], col_count: usize) -> Vec<TableCellAlignment> {
    let mut aligns = vec![TableCellAlignment::None; col_count];
    for row in rows {
        let mut col = 0;
        for cell in &row.cells {
            if col >= col_count {
                break;
            }
            if aligns[col] == TableCellAlignment::None && cell.align != TableCellAlignment::None {
                aligns[col] = cell.align;
            }
            col += cell.colspan.max(1);
        }
    }
    aligns
}

fn compute_column_widths(
    rows: &[&TableRow],
    col_count: usize,
    aligns: &[TableCellAlignment],
) -> Vec<usize> {
    let mut widths = vec![0usize; col_count];
    for row in rows {
        let mut col = 0;
        for cell in &row.cells {
            if col >= col_count {
                break;
            }
            let text_len = cell.content.as_string().trim().chars().count();
            widths[col] = widths[col].max(text_len);
            col += cell.colspan.max(1);
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

fn format_pipe_row(row: &TableRow, widths: &[usize], col_count: usize) -> String {
    let mut out = String::from("|");
    let mut col = 0;
    for cell in &row.cells {
        if col >= col_count {
            break;
        }
        let span = cell.colspan.max(1);
        // Content lives in the top-left cell of the span, at its own column
        // width; each absorbed column re-emits a `>>` marker so the column grid
        // (and the colspan) survive a re-parse (lex#683). Without this the cell
        // was merged into one wide column and the span was lost.
        // `col < col_count` here and `widths.len() == col_count`, so direct
        // indexing is in-bounds.
        push_cell(&mut out, cell.content.as_string().trim(), widths[col]);
        col += 1;
        for _ in 1..span {
            if col >= col_count {
                break;
            }
            push_cell(&mut out, ">>", widths[col]);
            col += 1;
        }
    }
    // Pad trailing empty cells.
    while col < col_count {
        push_cell(&mut out, "", widths[col]);
        col += 1;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use lex_core::lex::testing::lexplore::{ElementType, Lexplore};
    use lex_core::lex::testing::text_diff::assert_text_eq;

    fn format_source(source: &str) -> String {
        let format = super::super::LexFormat::default();
        let doc = format.parse(source).unwrap();
        let rules = FormattingRules::default();
        let mut serializer = LexSerializer::new(rules);
        doc.accept(&mut serializer);
        serializer.output
    }

    /// Format through the full `LexFormat` pipeline (annotation inlining +
    /// blank coalescing), i.e. what `lexd format` actually does — as opposed to
    /// driving the bare `LexSerializer`. Needed for annotation cases, where the
    /// pipeline strips the empty-paragraph marker artifact.
    fn format_full(source: &str) -> String {
        use crate::format::Format;
        let format = super::super::LexFormat::default();
        let doc = format.parse(source).unwrap();
        format.serialize(&doc).unwrap()
    }

    // ==== Form-preserving roundtrip tests (#584 PR 3) =====================

    #[test]
    fn shortcut_form_round_trips_to_shortcut_spelling() {
        // `:: author ::` source classifies as form=Shortcut for
        // canonical `lex.metadata.author`. The formatter must emit the
        // shortcut back, not the canonical. (The serializer's
        // single-line-vs-block emission is a separate concern; this
        // test focuses on the label-spelling preservation contract.)
        let formatted = format_source(":: author :: Alice\n\nBody.\n");
        assert!(
            formatted.contains(":: author"),
            "shortcut spelling should round-trip; got: {formatted}"
        );
        assert!(
            !formatted.contains("lex.metadata.author"),
            "canonical spelling must not leak into output: {formatted}"
        );
    }

    #[test]
    fn stripped_form_round_trips_to_stripped_spelling() {
        // `:: metadata.category ::` classifies as Stripped — formatter
        // must emit `metadata.category`, not the canonical.
        let formatted = format_source(":: metadata.category :: tech\n\nBody.\n");
        assert!(
            formatted.contains(":: metadata.category"),
            "stripped spelling should round-trip; got: {formatted}"
        );
        assert!(
            !formatted.contains("lex.metadata.category"),
            "canonical spelling must not leak: {formatted}"
        );
    }

    #[test]
    fn canonical_form_round_trips_unchanged() {
        // `:: lex.metadata.title ::` classifies as Canonical and
        // formats back as itself.
        let formatted = format_source(":: lex.metadata.title :: My Doc\n\nBody.\n");
        assert!(
            formatted.contains(":: lex.metadata.title"),
            "canonical spelling should round-trip; got: {formatted}"
        );
    }

    #[test]
    fn community_form_round_trips_unchanged() {
        let formatted = format_source(":: acme.task id=42 :: foo\n\nBody.\n");
        assert!(
            formatted.contains(":: acme.task"),
            "community label should round-trip; got: {formatted}"
        );
    }

    #[test]
    fn verbatim_shortcut_closer_round_trips() {
        // `:: image src=x.png ::` (marker form) classifies as
        // Shortcut for `lex.media.image`. The closing label must
        // emit as `image`, not canonical.
        let formatted = format_source("Photo subject:\n    alt text\n:: image src=\"x.png\" ::\n");
        assert!(
            formatted.contains(":: image"),
            "verbatim closer should preserve shortcut: {formatted}"
        );
        assert!(
            !formatted.contains("lex.media.image"),
            "canonical must not leak: {formatted}"
        );
    }

    // ==== Paragraph Tests ====

    #[test]
    fn test_paragraph_01_oneline() {
        let source = Lexplore::load(ElementType::Paragraph, 1).source();
        let formatted = format_source(&source);
        assert_text_eq(
            &formatted,
            "This is a simple paragraph with just one line.\n",
        );
    }

    #[test]
    fn test_paragraph_02_multiline() {
        let source = Lexplore::load(ElementType::Paragraph, 2).source();
        let formatted = format_source(&source);
        assert!(formatted.contains("This is a multi-line paragraph"));
        assert!(formatted.contains("second line"));
        assert!(formatted.contains("third line"));
    }

    #[test]
    fn test_paragraph_03_special_chars() {
        let source = Lexplore::load(ElementType::Paragraph, 3).source();
        let formatted = format_source(&source);
        assert!(formatted.contains("!@#$%^&*()"));
    }

    // ==== Session Tests ====

    #[test]
    fn test_session_01_simple() {
        let source = Lexplore::load(ElementType::Session, 1).source();
        let formatted = format_source(&source);
        assert!(formatted.contains("Introduction\n"));
        assert!(formatted.contains("    This is a simple session"));
    }

    #[test]
    fn test_session_02_numbered_title() {
        let source = Lexplore::load(ElementType::Session, 2).source();
        let formatted = format_source(&source);
        assert!(formatted.contains("1. Introduction:\n"));
    }

    #[test]
    fn test_session_05_nested() {
        let source = Lexplore::load(ElementType::Session, 5).source();
        let formatted = format_source(&source);
        // This is actually a complex doc with paragraphs and sessions
        assert!(formatted.contains("1. Introduction {{session-title}}\n"));
        assert!(formatted.contains("    This is the content of the session"));
    }

    // ==== List Tests ====

    #[test]
    fn test_list_01_dash() {
        let source = Lexplore::load(ElementType::List, 1).source();
        let formatted = format_source(&source);
        assert!(formatted.contains("- First item\n"));
        assert!(formatted.contains("- Second item\n"));
    }

    #[test]
    fn test_list_02_numbered() {
        let source = Lexplore::load(ElementType::List, 2).source();
        let formatted = format_source(&source);
        // Should normalize to sequential numbering
        assert!(formatted.contains("1. "));
        assert!(formatted.contains("2. "));
        assert!(formatted.contains("3. "));
    }

    #[test]
    fn test_list_03_alphabetical() {
        let source = Lexplore::load(ElementType::List, 3).source();
        let formatted = format_source(&source);
        assert!(formatted.contains("a. "));
        assert!(formatted.contains("b. "));
        assert!(formatted.contains("c. "));
    }

    #[test]
    fn test_list_04_mixed_markers() {
        let source = Lexplore::load(ElementType::List, 4).source();
        let formatted = format_source(&source);
        // Should normalize to consistent markers
        assert!(formatted.contains("1. First item\n"));
        assert!(formatted.contains("2. Second item\n"));
        assert!(formatted.contains("3. Third item\n"));
    }

    #[test]
    fn test_list_07_nested_simple() {
        let source = Lexplore::load(ElementType::List, 7).source();
        let formatted = format_source(&source);
        // Check for proper indentation of nested items
        assert!(formatted.contains("- First outer item\n"));
        assert!(formatted.contains("    - First nested item\n"));
    }

    #[test]
    fn test_list_extended_markers_preserved() {
        // NOTE: Extended markers (e.g., "1.2.3") require core parser support
        // for Form::Extended. Currently the parser treats them as standard
        // numbered lists, so normalization produces "1.", "2.", etc.
        let source = "1.2.3 Item one\n1.2.4 Item two\n";
        let formatted = format_source(source);
        assert!(formatted.contains("1. Item one\n"));
        assert!(formatted.contains("2. Item two\n"));
    }

    #[test]
    fn test_list_extended_markers_nested_normalization() {
        // Nested list with extended markers: formatter should rebuild hierarchical markers
        let source = "Test:\n\n1. Outer level one\n    1.a Middle level one\n        1.a.1 Inner level one\n        1.a.2 Inner level two\n    1.b Middle level two\n2. Outer level two\n";
        let formatted = format_source(source);
        // Outer level items
        assert!(
            formatted.contains("1. Outer level one"),
            "Expected '1. Outer level one' in: {formatted}"
        );
        assert!(
            formatted.contains("2. Outer level two"),
            "Expected '2. Outer level two' in: {formatted}"
        );
    }

    #[test]
    fn test_list_12_extended_form_fixture() {
        let source = Lexplore::load(ElementType::List, 12).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    // ==== Definition Tests ====

    #[test]
    fn test_definition_01_simple() {
        let source = Lexplore::load(ElementType::Definition, 1).source();
        let formatted = format_source(&source);
        assert!(formatted.contains("Cache:\n"));
        assert!(formatted.contains("    Temporary storage"));
    }

    #[test]
    fn test_definition_02_multi_paragraph() {
        let source = Lexplore::load(ElementType::Definition, 2).source();
        let formatted = format_source(&source);
        // Should handle multiple paragraphs in definition body
        assert!(formatted.contains("Microservice:\n"));
        assert!(formatted.contains("    An architectural style"));
        assert!(formatted.contains("    Each service is independently"));
    }

    // ==== Verbatim Tests ====

    #[test]
    fn test_verbatim_01_simple_code() {
        let source = Lexplore::load(ElementType::Verbatim, 1).source();
        let formatted = format_source(&source);
        assert!(formatted.contains(":: javascript"));
        assert!(formatted.contains("function hello()"));
    }

    #[test]
    fn test_verbatim_02_with_caption() {
        let source = Lexplore::load(ElementType::Verbatim, 2).source();
        let formatted = format_source(&source);
        // Should preserve verbatim content and captions
        assert!(formatted.contains("API Response:"));
    }

    // ==== Annotation Tests ====

    #[test]
    fn test_annotation_01_marker_simple() {
        let source = Lexplore::load(ElementType::Annotation, 1).source();
        let formatted = format_full(&source);
        // Marker annotation: closed `:: label ::` form (the open form is invalid
        // and dropped on re-parse — lex#682).
        assert_eq!(formatted, ":: note ::\n");
    }

    #[test]
    fn test_annotation_02_with_params() {
        let source = Lexplore::load(ElementType::Annotation, 2).source();
        let formatted = format_full(&source);
        assert_eq!(formatted, ":: warning severity=high ::\n");
    }

    #[test]
    fn test_annotation_05_block_paragraph() {
        let source = Lexplore::load(ElementType::Annotation, 5).source();
        let formatted = format_full(&source);
        assert_eq!(
            formatted,
            ":: note ::\n    This is an important note that requires a detailed explanation.\n"
        );
    }

    // ==== Round-trip Tests ====
    // Format → parse → format should be idempotent

    #[test]
    fn test_round_trip_paragraph_01() {
        let source = Lexplore::load(ElementType::Paragraph, 1).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_paragraph_02_multiline() {
        let source = Lexplore::load(ElementType::Paragraph, 2).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_session_01() {
        let source = Lexplore::load(ElementType::Session, 1).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_session_02_numbered() {
        let source = Lexplore::load(ElementType::Session, 2).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_list_01_dash() {
        let source = Lexplore::load(ElementType::List, 1).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_list_02_numbered() {
        let source = Lexplore::load(ElementType::List, 2).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_list_03_alphabetical() {
        let source = Lexplore::load(ElementType::List, 3).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_list_04_mixed_markers() {
        let source = Lexplore::load(ElementType::List, 4).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_list_07_nested() {
        let source = Lexplore::load(ElementType::List, 7).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_definition_01() {
        let source = Lexplore::load(ElementType::Definition, 1).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_definition_02_multi() {
        let source = Lexplore::load(ElementType::Definition, 2).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_verbatim_01() {
        let source = Lexplore::load(ElementType::Verbatim, 1).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_round_trip_verbatim_02_caption() {
        let source = Lexplore::load(ElementType::Verbatim, 2).source();
        let formatted = format_source(&source);
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_verbatim_03_table_formatting() {
        // PR 2 of #584 retired the legacy verbatim-with-markdown-body
        // path: `:: doc.table ::` is forbidden, and `:: lex.tabular.table ::`
        // / `:: tabular.table ::` source no longer round-trips through
        // a markdown reformatter. The only surviving path is the
        // structural Table element triggered by the bare `:: table ::`
        // closer — `LexSerializer::visit_table` emits the pipe table
        // directly with column alignment.
        let source = "Table Example:\n    | A | B |\n    |---|---|\n    | 1 | 2 |\n:: table ::\n";
        let formatted = format_source(source);

        // Column-aligned pipe-table output from visit_table.
        assert!(formatted.contains("| A   | B   |"));
        assert!(formatted.contains("| --- | --- |"));
        assert!(formatted.contains("| 1   | 2   |"));

        // Also test with unformatted input — visit_table normalises.
        let unformatted = "Table Example:\n    |A|B|\n    |-|-|\n    |1|2|\n:: table ::\n";
        let formatted_2 = format_source(unformatted);

        // Should be formatted nicely
        assert!(formatted_2.contains("| A   | B   |"));
        assert!(formatted_2.contains("| --- | --- |"));
        assert!(formatted_2.contains("| 1   | 2   |"));
    }

    #[test]
    fn test_round_trip_paragraph_then_verbatim_lex505() {
        // Regression: Verbatim preceded by a paragraph must keep its leading
        // blank line through a parse → serialize → parse round-trip. Without
        // the blank, the re-parser merges the verbatim's subject line into
        // the prior paragraph and the verbatim is lost. The parser consumes
        // that blank as part of the verbatim's preamble (it doesn't appear
        // as a BlankLineGroup in the AST), so the serializer has to emit it.
        //
        // Uses a `Title\n=====\n` header so the first line isn't absorbed as
        // the document title — without it, "Intro paragraph." would become
        // the doc title and the regression wouldn't be exercised.
        let source =
            "Doc\n===\n\nIntro paragraph.\n\nCode Example:\n\n    fn main() {}\n\n:: rust ::\n";
        let formatted = format_source(source);
        assert!(
            formatted.contains("Intro paragraph.\n\nCode Example:"),
            "expected blank line between paragraph and verbatim subject, got:\n{formatted}"
        );
        let formatted_again = format_source(&formatted);
        assert_text_eq(&formatted, &formatted_again);
    }

    #[test]
    fn test_verbatim_04_user_repro() {
        // The original user input had dedented marker "::  doc.table ::"
        // which caused parse-as-Definition + Document Annotation. The
        // fix is to indent the marker to match the subject. Updated
        // for PR 2 of #584: source uses the blessed `table` closer
        // which triggers structural-Table parsing; the legacy verbatim
        // path with markdown reformat is gone.
        let source = "  The Table:\n    | Markup Language | Great |\n    |--------------------|--------|\n    | Markdown | No |\n    | Lex | Yes |\n  ::  table ::\n";

        let formatted = format_source(source);

        let table_start = formatted
            .find("| Markup Language | Great |")
            .expect("Table start not found");
        let separator = formatted
            .find("| --------------- | ----- |")
            .expect("Separator not found");
        // PR 3 of #584 wired form-preserving emission: the `:: table ::`
        // source classifies as Shortcut, so the emitted closer is also
        // `:: table ::`, not the canonical `:: lex.tabular.table ::`.
        let footer_start = formatted.find(":: table ::").expect("Footer not found");

        assert!(table_start < separator);
        assert!(separator < footer_start);
    }
}
