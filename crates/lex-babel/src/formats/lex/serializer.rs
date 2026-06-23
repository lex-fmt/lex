use super::formatting_rules::FormattingRules;
use lex_core::lex::ast::{
    elements::{
        blank_line_group::BlankLineGroup, paragraph::TextLine, sequence_marker::Form,
        verbatim::VerbatimGroupItemRef, VerbatimLine,
    },
    traits::{AstNode, Visitor},
    Annotation, Definition, Document, List, ListItem, Paragraph, Session, Table, Verbatim,
};

use lex_core::lex::assembling::stages::normalize_labels::source_spelling;
use lex_core::lex::ast::elements::sequence_marker::DecorationStyle;

mod numbering;
mod tables;

use numbering::format_marker_index;
use tables::emit_pipe_table;

#[cfg(test)]
mod tests;

struct ListContext {
    index: usize,
    style: DecorationStyle,
    upper_case: bool,
    marker_form: Option<Form>,
}

pub struct LexSerializer {
    rules: FormattingRules,
    output: String,
    indent_level: usize,
    consecutive_newlines: usize,
    list_stack: Vec<ListContext>,
    /// Footnote lists already emitted inside their table block (lex#684). Their
    /// second, accept-driven walk must produce no output; see `suppress_output`.
    emitted_footnote_lists: Vec<*const List>,
    /// While > 0, `write_line` / `ensure_blank_lines` are no-ops. Used to swallow
    /// the redundant accept-driven walk of a table's footnote list without
    /// unbalancing the list stack — the visit still runs, only output is muted.
    suppress_output: usize,
}

impl LexSerializer {
    pub fn new(rules: FormattingRules) -> Self {
        Self {
            rules,
            output: String::new(),
            indent_level: 0,
            consecutive_newlines: 2, // Start as if we have blank lines
            list_stack: Vec::new(),
            emitted_footnote_lists: Vec::new(),
            suppress_output: 0,
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

    pub(super) fn write_line(&mut self, text: &str) {
        if self.suppress_output > 0 {
            return;
        }
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
        if self.suppress_output > 0 {
            return;
        }
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

        // A table's footnote list is emitted once inside its block by
        // `visit_table`; its second, accept-driven walk must be muted (lex#684).
        // Enter suppression here (and stay in it for any nested lists) but still
        // push the context so `leave_list` stays balanced.
        if self.suppress_output > 0 || self.emitted_footnote_lists.contains(&(list as *const List))
        {
            self.suppress_output += 1;
        }

        self.list_stack.push(ListContext {
            style,
            upper_case,
            marker_form,
            index: 1,
        });
    }

    fn leave_list(&mut self, _list: &List) {
        self.list_stack.pop();
        if self.suppress_output > 0 {
            self.suppress_output -= 1;
        }
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
        // Parameters are comma-separated: the parser treats the comma as the only
        // parameter separator (whitespace is ignored), so emitting them space-only
        // collapses `k1=v1, k2=v2` into a single `k1=v1 k2=v2` value on re-parse.
        for (i, param) in params.iter().enumerate() {
            header.push_str(if i == 0 { " " } else { ", " });
            header.push_str(&param.key);
            header.push('=');
            header.push_str(&param.value);
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

        // Emit the scoped footnote list *inside* the indented block, after the
        // rows and before the dedent, so it stays part of the table and keeps
        // its numbered markers (lex#684). `Table::accept` walks `footnotes`
        // again after `visit_table` returns — at the outer indent and after the
        // closer — so record the list here and mute that second walk
        // (`visit_list` / `suppress_output`).
        if let Some(footnotes) = &table.footnotes {
            self.ensure_blank_lines(1);
            footnotes.accept(self);
            self.emitted_footnote_lists
                .push(footnotes.as_ref() as *const List);
        }

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
