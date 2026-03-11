//! Session Data Extraction
//!
//! Extracts primitive data (text, byte ranges) from normalized token vectors
//! for building Session AST nodes.

use crate::lex::ast::elements::sequence_marker::{DecorationStyle, Form, Separator};
use crate::lex::lexing::line_classification::parse_seq_marker;
use crate::lex::token::normalization::utilities::{compute_bounding_box, extract_text};
use crate::lex::token::Token;
use std::ops::Range as ByteRange;

/// Extracted marker data for a session
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct SessionMarkerData {
    pub text: String,
    pub byte_range: ByteRange<usize>,
    pub style: DecorationStyle,
    pub separator: Separator,
    pub form: Form,
}

/// Extracted data for building a Session AST node.
///
/// Contains the title text (stripped of marker) and optional marker data.
#[derive(Debug, Clone)]
pub(in crate::lex::building) struct SessionData {
    /// The session title text (without marker)
    pub title_text: String,
    /// Byte range of the title (without marker)
    pub title_byte_range: ByteRange<usize>,
    /// Optional sequence marker data
    pub marker: Option<SessionMarkerData>,
}

/// Extract session data from title tokens.
///
/// # Arguments
///
/// * `tokens` - Normalized token vector for the session title
/// * `source` - The original source string
///
/// # Returns
///
/// SessionData containing the parsed title and optional marker
pub(in crate::lex::building) fn extract_session_data(
    tokens: Vec<(Token, ByteRange<usize>)>,
    source: &str,
) -> SessionData {
    // Try to parse a marker from the tokens
    // We reuse parse_seq_marker because the syntax is identical,
    // but we must filter out Plain style (dashes) which are not valid for sessions.
    let token_refs: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();
    let parsed_marker = parse_seq_marker(&token_refs);

    if let Some(pm) = parsed_marker {
        // Sessions do not support Plain markers (dashes)
        if pm.style != DecorationStyle::Plain {
            // We have a valid session marker
            let marker_tokens = &tokens[pm.marker_start..pm.marker_end];
            let marker_byte_range = compute_bounding_box(marker_tokens);
            let marker_text = extract_text(marker_byte_range.clone(), source);

            let marker_data = SessionMarkerData {
                text: marker_text,
                byte_range: marker_byte_range,
                style: pm.style,
                separator: pm.separator,
                form: pm.form,
            };

            // Title includes EVERYTHING (marker + separator + body) for backward compatibility
            let title_byte_range = compute_bounding_box(&tokens);
            let title_text = extract_text(title_byte_range.clone(), source);

            return SessionData {
                title_text,
                title_byte_range,
                marker: Some(marker_data),
            };
        }
    }

    // No valid marker found, treat everything as title
    let title_byte_range = compute_bounding_box(&tokens);
    let title_text = extract_text(title_byte_range.clone(), source);

    SessionData {
        title_text,
        title_byte_range,
        marker: None,
    }
}
