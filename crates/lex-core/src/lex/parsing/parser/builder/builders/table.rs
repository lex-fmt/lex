//! Table builder
//!
//! Handles construction of table nodes from a subject line and indented content
//! whose first non-blank line is a pipe row. Optionally extracts a :: table ::
//! annotation from the content for configuration (align, header parameters).

use super::helpers::{collect_line_tokens, extract_annotation_header_tokens, extract_line_token};
use crate::lex::parsing::ir::{NodeType, ParseNode, ParseNodePayload};
use crate::lex::token::{LineContainer, LineType, Token};

/// Build a table node from a subject line and a container of pipe-row content.
///
/// Scans the container for a :: table :: data marker line to use as configuration.
/// If found, it is extracted and not included in the content lines.
pub(in crate::lex::parsing::parser::builder) fn build_table(
    tokens: &[LineContainer],
    subject_idx: usize,
    content_idx: usize,
) -> Result<ParseNode, String> {
    let subject_token = extract_line_token(&tokens[subject_idx])?.clone();

    // Flatten the container into line tokens
    let mut all_lines = Vec::new();
    if let Some(container) = tokens.get(content_idx) {
        collect_line_tokens(container, &mut all_lines);
    }

    // Scan for the last DataMarkerLine with "table" label — that's the config annotation.
    // Content lines inside pipe rows (e.g., :: python :: inside a cell) won't appear
    // as separate DataMarkerLine tokens because they're embedded in pipe-delimited lines.
    let mut closing_data_idx: Option<usize> = None;
    for (i, line) in all_lines.iter().enumerate() {
        if line.line_type == LineType::DataMarkerLine {
            // Check if the label is "table"
            if let Ok(header_tokens) = extract_annotation_header_tokens(line) {
                let is_table = header_tokens.iter().find_map(|(token, _)| match token {
                    Token::Text(text) => Some(text.as_str()),
                    _ => None,
                }) == Some("table");
                if is_table {
                    closing_data_idx = Some(i);
                }
            }
        }
    }

    // Extract config annotation tokens and remove from content
    let config_annotation_tokens = if let Some(idx) = closing_data_idx {
        let line = all_lines.remove(idx);
        let header_tokens = extract_annotation_header_tokens(&line)?;
        Some(header_tokens)
    } else {
        None
    };

    Ok(
        ParseNode::new(NodeType::Table, vec![], vec![]).with_payload(ParseNodePayload::Table {
            subject: subject_token,
            content_lines: all_lines,
            config_annotation_tokens,
        }),
    )
}

/// Check if the first non-blank line in a container starts with a pipe character.
///
/// This is used to distinguish tables from definitions: both match the
/// `subject + container` pattern, but tables have pipe-delimited content.
pub(in crate::lex::parsing::parser) fn container_starts_with_pipe_row(
    container: &LineContainer,
) -> bool {
    let children = match container {
        LineContainer::Container { children } => children,
        _ => return false,
    };

    for child in children {
        if let LineContainer::Token(line_token) = child {
            if line_token.line_type == LineType::BlankLine {
                continue;
            }
            // Check if first non-whitespace token is Text starting with '|'
            for token in &line_token.source_tokens {
                match token {
                    Token::Whitespace(_) | Token::Indentation => continue,
                    Token::Indent(_) => continue,
                    Token::Text(text) => return text.starts_with('|'),
                    _ => return false,
                }
            }
            return false;
        }
        // If it's a nested container, skip it (shouldn't normally happen as first child)
    }
    false
}
