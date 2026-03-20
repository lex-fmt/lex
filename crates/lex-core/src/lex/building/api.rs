//! Public AST Builder API
//!
//! This module provides the public API for building AST nodes from tokens.
//! It coordinates the three-layer architecture:
//!
//! 1. Token Normalization - Convert various token formats to standard vectors
//! 2. Data Extraction - Extract primitive data (text, byte ranges) from tokens
//! 3. AST Creation - Convert primitives to AST nodes with ast::Range
//!
//! # Architecture
//!
//! ```text
//! Tokens → normalize → extract → create → AST Nodes
//!   ↓          ↓          ↓          ↓
//! Parser  token_norm  data_ext  ast_create
//! ```
//!
//! # Usage
//!
//! Parsers should only use functions from this module. They should never:
//! - Extract text manually
//! - Call data_extraction functions directly
//! - Call ast_creation functions directly
//!
//! ```rust,ignore
//! use crate::lex::building::api;
//!
//! // In parser:
//! let paragraph = api::paragraph_from_line_tokens(&line_tokens, source);
//! let session_children: Vec<SessionContent> = vec![];
//! let session = api::session_from_title_token(&title_token, session_children, source);
//! ```

use crate::lex::ast::elements::typed_content::{ContentElement, SessionContent};
use crate::lex::ast::{Data, ListItem};
use crate::lex::parsing::ContentItem;
use crate::lex::token::{normalization, LineToken};

use super::ast_nodes;
use super::extraction;
use crate::lex::ast::range::SourceLocation;

// ============================================================================
// PARAGRAPH BUILDING
// ============================================================================

/// Build a Paragraph AST node from line tokens.
///
/// This is the complete pipeline: normalize → extract → create.
///
/// # Arguments
///
/// * `line_tokens` - LineTokens representing paragraph lines
/// * `source` - Original source string
///
/// # Returns
///
/// A Paragraph ContentItem
///
/// # Example
///
/// ```rust,ignore
/// let line_tokens: Vec<LineToken> = /* ... from parser ... */;
/// let paragraph = paragraph_from_line_tokens(&line_tokens, source);
/// ```
pub fn paragraph_from_line_tokens(
    line_tokens: &[LineToken],
    source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    // 1. Normalize: LineTokens → Vec<Vec<(Token, Range<usize>)>>
    let token_lines = normalization::normalize_line_tokens(line_tokens);

    // 2. Extract: normalized tokens → ParagraphData
    let data = extraction::extract_paragraph_data(token_lines, source);

    // 3. Create: ParagraphData → Paragraph AST node
    ast_nodes::paragraph_node(data, source_location)
}

// ============================================================================
// SESSION BUILDING
// ============================================================================

/// Build a Session AST node from a title token and content.
///
/// # Arguments
///
/// * `title_token` - LineToken for the session title
/// * `content` - Typed child content items (already constructed)
/// * `source` - Original source string
///
/// # Returns
///
/// A Session ContentItem
pub fn session_from_title_token(
    title_token: &LineToken,
    content: Vec<SessionContent>,
    source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    // 1. Normalize
    let tokens = normalization::normalize_line_token(title_token);

    // 2. Extract
    let data = extraction::extract_session_data(tokens, source);

    // 3. Create
    ast_nodes::session_node(data, content, source_location)
}

// ============================================================================
// DEFINITION BUILDING
// ============================================================================

/// Build a Definition AST node from a subject token and content.
///
/// # Arguments
///
/// * `subject_token` - LineToken for the definition subject
/// * `content` - Typed child content items (already constructed)
/// * `source` - Original source string
///
/// # Returns
///
/// A Definition ContentItem
pub fn definition_from_subject_token(
    subject_token: &LineToken,
    content: Vec<ContentElement>,
    source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    // 1. Normalize
    let tokens = normalization::normalize_line_token(subject_token);

    // 2. Extract
    let data = extraction::extract_definition_data(tokens, source);

    // 3. Create
    ast_nodes::definition_node(data, content, source_location)
}

// ============================================================================
// LIST BUILDING
// ============================================================================

/// Build a List AST node from list items.
///
/// # Arguments
///
/// * `items` - Vector of ListItem nodes
///
/// # Returns
///
/// A List ContentItem
pub fn list_from_items(items: Vec<ListItem>) -> ContentItem {
    // No normalization/extraction needed - items already constructed
    ast_nodes::list_node(items)
}

// ============================================================================
// LIST ITEM BUILDING
// ============================================================================

/// Build a ListItem AST node from a marker token and content.
///
/// # Arguments
///
/// * `marker_token` - LineToken for the list item marker
/// * `content` - Typed child content items (already constructed)
/// * `source` - Original source string
///
/// # Returns
///
/// A ListItem node (not wrapped in ContentItem)
pub fn list_item_from_marker_token(
    marker_token: &LineToken,
    content: Vec<ContentElement>,
    source: &str,
    source_location: &SourceLocation,
) -> ListItem {
    // 1. Normalize
    let tokens = normalization::normalize_line_token(marker_token);

    // 2. Extract
    let data = extraction::extract_list_item_data(tokens, source);

    // 3. Create
    ast_nodes::list_item_node(data, content, source_location)
}

// ============================================================================
// ANNOTATION BUILDING
// ============================================================================

/// Build an Annotation AST node from a label token and content.
///
/// Goes through the full pipeline: normalize → extract (with label/param parsing) → create.
///
/// # Arguments
///
/// * `label_token` - LineToken for the annotation label (includes label and parameters between :: markers)
/// * `content` - Typed child content items (already constructed)
/// * `source` - Original source string
///
/// # Returns
///
/// An Annotation ContentItem
pub fn annotation_from_label_token(
    label_token: &LineToken,
    content: Vec<ContentElement>,
    source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    // 1. Normalize
    let tokens = normalization::normalize_line_token(label_token);

    let data = data_from_tokens(tokens, source, source_location);

    ast_nodes::annotation_node(data, content)
}

/// Build a Data node from already-normalized tokens (no closing :: marker).
pub fn data_from_tokens(
    label_tokens: Vec<(Token, ByteRange<usize>)>,
    source: &str,
    source_location: &SourceLocation,
) -> Data {
    let data = extraction::extract_data(label_tokens, source);
    ast_nodes::data_node(data, source_location)
}

// ============================================================================
// VERBATIM BLOCK BUILDING
// ============================================================================

/// Build a VerbatimBlock AST node from subject, content, and closing data.
///
/// This function implements the indentation wall stripping logic - content at
/// different nesting levels will have identical text after wall removal.
///
/// # Arguments
///
/// * `subject_token` - LineToken for the verbatim block subject
/// * `content_tokens` - LineTokens for each content line
/// * `closing_data` - The closing data node (label + parameters)
/// * `source` - Original source string
///
/// # Returns
///
/// A VerbatimBlock ContentItem
///
/// # Example
///
/// ```text,ignore
/// // Top-level: "Code:\n    line1\n    line2\n:: js"
/// // Nested:    "Session:\n    Code:\n        line1\n        line2\n    :: js"
/// //
/// // Both produce VerbatimBlock with content: "line1\nline2"
/// // The indentation wall (minimum indentation) is stripped.
/// ```
pub fn verbatim_block_from_lines(
    subject_token: &LineToken,
    content_tokens: &[LineToken],
    closing_data: Data,
    source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    // 1. Extract (includes mode detection and indentation wall stripping)
    let data = extraction::extract_verbatim_block_data(subject_token, content_tokens, source);

    // 2. Create
    ast_nodes::verbatim_block_node(data, closing_data, source_location)
}

// ============================================================================
// TABLE BUILDING
// ============================================================================

/// Build a Table AST node from subject, content, and closing data.
///
/// Reuses verbatim block's outer structure handling (mode detection, wall stripping)
/// and adds pipe-row parsing, merge resolution, and header/alignment extraction.
///
/// # Arguments
///
/// * `subject_token` - LineToken for the table subject/caption
/// * `content_tokens` - LineTokens for each content line (pipe rows)
/// * `closing_data` - The closing data node (:: table params? ::)
/// * `source` - Original source string
///
/// # Returns
///
/// A Table ContentItem
pub fn table_from_lines(
    subject_token: &LineToken,
    content_tokens: &[LineToken],
    closing_data: Option<Data>,
    source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    // 1. Extract (reuses verbatim wall stripping + parses pipe rows)
    let data =
        extraction::extract_table_data(subject_token, content_tokens, closing_data.as_ref(), source);

    // 2. Extract alignment hints from annotation (if present)
    let alignments = closing_data
        .as_ref()
        .map(|d| extraction::table::extract_alignments(d))
        .unwrap_or_default();

    // 3. Create
    ast_nodes::table_node(data, closing_data, &alignments, source_location)
}

// ============================================================================
// NORMALIZED TOKEN API (tokens already normalized)
// ============================================================================
//
// Some callers (e.g., the parser's unwrappers) already work with
// normalized Vec<(Token, Range)> sequences. These helpers skip the
// normalization pass and go straight to data extraction/AST creation.

use crate::lex::token::Token;
use std::ops::Range as ByteRange;

/// Build a Paragraph from already-normalized token lines.
///
/// # Arguments
///
/// * `token_lines` - Normalized token vectors, one per line
/// * `source` - Original source string
///
/// # Returns
///
/// A Paragraph ContentItem
pub fn paragraph_from_token_lines(
    mut token_lines: Vec<Vec<(Token, ByteRange<usize>)>>,
    source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    if token_lines.len() == 1 {
        let mut new_token_lines = vec![];
        let mut current_line = vec![];
        for token_data in token_lines.remove(0) {
            if let Token::BlankLine(_) = token_data.0 {
                if !current_line.is_empty() {
                    new_token_lines.push(current_line);
                    current_line = vec![];
                }
            } else {
                current_line.push(token_data);
            }
        }
        if !current_line.is_empty() {
            new_token_lines.push(current_line);
        }
        token_lines = new_token_lines;
    }

    // 1. Extract
    let data = extraction::extract_paragraph_data(token_lines, source);

    // 2. Create
    ast_nodes::paragraph_node(data, source_location)
}

/// Build a Session from already-normalized title tokens.
///
/// # Arguments
///
/// * `title_tokens` - Normalized tokens for the session title
/// * `content` - Typed child content items (already constructed)
/// * `source` - Original source string
///
/// # Returns
///
/// A Session ContentItem
pub fn session_from_tokens(
    title_tokens: Vec<(Token, ByteRange<usize>)>,
    content: Vec<SessionContent>,
    source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    let filtered_tokens: Vec<_> = title_tokens
        .into_iter()
        .filter(|(token, _)| !matches!(token, Token::BlankLine(_)))
        .collect();

    // Skip normalization, tokens already normalized
    // 1. Extract
    let data = extraction::extract_session_data(filtered_tokens, source);

    // 2. Create
    ast_nodes::session_node(data, content, source_location)
}

/// Build a Definition from already-normalized subject tokens.
///
/// # Arguments
///
/// * `subject_tokens` - Normalized tokens for the definition subject
/// * `content` - Typed child content items (already constructed)
/// * `source` - Original source string
///
/// # Returns
///
/// A Definition ContentItem
pub fn definition_from_tokens(
    subject_tokens: Vec<(Token, ByteRange<usize>)>,
    content: Vec<ContentElement>,
    source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    let filtered_tokens: Vec<_> = subject_tokens
        .into_iter()
        .filter(|(token, _)| !matches!(token, Token::BlankLine(_)))
        .collect();

    // Skip normalization, tokens already normalized
    // 1. Extract
    let data = extraction::extract_definition_data(filtered_tokens, source);

    // 2. Create
    ast_nodes::definition_node(data, content, source_location)
}

/// Build a ListItem from already-normalized marker tokens.
///
/// # Arguments
///
/// * `marker_tokens` - Normalized tokens for the list item marker and text
/// * `content` - Typed child content items (already constructed)
/// * `source` - Original source string
///
/// # Returns
///
/// A ListItem node (not wrapped in ContentItem)
pub fn list_item_from_tokens(
    marker_tokens: Vec<(Token, ByteRange<usize>)>,
    content: Vec<ContentElement>,
    source: &str,
    source_location: &SourceLocation,
) -> ListItem {
    // Skip normalization, tokens already normalized
    // 1. Extract
    let data = extraction::extract_list_item_data(marker_tokens, source);

    // 2. Create
    ast_nodes::list_item_node(data, content, source_location)
}

/// Build an Annotation from already-normalized label tokens.
///
/// Skips normalization, goes through: extract (with label/param parsing) → create.
///
/// # Arguments
///
/// * `label_tokens` - Normalized tokens for the annotation label (includes label and parameters)
/// * `content` - Typed child content items (already constructed)
/// * `source` - Original source string
///
/// # Returns
///
/// An Annotation ContentItem
pub fn annotation_from_tokens(
    label_tokens: Vec<(Token, ByteRange<usize>)>,
    content: Vec<ContentElement>,
    source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    let data = data_from_tokens(label_tokens, source, source_location);
    ast_nodes::annotation_node(data, content)
}

/// Build a BlankLineGroup from already-normalized blank line tokens.
pub fn blank_line_group_from_tokens(
    tokens: Vec<(Token, ByteRange<usize>)>,
    _source: &str,
    source_location: &SourceLocation,
) -> ContentItem {
    ast_nodes::blank_line_group_node(tokens, source_location)
}

/// Build a TextContent from already-normalized tokens.
///
/// This extracts the text and location from tokens without wrapping in a ContentItem.
/// Used for extracting document titles.
///
/// # Arguments
///
/// * `tokens` - Normalized tokens for the text
/// * `source` - Original source string
/// * `source_location` - Source location helper
///
/// # Returns
///
/// A TextContent with the extracted text and location
pub fn text_content_from_tokens(
    tokens: Vec<(Token, ByteRange<usize>)>,
    source: &str,
    source_location: &SourceLocation,
) -> crate::lex::ast::text_content::TextContent {
    use crate::lex::token::normalization::utilities::{compute_bounding_box, extract_text};

    // Filter out BlankLine tokens which represent trailing newlines
    // that shouldn't be part of the text content
    let filtered_tokens: Vec<_> = tokens
        .into_iter()
        .filter(|(token, _)| !matches!(token, Token::BlankLine(_)))
        .collect();

    if filtered_tokens.is_empty() {
        return crate::lex::ast::text_content::TextContent::from_string(String::new(), None);
    }

    let byte_range = compute_bounding_box(&filtered_tokens);
    let text = extract_text(byte_range.clone(), source);
    let location = source_location.byte_range_to_ast_range(&byte_range);

    crate::lex::ast::text_content::TextContent::from_string(text, Some(location))
}

// ============================================================================
// TEXT-BASED API (for pre-extracted inputs)
// ============================================================================
//
// These functions accept pre-extracted text and ast::Range locations for tests
// or any parser variant that wants to bypass token processing entirely.

/// Build a Paragraph from pre-extracted text lines with locations.
///
/// # Arguments
///
/// * `text_lines` - Vec of (text, location) tuples for each line
/// * `overall_location` - The combined location for the entire paragraph
///
/// # Returns
///
/// A Paragraph ContentItem
pub fn paragraph_from_text_segments(
    text_lines: Vec<(String, crate::lex::ast::Range)>,
    overall_location: crate::lex::ast::Range,
) -> ContentItem {
    use crate::lex::ast::{Paragraph, TextContent, TextLine};

    let lines: Vec<ContentItem> = text_lines
        .into_iter()
        .map(|(text, location)| {
            let text_content = TextContent::from_string(text, Some(location.clone()));
            let text_line = TextLine::new(text_content).at(location);
            ContentItem::TextLine(text_line)
        })
        .collect();

    ContentItem::Paragraph(Paragraph {
        lines,
        annotations: Vec::new(),
        location: overall_location,
    })
}

/// Build a VerbatimBlock from pre-extracted text and locations.
///
/// NOTE: This does NOT perform indentation wall stripping.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::elements::typed_content::SessionContent;
    use crate::lex::ast::range::SourceLocation;
    use crate::lex::token::LineType;
    use crate::lex::token::Token;

    fn make_line_token(tokens: Vec<Token>, spans: Vec<std::ops::Range<usize>>) -> LineToken {
        LineToken {
            source_tokens: tokens,
            token_spans: spans,
            line_type: LineType::ParagraphLine,
        }
    }

    #[test]
    fn test_paragraph_from_line_tokens() {
        let source = "hello world";
        let source_location = SourceLocation::new(source);
        let line_tokens = vec![make_line_token(
            vec![
                Token::Text("hello".to_string()),
                Token::Whitespace(1),
                Token::Text("world".to_string()),
            ],
            vec![0..5, 5..6, 6..11],
        )];

        let result = paragraph_from_line_tokens(&line_tokens, source, &source_location);

        match result {
            ContentItem::Paragraph(para) => {
                assert_eq!(para.lines.len(), 1);
            }
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_session_from_title_token() {
        let source = "Session:";
        let source_location = SourceLocation::new(source);
        let title_token = make_line_token(
            vec![Token::Text("Session".to_string()), Token::Colon],
            vec![0..7, 7..8],
        );

        let result = session_from_title_token(
            &title_token,
            Vec::<SessionContent>::new(),
            source,
            &source_location,
        );

        match result {
            ContentItem::Session(session) => {
                assert_eq!(session.title.as_string(), "Session:");
            }
            _ => panic!("Expected Session"),
        }
    }
}
