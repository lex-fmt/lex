//! Reference classification for inline elements.
//!
//! Handles detection and classification of different reference types:
//! - TK placeholders (`[TK]`, `[TK-identifier]`)
//! - Citations (`[@key]`)
//! - Session references (`[#42]`)
//! - URLs (`[https://example.com]`)
//! - File paths (`[./file.txt]`)
//! - Footnotes (`[^note]`, `[42]`)
//! - General references (`[Section Title]`)

use super::citations::parse_citation_data;
use crate::lex::ast::elements::inlines::{InlineNode, ReferenceType};

/// Post-processor callback for reference nodes that classifies their type.
pub(super) fn classify_reference_node(node: InlineNode) -> InlineNode {
    match node {
        InlineNode::Reference {
            mut data,
            annotations,
        } => {
            data.reference_type = determine_reference_type(&data.raw);
            InlineNode::Reference { data, annotations }
        }
        other => other,
    }
}

/// Determine the reference type from raw content.
fn determine_reference_type(raw: &str) -> ReferenceType {
    let trimmed = raw.trim();
    if trimmed.is_empty() || !trimmed.chars().any(|ch| ch.is_alphanumeric()) {
        return ReferenceType::NotSure;
    }

    if let Some(identifier) = detect_tk_reference(trimmed) {
        return ReferenceType::ToCome { identifier };
    }

    if let Some(rest) = trimmed.strip_prefix('@') {
        if let Some(citation) = parse_citation_data(rest) {
            return ReferenceType::Citation(citation);
        }
    }

    if let Some(rest) = trimmed.strip_prefix('^') {
        if !rest.is_empty() {
            return ReferenceType::FootnoteLabeled {
                label: rest.to_string(),
            };
        }
    }

    if let Some(session_target) = parse_session_reference(trimmed) {
        return ReferenceType::Session {
            target: session_target,
        };
    }

    if is_url_reference(trimmed) {
        return ReferenceType::Url {
            target: trimmed.to_string(),
        };
    }

    if is_file_reference(trimmed) {
        return ReferenceType::File {
            target: trimmed.to_string(),
        };
    }

    if let Some(number) = parse_footnote_number(trimmed) {
        return ReferenceType::FootnoteNumber { number };
    }

    ReferenceType::General {
        target: trimmed.to_string(),
    }
}

/// Detect TK placeholder references.
///
/// - `[TK]` → ToCome with no identifier
/// - `[TK-feature]` → ToCome with identifier "feature"
fn detect_tk_reference(trimmed: &str) -> Option<Option<String>> {
    if trimmed.eq_ignore_ascii_case("TK") {
        return Some(None);
    }

    // Check for "TK-" prefix case-insensitively using strip_prefix variants
    // We need to check both cases since strip_prefix is case-sensitive
    let identifier = trimmed
        .strip_prefix("TK-")
        .or_else(|| trimmed.strip_prefix("tk-"))
        .or_else(|| trimmed.strip_prefix("Tk-"))
        .or_else(|| trimmed.strip_prefix("tK-"));

    if let Some(identifier) = identifier {
        if !identifier.is_empty()
            && identifier.len() <= 20
            && identifier
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return Some(Some(identifier.to_string()));
        }
    }
    None
}

/// Parse session reference from content starting with `#`.
///
/// Example: `[#2.1]` → Session reference to "2.1"
fn parse_session_reference(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix('#')?;
    if rest.is_empty() {
        return None;
    }
    if rest
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
    {
        Some(rest.to_string())
    } else {
        None
    }
}

/// Check if the reference is a URL.
fn is_url_reference(trimmed: &str) -> bool {
    trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("mailto:")
}

/// Check if the reference is a file path.
fn is_file_reference(trimmed: &str) -> bool {
    trimmed.starts_with('.') || trimmed.starts_with('/')
}

/// Parse numeric footnote reference.
///
/// Example: `[42]` → FootnoteNumber { number: 42 }
fn parse_footnote_number(trimmed: &str) -> Option<u32> {
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        trimmed.parse::<u32>().ok()
    } else {
        None
    }
}
