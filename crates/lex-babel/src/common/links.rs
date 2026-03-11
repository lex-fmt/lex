//! Generic helpers for converting between Lex references and format-specific links with anchors.
//!
//! Lex does not have link anchors - it only has references like `[url]`.
//! Many formats (HTML, Markdown) have links with anchor text like `<a href="url">text</a>`.
//!
//! This module provides generic helpers for:
//! - **Export**: Extract a word before/after a reference to use as the anchor
//! - **Import**: Insert anchor text + reference into inline content
//!
//! See README.lex section 0.6.1 for the full specification.

use crate::ir::nodes::InlineContent;

/// Extract anchor text and href from inline content for a reference at the given position.
///
/// Rules:
/// - Takes the word BEFORE the reference as anchor (preferred)
/// - If reference is first, takes the word AFTER it
/// - Returns (anchor_text, href, modified_content) where the anchor word is removed from content
///
/// # Example
///
/// ```
/// use lex_babel::common::links::extract_anchor_for_reference;
/// use lex_babel::ir::nodes::InlineContent;
///
/// let content = vec![
///     InlineContent::Text("visit the ".to_string()),
///     InlineContent::Text("bahamas".to_string()),
///     InlineContent::Text(" ".to_string()),
///     InlineContent::Reference("bahamas.gov".to_string()),
/// ];
///
/// let result = extract_anchor_for_reference(&content, 3);
/// assert!(result.is_some());
/// let (anchor, href, modified) = result.unwrap();
/// assert_eq!(anchor, "bahamas");
/// assert_eq!(href, "bahamas.gov");
/// ```
pub fn extract_anchor_for_reference(
    content: &[InlineContent],
    ref_index: usize,
) -> Option<(String, String, Vec<InlineContent>)> {
    if ref_index >= content.len() {
        return None;
    }

    let reference = match &content[ref_index] {
        InlineContent::Reference(href) => href.clone(),
        _ => return None,
    };

    // Try to find the word before the reference
    if let Some((anchor, modified)) = extract_word_before(content, ref_index) {
        return Some((anchor, reference, modified));
    }

    // If reference is first or no word before, try word after
    if let Some((anchor, modified)) = extract_word_after(content, ref_index) {
        return Some((anchor, reference, modified));
    }

    // No suitable anchor found - use the URL as both anchor and href
    let mut modified = content.to_vec();
    modified.remove(ref_index);
    Some((reference.clone(), reference, modified))
}

/// Extract the last word from text content before the given index.
/// Returns (word, modified_content) where the word is removed.
fn extract_word_before(
    content: &[InlineContent],
    ref_index: usize,
) -> Option<(String, Vec<InlineContent>)> {
    // Look backwards from ref_index for Text content
    for i in (0..ref_index).rev() {
        if let InlineContent::Text(text) = &content[i] {
            // Extract last word from this text
            let trimmed = text.trim_end();
            if trimmed.is_empty() {
                continue;
            }

            // Find the last word boundary
            let last_space = trimmed.rfind(char::is_whitespace);
            let (prefix, word) = match last_space {
                Some(pos) => (&trimmed[..=pos], &trimmed[pos + 1..]),
                None => ("", trimmed),
            };

            if word.is_empty() {
                continue;
            }

            // Build modified content
            let mut modified = Vec::new();
            for (idx, item) in content.iter().enumerate() {
                if idx == i {
                    // Replace with prefix (text before the word)
                    if !prefix.is_empty() {
                        modified.push(InlineContent::Text(prefix.to_string()));
                    }
                } else if idx != ref_index {
                    modified.push(item.clone());
                }
            }

            return Some((word.to_string(), modified));
        }
    }

    None
}

/// Extract the first word from text content after the given index.
/// Returns (word, modified_content) where the word is removed.
fn extract_word_after(
    content: &[InlineContent],
    ref_index: usize,
) -> Option<(String, Vec<InlineContent>)> {
    // Look forwards from ref_index for Text content
    for i in (ref_index + 1)..content.len() {
        if let InlineContent::Text(text) = &content[i] {
            // Extract first word from this text
            let trimmed = text.trim_start();
            if trimmed.is_empty() {
                continue;
            }

            // Find the first word boundary
            let first_space = trimmed.find(char::is_whitespace);
            let (word, suffix) = match first_space {
                Some(pos) => (&trimmed[..pos], &trimmed[pos..]),
                None => (trimmed, ""),
            };

            if word.is_empty() {
                continue;
            }

            // Build modified content
            let mut modified = Vec::new();
            for (idx, item) in content.iter().enumerate() {
                if idx == i {
                    // Replace with suffix (text after the word)
                    if !suffix.is_empty() {
                        modified.push(InlineContent::Text(suffix.to_string()));
                    }
                } else if idx != ref_index {
                    modified.push(item.clone());
                }
            }

            return Some((word.to_string(), modified));
        }
    }

    None
}

/// Insert anchor text and reference into inline content.
/// The anchor text is inserted as Text, followed by a space, followed by the Reference.
///
/// # Example
///
/// ```
/// use lex_babel::common::links::insert_reference_with_anchor;
/// use lex_babel::ir::nodes::InlineContent;
///
/// let content = vec![
///     InlineContent::Text("visit ".to_string()),
/// ];
///
/// let modified = insert_reference_with_anchor(content, "bahamas".to_string(), "bahamas.gov".to_string());
/// assert_eq!(modified.len(), 3);
/// ```
pub fn insert_reference_with_anchor(
    mut content: Vec<InlineContent>,
    anchor: String,
    href: String,
) -> Vec<InlineContent> {
    // Append anchor text
    content.push(InlineContent::Text(anchor));

    // Append space
    content.push(InlineContent::Text(" ".to_string()));

    // Append reference
    content.push(InlineContent::Reference(href));

    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_anchor_word_before() {
        let content = vec![
            InlineContent::Text("visit the ".to_string()),
            InlineContent::Text("bahamas ".to_string()),
            InlineContent::Reference("bahamas.gov".to_string()),
        ];

        let result = extract_anchor_for_reference(&content, 2);
        assert!(result.is_some());

        let (anchor, href, modified) = result.unwrap();
        assert_eq!(anchor, "bahamas");
        assert_eq!(href, "bahamas.gov");

        // Verify modified content doesn't include the anchor word or reference
        // The "bahamas " text element is fully consumed (trimmed to "bahamas" which is extracted)
        assert_eq!(modified.len(), 1);
        assert!(matches!(&modified[0], InlineContent::Text(t) if t == "visit the "));
    }

    #[test]
    fn test_extract_anchor_word_after() {
        let content = vec![
            InlineContent::Reference("wikipedia.org".to_string()),
            InlineContent::Text(" Wikipedia is useful".to_string()),
        ];

        let result = extract_anchor_for_reference(&content, 0);
        assert!(result.is_some());

        let (anchor, href, modified) = result.unwrap();
        assert_eq!(anchor, "Wikipedia");
        assert_eq!(href, "wikipedia.org");

        // Verify modified content
        assert_eq!(modified.len(), 1);
        assert!(matches!(&modified[0], InlineContent::Text(t) if t == " is useful"));
    }

    #[test]
    fn test_extract_anchor_no_text() {
        let content = vec![
            InlineContent::Bold(vec![InlineContent::Text("bold".to_string())]),
            InlineContent::Reference("example.com".to_string()),
        ];

        let result = extract_anchor_for_reference(&content, 1);
        assert!(result.is_some());

        let (anchor, href, _modified) = result.unwrap();
        // Should fall back to using URL as anchor
        assert_eq!(anchor, "example.com");
        assert_eq!(href, "example.com");
    }

    #[test]
    fn test_insert_reference_with_anchor() {
        let content = vec![InlineContent::Text("visit ".to_string())];

        let modified =
            insert_reference_with_anchor(content, "bahamas".to_string(), "bahamas.gov".to_string());

        assert_eq!(modified.len(), 4);
        assert!(matches!(&modified[0], InlineContent::Text(t) if t == "visit "));
        assert!(matches!(&modified[1], InlineContent::Text(t) if t == "bahamas"));
        assert!(matches!(&modified[2], InlineContent::Text(t) if t == " "));
        assert!(matches!(&modified[3], InlineContent::Reference(r) if r == "bahamas.gov"));
    }
}
