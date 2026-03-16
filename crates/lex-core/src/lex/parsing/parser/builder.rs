//! AST Node Builder
//!
//! This module converts matched grammar patterns into ParseNode AST structures.
//! It handles the extraction of tokens from LineContainers and the recursive
//! descent into nested containers.

use crate::lex::parsing::ir::ParseNode;
use crate::lex::token::LineContainer;
use std::ops::Range;

mod builders;

use builders::{
    build_annotation_block, build_annotation_single, build_blank_line_group, build_definition,
    build_list, build_paragraph, build_session, build_verbatim_block,
};

/// Type alias for the recursive parser function callback
type ParserFn = dyn Fn(Vec<LineContainer>, &str) -> Result<Vec<ParseNode>, String>;

/// Represents the result of pattern matching
#[derive(Debug, Clone)]
pub(super) enum PatternMatch {
    /// Verbatim block: subject + arbitrary content lines + closing annotation
    VerbatimBlock {
        subject_idx: usize,
        content_range: Range<usize>,
        closing_idx: usize,
    },
    /// Annotation block: start + container + end
    AnnotationBlock {
        start_idx: usize,
        content_idx: usize,
    },
    /// Annotation single: just start line
    AnnotationSingle { start_idx: usize },
    /// List: optional preceding/trailing blanks + 2+ consecutive list items
    List {
        items: Vec<(usize, Option<usize>)>,
        preceding_blank_range: Option<Range<usize>>,
        trailing_blank_range: Option<Range<usize>>,
    },
    /// Definition: subject + immediate indent + content
    Definition {
        subject_idx: usize,
        content_idx: usize,
    },
    /// Session: subject + blank line + indent + content
    Session {
        subject_idx: usize,
        content_idx: usize,
        preceding_blank_range: Option<Range<usize>>,
    },
    /// Paragraph: one or more consecutive non-blank, non-special lines
    Paragraph { start_idx: usize, end_idx: usize },
    /// Blank line group: one or more consecutive blank lines
    BlankLineGroup,
    /// Document title: single line followed by blank lines, no indented content.
    /// If the title line ends with a colon and a second line follows before the
    /// blank separator, the second line is parsed as a subtitle.
    DocumentTitle {
        title_idx: usize,
        subtitle_idx: Option<usize>,
    },
    /// Document start marker: synthetic boundary between metadata and content
    DocumentStart,
}

/// Convert a matched pattern to a ParseNode.
///
/// # Arguments
///
/// * `tokens` - The full token array
/// * `pattern` - The matched pattern with relative indices
/// * `pattern_offset` - Index where the pattern starts (converts relative to absolute indices)
/// * `source` - Original source text
/// * `parse_children` - Function to recursively parse nested containers
pub(super) fn convert_pattern_to_node(
    tokens: &[LineContainer],
    pattern: &PatternMatch,
    pattern_range: Range<usize>,
    source: &str,
    parse_children: &ParserFn,
) -> Result<ParseNode, String> {
    let pattern_offset = pattern_range.start;
    match pattern {
        PatternMatch::VerbatimBlock {
            subject_idx,
            content_range,
            closing_idx,
        } => build_verbatim_block(tokens, *subject_idx, content_range.clone(), *closing_idx),
        PatternMatch::AnnotationBlock {
            start_idx,
            content_idx,
        } => build_annotation_block(
            tokens,
            pattern_offset + start_idx,
            pattern_offset + content_idx,
            source,
            parse_children,
        ),
        PatternMatch::AnnotationSingle { start_idx } => {
            build_annotation_single(tokens, pattern_offset + start_idx)
        }
        PatternMatch::List { items, .. } => {
            build_list(tokens, items, pattern_offset, source, parse_children)
        }
        PatternMatch::Definition {
            subject_idx,
            content_idx,
        } => build_definition(
            tokens,
            pattern_offset + subject_idx,
            pattern_offset + content_idx,
            source,
            parse_children,
        ),
        PatternMatch::Session {
            subject_idx,
            content_idx,
            ..
        } => build_session(
            tokens,
            pattern_offset + subject_idx,
            pattern_offset + content_idx,
            source,
            parse_children,
        ),
        PatternMatch::Paragraph { start_idx, end_idx } => {
            build_paragraph(tokens, pattern_offset + start_idx, pattern_offset + end_idx)
        }
        PatternMatch::BlankLineGroup => build_blank_line_group(tokens, pattern_range.clone()),
        PatternMatch::DocumentTitle {
            title_idx,
            subtitle_idx,
        } => build_document_title(
            tokens,
            pattern_offset + title_idx,
            subtitle_idx.map(|i| pattern_offset + i),
        ),
        PatternMatch::DocumentStart => build_document_start(),
    }
}

/// Build a DocumentTitle node from the title line token and optional subtitle token.
///
/// When the title line ends with a colon and a subtitle line follows, the colon
/// is stripped from the title tokens and the subtitle tokens are attached as a
/// child node with `NodeType::DocumentSubtitle`.
fn build_document_title(
    tokens: &[LineContainer],
    title_idx: usize,
    subtitle_idx: Option<usize>,
) -> Result<ParseNode, String> {
    let mut title_tokens: Vec<(crate::lex::lexing::Token, std::ops::Range<usize>)> =
        match &tokens[title_idx] {
            LineContainer::Token(line) => line
                .source_tokens
                .iter()
                .zip(line.token_spans.iter())
                .map(|(token, span)| (token.clone(), span.clone()))
                .collect(),
            LineContainer::Container { .. } => {
                return Err("Expected title token, found container".to_string())
            }
        };

    let mut children = vec![];

    if let Some(sub_idx) = subtitle_idx {
        // Strip the trailing colon from the title tokens.
        // The last meaningful token should be the colon (SubjectMarker) or
        // text ending with a colon.
        strip_trailing_colon(&mut title_tokens);

        let subtitle_tokens = match &tokens[sub_idx] {
            LineContainer::Token(line) => line
                .source_tokens
                .iter()
                .zip(line.token_spans.iter())
                .map(|(token, span)| (token.clone(), span.clone()))
                .collect(),
            LineContainer::Container { .. } => {
                return Err("Expected subtitle token, found container".to_string())
            }
        };

        children.push(ParseNode::new(
            crate::lex::parsing::ir::NodeType::DocumentSubtitle,
            subtitle_tokens,
            vec![],
        ));
    }

    Ok(ParseNode::new(
        crate::lex::parsing::ir::NodeType::DocumentTitle,
        title_tokens,
        children,
    ))
}

/// Strip the trailing colon from the title token list.
/// The lexer represents the colon as a separate `Colon` token. Walk from the
/// end, skip BlankLine/Whitespace, and remove the first `Colon` found.
fn strip_trailing_colon(tokens: &mut Vec<(crate::lex::lexing::Token, std::ops::Range<usize>)>) {
    use crate::lex::lexing::Token;

    if tokens.is_empty() {
        return;
    }

    let mut idx = tokens.len();
    while idx > 0 {
        idx -= 1;
        match &tokens[idx].0 {
            Token::BlankLine(_) | Token::Whitespace(_) => continue,
            Token::Colon => {
                tokens.remove(idx);
                return;
            }
            _ => return,
        }
    }
}

/// Build a DocumentStart node (synthetic marker with no content)
fn build_document_start() -> Result<ParseNode, String> {
    Ok(ParseNode::new(
        crate::lex::parsing::ir::NodeType::DocumentStart,
        vec![],
        vec![],
    ))
}

pub(super) fn blank_line_node_from_range(
    tokens: &[LineContainer],
    token_range: Range<usize>,
) -> Result<ParseNode, String> {
    build_blank_line_group(tokens, token_range)
}
