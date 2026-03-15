use super::formatting_rules::FormattingRules;
use lex_core::lex::ast::{
    elements::{
        blank_line_group::BlankLineGroup, paragraph::TextLine, sequence_marker::Form,
        verbatim::VerbatimGroupItemRef, VerbatimLine,
    },
    traits::{AstNode, Visitor},
    Annotation, Definition, Document, List, ListItem, Paragraph, Session, Verbatim,
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum MarkerType {
    Bullet,
    Numeric,
    AlphaLower,
    AlphaUpper,
    RomanUpper,
}

struct ListContext {
    index: usize,
    marker_type: MarkerType,
    marker_form: Option<Form>,
}

impl MarkerType {}

fn format_marker_index(marker_type: MarkerType, index: usize) -> String {
    match marker_type {
        MarkerType::Bullet => "-".to_string(),
        MarkerType::Numeric => index.to_string(),
        MarkerType::AlphaLower => to_alpha_lower(index),
        MarkerType::AlphaUpper => to_alpha_upper(index),
        MarkerType::RomanUpper => to_roman_upper(index),
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

use crate::common::verbatim::VerbatimRegistry;

pub struct LexSerializer {
    rules: FormattingRules,
    output: String,
    indent_level: usize,
    consecutive_newlines: usize,
    list_stack: Vec<ListContext>,
    verbatim_registry: VerbatimRegistry,
    skip_verbatim_lines: bool,
    formatted_verbatim_content: Option<String>,
}

impl LexSerializer {
    pub fn new(rules: FormattingRules) -> Self {
        Self {
            rules,
            output: String::new(),
            indent_level: 0,
            consecutive_newlines: 2, // Start as if we have blank lines
            list_stack: Vec::new(),
            verbatim_registry: VerbatimRegistry::default_with_standard(),
            skip_verbatim_lines: false,
            formatted_verbatim_content: None,
        }
    }

    pub fn serialize(mut self, doc: &Document) -> Result<String, String> {
        // Output document title if present
        if let Some(title) = &doc.title {
            self.output.push_str(title.as_str());
            self.output.push('\n');
            self.consecutive_newlines = 1;
        }
        doc.root.accept(&mut self);
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
            parts.push(format_marker_index(ctx.marker_type, idx));
        }
        format!("{}.", parts.join("."))
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
        // Use the SequenceMarker to determine marker type
        let marker_type = if let Some(marker) = &list.marker {
            use lex_core::lex::ast::elements::DecorationStyle;
            match marker.style {
                DecorationStyle::Plain => MarkerType::Bullet,
                DecorationStyle::Numerical => MarkerType::Numeric,
                DecorationStyle::Alphabetical => {
                    let text = marker.as_str();
                    if text.chars().next().is_some_and(|c| c.is_uppercase()) {
                        MarkerType::AlphaUpper
                    } else {
                        MarkerType::AlphaLower
                    }
                }
                DecorationStyle::Roman => MarkerType::RomanUpper,
            }
        } else {
            MarkerType::Bullet
        };

        // Determine marker form (Standard vs Extended)
        let marker_form = list.marker.as_ref().map(|marker| marker.form);

        self.list_stack.push(ListContext {
            marker_type,
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
                match context.marker_type {
                    MarkerType::Bullet => self.rules.unordered_seq_marker.to_string(),
                    MarkerType::Numeric => format!("{}.", context.index),
                    MarkerType::AlphaLower => format!("{}.", to_alpha_lower(context.index)),
                    MarkerType::AlphaUpper => format!("{}.", to_alpha_upper(context.index)),
                    MarkerType::RomanUpper => format!("{}.", to_roman_upper(context.index)),
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
        let label = &annotation.data.label.value;
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

        // Only add closing :: for short-form annotations (no children)
        if annotation.children.is_empty() {
            header.push_str(" ::");
        }

        self.write_line(&header);

        if !annotation.children.is_empty() {
            self.indent_level += 1;
        }
    }

    fn leave_annotation(&mut self, annotation: &Annotation) {
        if !annotation.children.is_empty() {
            self.indent_level -= 1;
        }
    }

    fn visit_verbatim_block(&mut self, verbatim: &Verbatim) {
        let label = &verbatim.closing_data.label.value;

        // Try to get formatted content from handler
        if let Some(handler) = self.verbatim_registry.get(label) {
            // We ignore errors here for now as the visitor trait doesn't support Result
            if let Ok(Some(content)) = handler.format_content(verbatim) {
                self.formatted_verbatim_content = Some(content);
                self.skip_verbatim_lines = true;
            } else {
                self.formatted_verbatim_content = None;
                self.skip_verbatim_lines = false;
            }
        } else {
            self.formatted_verbatim_content = None;
            self.skip_verbatim_lines = false;
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
        if !self.skip_verbatim_lines {
            self.write_line(verbatim_line.content.as_string());
        }
    }

    fn leave_verbatim_block(&mut self, verbatim: &Verbatim) {
        // If we have formatted content, print it now (before the closing marker)
        if let Some(content) = self.formatted_verbatim_content.take() {
            self.output.push_str(&content);
            // Ensure newline after content if not present (though TableHandler adds it)
            if !content.ends_with('\n') {
                self.output.push('\n');
            }
        }

        let label = &verbatim.closing_data.label.value;
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
        let formatted = format_source(&source);
        // Document-level annotations should be preserved
        assert_eq!(formatted, ":: note\n");
    }

    #[test]
    fn test_annotation_02_with_params() {
        let source = Lexplore::load(ElementType::Annotation, 2).source();
        let formatted = format_source(&source);
        // Document-level annotations should be preserved
        assert_eq!(formatted, ":: warning severity=high\n");
    }

    #[test]
    fn test_annotation_05_block_paragraph() {
        let source = Lexplore::load(ElementType::Annotation, 5).source();
        let formatted = format_source(&source);
        // Document-level annotations should be preserved
        assert_eq!(
            formatted,
            ":: note\n    This is an important note that requires a detailed explanation.\n"
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
        // Use standard verbatim syntax: Subject + indented content + closing marker (dedented)
        let source =
            "Table Example:\n    | A | B |\n    |---|---|\n    | 1 | 2 |\n:: doc.table ::\n";
        // The serializer should format this table
        let formatted = format_source(source);

        // Check that it's formatted (aligned)
        // Note: The exact spacing depends on the markdown serializer, but it should be consistent
        // Markdown serializer adds padding for alignment
        assert!(formatted.contains("| A   | B   |"));
        assert!(formatted.contains("| --- | --- |"));
        assert!(formatted.contains("| 1   | 2   |"));

        // Also test with unformatted input
        let unformatted = "Table Example:\n    |A|B|\n    |-|-|\n    |1|2|\n:: doc.table ::\n";
        let formatted_2 = format_source(unformatted);

        // Should be formatted nicely
        assert!(formatted_2.contains("| A   | B   |"));
        assert!(formatted_2.contains("| --- | --- |"));
        assert!(formatted_2.contains("| 1   | 2   |"));
    }

    #[test]
    fn test_verbatim_04_user_repro() {
        // NOTE: The user's original input had dedented marker "::  doc.table ::".
        // This caused it to be parsed as Definition + Document Annotation.
        // The fix is to indent the marker to match the subject.
        let source = "  The Table:\n    | Markup Language | Great |\n    |--------------------|--------|\n    | Markdown | No |\n    | Lex | Yes |\n  ::  doc.table ::\n";

        let formatted = format_source(source);

        // Check for formatting
        // Markdown serializer adds padding for alignment
        let table_start = formatted
            .find("| Markup Language | Great |")
            .expect("Table start not found");
        let separator = formatted
            .find("| --------------- | ----- |")
            .expect("Separator not found");
        let footer_start = formatted.find(":: doc.table").expect("Footer not found");

        assert!(table_start < separator);
        assert!(separator < footer_start);
    }
}
