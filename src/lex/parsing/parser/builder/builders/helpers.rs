//! Helper functions for AST builders
//!
//! This module provides utility functions used across different element builders
//! for extracting and processing tokens.

use crate::lex::annotation::analyze_annotation_header_token_pairs;
use crate::lex::escape::find_structural_lex_marker_pairs;
use crate::lex::token::{LineContainer, LineToken, Token};
use std::ops::Range;

/// Extract a LineToken from a LineContainer
pub(super) fn extract_line_token(token: &LineContainer) -> Result<&LineToken, String> {
    match token {
        LineContainer::Token(t) => Ok(t),
        _ => Err("Expected LineToken, found Container".to_string()),
    }
}

/// Recursively gather all LineTokens contained within a LineContainer tree.
///
/// The tokenizer already encodes indentation structure via nested
/// `LineContainer::Container` nodes, so verbatim content that spans multiple
/// indentation levels needs to be flattened before we hand the tokens to the
/// shared AST builders. We keep every nested line (including those that contain
/// inline `::` markers) so verbatim blocks rely on dedent boundaries instead of
/// mistaking inline markers for closing annotations.
pub(super) fn collect_line_tokens(container: &LineContainer, out: &mut Vec<LineToken>) {
    match container {
        LineContainer::Token(token) => out.push(token.clone()),
        LineContainer::Container { children } => {
            for child in children {
                collect_line_tokens(child, out);
            }
        }
    }
}

/// Extract header tokens from an annotation start line.
/// Header tokens are all tokens between the two structural :: markers
/// (excluding the structural markers, but preserving any :: inside quoted values).
pub(super) fn extract_annotation_header_tokens(
    start_token: &LineToken,
) -> Result<Vec<(Token, std::ops::Range<usize>)>, String> {
    let all_tokens: Vec<_> = start_token
        .source_tokens
        .clone()
        .into_iter()
        .zip(start_token.token_spans.clone())
        .collect();

    let structural = find_structural_lex_marker_pairs(&all_tokens);

    // Filter out only structural LexMarker positions
    let header_tokens: Vec<_> = all_tokens
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !structural.contains(i))
        .map(|(_, pair)| pair)
        .collect();

    ensure_header_has_label(start_token, &header_tokens)?;
    Ok(header_tokens)
}

#[derive(Debug, Clone)]
pub(super) struct AnnotationHeaderAndContent {
    pub header_tokens: Vec<(Token, Range<usize>)>,
    pub children: Vec<crate::lex::parsing::ir::ParseNode>,
}

/// Extract content from an annotation single-line form.
/// Returns (header_tokens, content_children) where content_children is either empty
/// or contains a single Paragraph node with the inline content.
///
/// Uses quote-aware marker detection so `::` inside quoted parameter values
/// is preserved as header content, not treated as a structural delimiter.
pub(super) fn extract_annotation_single_content(
    start_token: &LineToken,
) -> Result<AnnotationHeaderAndContent, String> {
    use crate::lex::parsing::ir::{NodeType, ParseNode};

    let all_tokens: Vec<_> = start_token
        .source_tokens
        .clone()
        .into_iter()
        .zip(start_token.token_spans.clone())
        .collect();

    let structural = find_structural_lex_marker_pairs(&all_tokens);

    // The second structural marker separates header from content
    let second_marker_idx = structural.get(1).copied();

    let mut header_tokens = Vec::new();
    let mut content_tokens = Vec::new();

    for (i, (token, span)) in all_tokens.into_iter().enumerate() {
        if structural.contains(&i) {
            // Skip structural markers (opening and closing ::)
            continue;
        }

        if let Some(boundary) = second_marker_idx {
            if i < boundary {
                header_tokens.push((token, span));
            } else {
                content_tokens.push((token, span));
            }
        } else {
            header_tokens.push((token, span));
        }
    }

    ensure_header_has_label(start_token, &header_tokens)?;

    // If there's content after the header, create a paragraph for it
    let children = if !content_tokens.is_empty() {
        vec![ParseNode::new(NodeType::Paragraph, content_tokens, vec![])]
    } else {
        vec![]
    };

    Ok(AnnotationHeaderAndContent {
        header_tokens,
        children,
    })
}

fn ensure_header_has_label(
    start_token: &LineToken,
    header_tokens: &[(Token, Range<usize>)],
) -> Result<(), String> {
    let analysis = analyze_annotation_header_token_pairs(header_tokens);
    if analysis.has_label {
        return Ok(());
    }

    let byte = header_tokens
        .first()
        .map(|(_, span)| span.start)
        .or_else(|| start_token.token_spans.first().map(|span| span.start))
        .unwrap_or(0);

    Err(format!(
        "Annotation starting at byte {byte} must include a label before any parameters"
    ))
}
