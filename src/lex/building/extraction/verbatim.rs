//! Verbatim Block Data Extraction
//!
//! This module handles the extraction of verbatim block content from line tokens,
//! including mode detection (inflow vs. fullwidth) and indentation wall stripping.
//!
//! # Verbatim Block Modes
//!
//! ## Inflow Mode (Default)
//! Content is indented relative to the subject line:
//! ```text
//! Code Example:
//!     def hello():
//!         return "world"
//! :: python ::
//! ```
//! The indentation wall is at `subject_column + INFLOW_INDENT_STEP_COLUMNS`.
//!
//! ## Fullwidth Mode
//! Content starts at a fixed, absolute column regardless of nesting:
//! ```text
//! Wide Table:
//!  Header | Value | Notes
//!  -------+-------+------
//!  Alpha  | 10    | data
//! :: table ::
//! ```
//! The indentation wall is at `FULLWIDTH_INDENT_COLUMN` (column 2 in user-facing terms,
//! index 1 in zero-based).
//!
//! # Mode Detection
//!
//! The mode is automatically inferred by examining the first non-blank content line:
//! - If its first non-whitespace character is at column index 1 → Fullwidth
//! - Otherwise → Inflow
//!
//! # Indentation Wall Stripping
//!
//! After mode detection, the appropriate indentation wall is calculated and stripped
//! from all content lines. This normalization ensures verbatim content at different
//! nesting levels produces identical text output.
//!
//! # Verbatim Groups
//!
//! A single verbatim block can contain multiple subject/content pairs sharing one
//! closing annotation:
//! ```text
//! First command:
//!     $ ls
//! Second command:
//!     $ pwd
//! :: shell ::
//! ```
//! The `split_groups` function identifies these pairs by finding subject lines
//! at the same indentation level as the first subject.

use crate::lex::ast::elements::verbatim::VerbatimBlockMode;
use crate::lex::token::line::{LineToken, LineType};
use crate::lex::token::normalization::utilities::{compute_bounding_box, extract_text};
use crate::lex::token::Token;
use std::ops::Range as ByteRange;

/// The column (zero-based index) at which fullwidth verbatim content starts.
///
/// This is column 2 in user-facing terms (1-based). Using column 2 provides:
/// - Clear visual offset from the left margin
/// - Avoids ambiguity with closing annotation markers (`::`) which appear at column 1
///
/// See: docs/dev/proposals/fullwidth.lex for design rationale
pub(crate) const FULLWIDTH_INDENT_COLUMN: usize = 1;

/// The number of columns (spaces) that represent one indentation step in Lex.
///
/// Lex uses 4-space indentation. This constant is used to calculate the indentation
/// wall for inflow mode: `subject_column + INFLOW_INDENT_STEP_COLUMNS`.
pub(crate) const INFLOW_INDENT_STEP_COLUMNS: usize = 4;

/// Extracted data for an individual verbatim group item.
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct VerbatimGroupData {
    pub subject_text: String,
    pub subject_byte_range: ByteRange<usize>,
    pub content_lines: Vec<(String, ByteRange<usize>)>,
}

/// Extracted data for building a VerbatimBlock AST node.
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct VerbatimBlockData {
    pub groups: Vec<VerbatimGroupData>,
    pub mode: VerbatimBlockMode,
}

pub(in crate::lex::building) fn extract_verbatim_block_data(
    subject_line: &LineToken,
    content_lines: &[LineToken],
    source: &str,
) -> VerbatimBlockData {
    let mode = detect_mode(content_lines, source);
    let subject_column = first_visual_column(subject_line, source).unwrap_or(0);
    let wall_column = match mode {
        VerbatimBlockMode::Fullwidth => FULLWIDTH_INDENT_COLUMN,
        VerbatimBlockMode::Inflow => subject_column + INFLOW_INDENT_STEP_COLUMNS,
    };

    let groups = split_groups(subject_line, content_lines, subject_column, source)
        .into_iter()
        .map(|(subject, lines)| extract_group(subject, lines, wall_column, source))
        .collect();

    VerbatimBlockData { groups, mode }
}

fn detect_mode(content_lines: &[LineToken], source: &str) -> VerbatimBlockMode {
    for line in content_lines {
        if is_effectively_blank(line) {
            continue;
        }
        if let Some(column) = first_visual_column(line, source) {
            if column == FULLWIDTH_INDENT_COLUMN {
                return VerbatimBlockMode::Fullwidth;
            } else {
                break;
            }
        }
    }
    VerbatimBlockMode::Inflow
}

/// Split content lines into verbatim groups.
///
/// A verbatim block can contain multiple subject/content pairs (groups) that share
/// a single closing annotation. This function identifies group boundaries by finding
/// subject lines at the same indentation level as the first subject.
///
/// # Algorithm
///
/// 1. Start with the first subject and empty content accumulator
/// 2. For each content line:
///    - If the line is a subject at `base_subject_column`, it starts a new group:
///      push the accumulated (subject, content) pair and start fresh
///    - Otherwise, add the line to the current group's content
/// 3. After processing all lines, push the final accumulated group
///
/// # Invariants
///
/// - All group subjects must be at the same indentation level (`base_subject_column`)
/// - Blank lines between groups stay attached to the previous group's content
/// - Leading blank lines after a subject are preserved as part of that group's content
/// - Returns at least one group (the first subject with possibly empty content)
///
/// # Arguments
///
/// * `first_subject` - The initial subject line that starts the verbatim block
/// * `content_lines` - All lines between the first subject and closing annotation
/// * `base_subject_column` - The column where all group subjects must start
/// * `source` - Original source text for column calculation
///
/// # Returns
///
/// Vector of (subject, content_lines) pairs in order of appearance
fn split_groups(
    first_subject: &LineToken,
    content_lines: &[LineToken],
    base_subject_column: usize,
    source: &str,
) -> Vec<(LineToken, Vec<LineToken>)> {
    let mut groups = Vec::new();
    let mut current_subject = first_subject.clone();
    let mut current_content: Vec<LineToken> = Vec::new();

    for line in content_lines {
        // Check if this line starts a new group (subject at base indentation).
        if is_new_group_subject(line, base_subject_column, source) {
            // Save the current group and start a new one
            groups.push((current_subject, current_content));
            current_subject = line.clone();
            current_content = Vec::new();
        } else {
            // Add line to current group's content
            current_content.push(line.clone());
        }
    }

    // Don't forget the final group
    groups.push((current_subject, current_content));
    groups
}

fn extract_group(
    subject_line: LineToken,
    content_lines: Vec<LineToken>,
    wall_column: usize,
    source: &str,
) -> VerbatimGroupData {
    let subject_pairs: Vec<_> = subject_line
        .source_token_pairs()
        .into_iter()
        .filter(|(token, _)| !matches!(token, Token::Colon | Token::BlankLine(_)))
        .collect();
    let subject_byte_range = if subject_pairs.is_empty() {
        0..0
    } else {
        compute_bounding_box(&subject_pairs)
    };
    let subject_text = extract_text(subject_byte_range.clone(), source)
        .trim()
        .to_string();

    let content_lines: Vec<(String, ByteRange<usize>)> = content_lines
        .into_iter()
        .map(|line| extract_content_line(line, wall_column, source))
        .collect();

    VerbatimGroupData {
        subject_text,
        subject_byte_range,
        content_lines,
    }
}

fn extract_content_line(
    line: LineToken,
    wall_column: usize,
    source: &str,
) -> (String, ByteRange<usize>) {
    let bounds = line_bounds(&line);
    if bounds.is_none() {
        return (String::new(), 0..0);
    }
    let (first_token_start, line_end) = bounds.unwrap();
    let trimmed_end = trim_trailing_newline(source, first_token_start, line_end);
    if trimmed_end <= first_token_start {
        return (String::new(), first_token_start..first_token_start);
    }

    // The token spans don't include leading indentation whitespace (consumed by the
    // tree builder as Indent markers). Find the actual line start in the source by
    // scanning backwards to the preceding newline.
    let actual_line_start = source[..first_token_start]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0);

    let start_offset = advance_to_wall(source, actual_line_start, trimmed_end, wall_column);
    if start_offset >= trimmed_end {
        return (String::new(), trimmed_end..trimmed_end);
    }

    let text = source[start_offset..trimmed_end].to_string();
    (text, start_offset..trimmed_end)
}

/// Check if a line is effectively blank (contains only whitespace tokens).
///
/// Used to distinguish truly blank lines from those that might have content.
/// This helps with proper handling of inter-group spacing in verbatim blocks.
fn is_effectively_blank(line: &LineToken) -> bool {
    line.source_tokens.iter().all(|token| token.is_whitespace())
}

/// Determine if a line is a new group subject at the specified column.
///
/// A line qualifies as a new group subject if:
/// 1. It has a subject line type (SubjectLine or SubjectOrListItemLine)
/// 2. Its first non-whitespace content starts at exactly `base_column`
///
/// # Arguments
///
/// * `line` - The line token to check
/// * `base_column` - The column where group subjects must start
/// * `source` - Original source text for column calculation
///
/// # Returns
///
/// `true` if this line starts a new verbatim group, `false` otherwise
fn is_new_group_subject(line: &LineToken, base_column: usize, source: &str) -> bool {
    if !matches!(
        line.line_type,
        LineType::SubjectLine | LineType::SubjectOrListItemLine
    ) {
        return false;
    }
    first_visual_column(line, source) == Some(base_column)
}

/// Find the visual column of the first non-whitespace token in a line.
///
/// Visual columns account for tab expansion (tabs count as 4 columns each).
/// Returns `None` if the line contains only whitespace.
///
/// # Arguments
///
/// * `line` - The line token to analyze
/// * `source` - Original source text for column calculation
///
/// # Returns
///
/// The visual column (0-indexed) where the first content appears, or `None` for blank lines
fn first_visual_column(line: &LineToken, source: &str) -> Option<usize> {
    line.source_token_pairs()
        .into_iter()
        .find(|(token, _)| !token.is_whitespace())
        .map(|(_, range)| visual_column_at(range.start, source))
}

fn visual_column_at(offset: usize, source: &str) -> usize {
    let line_start = source[..offset].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let mut column = 0;
    let mut idx = line_start;
    while idx < offset {
        let ch = source[idx..].chars().next().unwrap();
        if ch == '\r' {
            idx += 1;
            continue;
        }
        if ch.is_whitespace() {
            column += whitespace_width(ch);
        } else {
            column += 1;
        }
        idx += ch.len_utf8();
    }
    column
}

fn line_bounds(line: &LineToken) -> Option<(usize, usize)> {
    let pairs = line.source_token_pairs();
    if pairs.is_empty() {
        None
    } else {
        let range = compute_bounding_box(&pairs);
        Some((range.start, range.end))
    }
}

fn trim_trailing_newline(source: &str, start: usize, mut end: usize) -> usize {
    while end > start {
        let byte = source.as_bytes()[end - 1];
        if byte == b'\n' || byte == b'\r' {
            end -= 1;
        } else {
            break;
        }
    }
    end
}

fn advance_to_wall(source: &str, start: usize, end: usize, wall_column: usize) -> usize {
    let mut column = 0;
    let mut offset = start;
    while offset < end && column < wall_column {
        let ch = source[offset..].chars().next().unwrap();
        if !ch.is_whitespace() {
            break;
        }
        column += whitespace_width(ch);
        offset += ch.len_utf8();
    }
    offset.min(end)
}

fn whitespace_width(ch: char) -> usize {
    match ch {
        '\t' => INFLOW_INDENT_STEP_COLUMNS,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::token::Token;

    struct SourceBuilder {
        text: String,
    }

    impl SourceBuilder {
        fn new() -> Self {
            Self {
                text: String::new(),
            }
        }

        fn push(&mut self, fragment: &str) -> ByteRange<usize> {
            let start = self.text.len();
            self.text.push_str(fragment);
            start..self.text.len()
        }
    }

    fn line_token(line_type: LineType, parts: Vec<(Token, ByteRange<usize>)>) -> LineToken {
        let (tokens, spans): (Vec<_>, Vec<_>) = parts.into_iter().unzip();
        LineToken {
            source_tokens: tokens,
            token_spans: spans,
            line_type,
        }
    }

    fn subject_line(builder: &mut SourceBuilder, indent_levels: usize, label: &str) -> LineToken {
        let mut parts = Vec::new();
        for _ in 0..indent_levels {
            let range = builder.push("    ");
            parts.push((Token::Indentation, range));
        }
        let range = builder.push(label);
        parts.push((Token::Text(label.to_string()), range));
        let range = builder.push(":");
        parts.push((Token::Colon, range));
        let range = builder.push("\n");
        parts.push((Token::BlankLine(Some("\n".to_string())), range));
        line_token(LineType::SubjectLine, parts)
    }

    fn content_line(builder: &mut SourceBuilder, indent_spaces: usize, text: &str) -> LineToken {
        let mut parts = Vec::new();
        for _ in 0..indent_spaces {
            let range = builder.push(" ");
            parts.push((Token::Whitespace(1), range));
        }
        if !text.is_empty() {
            let range = builder.push(text);
            parts.push((Token::Text(text.to_string()), range));
        }
        let range = builder.push("\n");
        parts.push((Token::BlankLine(Some("\n".to_string())), range));
        line_token(LineType::ParagraphLine, parts)
    }

    #[test]
    fn detects_fullwidth_mode_and_trims_wall() {
        let mut builder = SourceBuilder::new();
        let subject = subject_line(&mut builder, 0, "Fullwidth Example");
        // Content at FULLWIDTH_INDENT_COLUMN triggers fullwidth mode
        let content = content_line(
            &mut builder,
            FULLWIDTH_INDENT_COLUMN,
            "Header | Value | Notes",
        );

        let data = extract_verbatim_block_data(&subject, &[content], &builder.text);

        assert_eq!(data.mode, VerbatimBlockMode::Fullwidth);
        assert_eq!(data.groups.len(), 1);
        assert_eq!(data.groups[0].content_lines.len(), 1);
        assert_eq!(data.groups[0].content_lines[0].0, "Header | Value | Notes");
        assert!(data.groups[0].content_lines[0].1.start < data.groups[0].content_lines[0].1.end);
    }

    #[test]
    fn splits_groups_and_strips_inflow_wall() {
        let mut builder = SourceBuilder::new();
        let subject_indent_level = 1;
        let subject = subject_line(&mut builder, subject_indent_level, "Snippet");
        // Inflow mode: content indented relative to subject (subject_column + INFLOW_INDENT_STEP_COLUMNS)
        // Subject at indent level 1 = column 4 (1 * 4 spaces)
        // Content wall = 4 + 4 = column 8
        let inflow_content_column = (subject_indent_level * 4) + INFLOW_INDENT_STEP_COLUMNS;
        let line1 = content_line(&mut builder, inflow_content_column, "line one");
        let line2 = content_line(&mut builder, inflow_content_column, "line two");
        let second_subject = subject_line(&mut builder, subject_indent_level, "Another block");
        let line3 = content_line(&mut builder, inflow_content_column, "inner body");

        let content = vec![line1, line2, second_subject.clone(), line3];
        let data = extract_verbatim_block_data(&subject, &content, &builder.text);

        assert_eq!(data.mode, VerbatimBlockMode::Inflow);
        assert_eq!(data.groups.len(), 2);
        assert_eq!(data.groups[0].subject_text, "Snippet");
        assert_eq!(data.groups[0].content_lines[0].0, "line one");
        assert_eq!(data.groups[1].subject_text, "Another block");
        assert_eq!(data.groups[1].content_lines[0].0, "inner body");
    }

    #[test]
    fn detects_fullwidth_at_exact_boundary() {
        // Test that content at exactly column index 1 (FULLWIDTH_INDENT_COLUMN) triggers fullwidth
        let mut builder = SourceBuilder::new();
        let subject = subject_line(&mut builder, 0, "Boundary Test");
        let content = content_line(&mut builder, FULLWIDTH_INDENT_COLUMN, "At exact boundary");

        let data = extract_verbatim_block_data(&subject, &[content], &builder.text);

        assert_eq!(data.mode, VerbatimBlockMode::Fullwidth);
        assert_eq!(data.groups[0].content_lines[0].0, "At exact boundary");
    }

    #[test]
    fn detects_inflow_at_column_2() {
        // Test that content at column 2 or beyond triggers inflow mode
        let mut builder = SourceBuilder::new();
        let subject = subject_line(&mut builder, 0, "Inflow Test");
        // Column 2 (index 2, which is > FULLWIDTH_INDENT_COLUMN)
        let content = content_line(&mut builder, 2, "Beyond fullwidth boundary");

        let data = extract_verbatim_block_data(&subject, &[content], &builder.text);

        assert_eq!(data.mode, VerbatimBlockMode::Inflow);
    }

    #[test]
    fn detects_inflow_at_standard_indent() {
        // Test inflow mode with standard 4-space indent after root-level subject
        let mut builder = SourceBuilder::new();
        let subject = subject_line(&mut builder, 0, "Standard Test");
        // Subject at column 0, so inflow content should be at column 4
        let content = content_line(&mut builder, INFLOW_INDENT_STEP_COLUMNS, "Standard indent");

        let data = extract_verbatim_block_data(&subject, &[content], &builder.text);

        assert_eq!(data.mode, VerbatimBlockMode::Inflow);
        assert_eq!(data.groups[0].content_lines[0].0, "Standard indent");
    }

    #[test]
    fn detects_mode_skips_blank_lines() {
        // Test that mode detection skips blank lines to find first content
        let mut builder = SourceBuilder::new();
        let subject = subject_line(&mut builder, 0, "With Blanks");
        let blank1 = content_line(&mut builder, 0, "");
        let blank2 = content_line(&mut builder, 0, "");
        let content = content_line(&mut builder, FULLWIDTH_INDENT_COLUMN, "First real content");

        let data = extract_verbatim_block_data(&subject, &[blank1, blank2, content], &builder.text);

        assert_eq!(data.mode, VerbatimBlockMode::Fullwidth);
        // Blank lines should be included but empty
        assert_eq!(data.groups[0].content_lines.len(), 3);
        assert_eq!(data.groups[0].content_lines[2].0, "First real content");
    }

    #[test]
    fn handles_tabs_in_fullwidth_content() {
        // Test that tabs are properly counted as 4 columns (INFLOW_INDENT_STEP_COLUMNS)
        let mut builder = SourceBuilder::new();
        let subject = subject_line(&mut builder, 0, "Tab Test");

        // Create content with a tab character - tab should count as 4 columns
        // For fullwidth to trigger, content must start at column 1
        // A single space (column 1) followed by text should trigger fullwidth
        let mut parts = Vec::new();
        let range = builder.push(" "); // Single space at column 1
        parts.push((Token::Whitespace(1), range));
        let range = builder.push("Content\twith\ttabs");
        parts.push((Token::Text("Content\twith\ttabs".to_string()), range));
        let range = builder.push("\n");
        parts.push((Token::BlankLine(Some("\n".to_string())), range));
        let content = line_token(LineType::ParagraphLine, parts);

        let data = extract_verbatim_block_data(&subject, &[content], &builder.text);

        assert_eq!(data.mode, VerbatimBlockMode::Fullwidth);
        assert_eq!(data.groups[0].content_lines[0].0, "Content\twith\ttabs");
    }

    #[test]
    fn handles_empty_fullwidth_block() {
        // Test fullwidth block with no content (only blank lines before annotation)
        let mut builder = SourceBuilder::new();
        let subject = subject_line(&mut builder, 0, "Empty Fullwidth");

        // No content lines - the annotation would follow immediately
        let data = extract_verbatim_block_data(&subject, &[], &builder.text);

        // With no content, defaults to Inflow mode (since no line to detect from)
        assert_eq!(data.mode, VerbatimBlockMode::Inflow);
        assert_eq!(data.groups.len(), 1);
        assert_eq!(data.groups[0].content_lines.len(), 0);
    }

    #[test]
    fn handles_fullwidth_with_only_blank_lines() {
        // Test fullwidth block where all content lines are blank
        let mut builder = SourceBuilder::new();
        let subject = subject_line(&mut builder, 0, "All Blanks");
        let blank1 = content_line(&mut builder, 0, "");
        let blank2 = content_line(&mut builder, 0, "");
        let blank3 = content_line(&mut builder, 0, "");

        let data = extract_verbatim_block_data(&subject, &[blank1, blank2, blank3], &builder.text);

        // All blank lines means no real content to detect mode from, defaults to Inflow
        assert_eq!(data.mode, VerbatimBlockMode::Inflow);
        assert_eq!(data.groups[0].content_lines.len(), 3);
    }

    #[test]
    fn handles_tab_indentation_in_inflow_mode() {
        // Test that tabs work correctly in inflow mode
        // A tab should count as 4 columns (INFLOW_INDENT_STEP_COLUMNS)
        let mut builder = SourceBuilder::new();
        let subject = subject_line(&mut builder, 0, "Tab Inflow");

        // Create content with tabs that add up to inflow indent (4 columns)
        let mut parts = Vec::new();
        let range = builder.push("\t"); // Tab = 4 columns
        parts.push((Token::Indentation, range));
        let range = builder.push("Tabbed content");
        parts.push((Token::Text("Tabbed content".to_string()), range));
        let range = builder.push("\n");
        parts.push((Token::BlankLine(Some("\n".to_string())), range));
        let content = line_token(LineType::ParagraphLine, parts);

        let data = extract_verbatim_block_data(&subject, &[content], &builder.text);

        assert_eq!(data.mode, VerbatimBlockMode::Inflow);
        assert_eq!(data.groups[0].content_lines[0].0, "Tabbed content");
    }
}
