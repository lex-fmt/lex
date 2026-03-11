//! Transform integration for lex-babel formats
//!
//! This module provides transform-style interfaces for format conversions.
//! While lex-parser provides the core transform infrastructure, lex-babel
//! adds serialization transforms that operate on AST nodes.

use crate::format::Format;
use crate::formats::lex::formatting_rules::FormattingRules;
use crate::formats::lex::LexFormat;
use lex_core::lex::ast::elements::typed_content::ContentElement;
use lex_core::lex::ast::{ContentItem, Document, List, ListItem, Session};

/// Serialize a Document to Lex format with default formatting rules
///
/// This provides a simple functional interface that can be used
/// in transform-style pipelines outside the standard lex-parser transforms.
///
/// # Example
///
/// ```
/// use lex_babel::transforms::serialize_to_lex;
/// use lex_core::lex::transforms::standard::STRING_TO_AST;
///
/// let source = "Hello world\n";
/// let doc = STRING_TO_AST.run(source.to_string()).unwrap();
/// let formatted = serialize_to_lex(&doc).unwrap();
/// assert_eq!(formatted, "Hello world\n");
/// ```
pub fn serialize_to_lex(doc: &Document) -> Result<String, String> {
    let format = LexFormat::default();
    format.serialize(doc).map_err(|e| e.to_string())
}

/// Serialize a Document to Lex format with custom formatting rules
///
/// # Example
///
/// ```
/// use lex_babel::transforms::serialize_to_lex_with_rules;
/// use lex_babel::formats::lex::formatting_rules::FormattingRules;
/// use lex_core::lex::transforms::standard::STRING_TO_AST;
///
/// let source = "Hello world\n";
/// let doc = STRING_TO_AST.run(source.to_string()).unwrap();
///
/// let mut rules = FormattingRules::default();
/// rules.indent_string = "  ".to_string(); // 2-space indent
///
/// let formatted = serialize_to_lex_with_rules(&doc, rules).unwrap();
/// ```
pub fn serialize_to_lex_with_rules(
    doc: &Document,
    rules: FormattingRules,
) -> Result<String, String> {
    let format = LexFormat::new(rules);
    format.serialize(doc).map_err(|e| e.to_string())
}

/// Round-trip transformation: parse and re-serialize
///
/// Useful for formatting operations and testing.
///
/// # Example
///
/// ```
/// use lex_babel::transforms::format_lex_source;
///
/// let source = "Hello world\n";
/// let formatted = format_lex_source(source).unwrap();
/// assert_eq!(formatted, "Hello world\n");
/// ```
pub fn format_lex_source(source: &str) -> Result<String, String> {
    use lex_core::lex::transforms::standard::STRING_TO_AST;

    let mut doc = STRING_TO_AST
        .run(source.to_string())
        .map_err(|e| e.to_string())?;

    normalize_footnotes(&mut doc);

    serialize_to_lex(&doc)
}

/// Normalizes footnote definitions in a document from session-based format to list-based format.
///
/// Lex supports two formats for footnotes:
/// 1. **Session-based** (legacy): Each note is a child session with title "1. Note content"
/// 2. **List-based** (preferred): Notes are list items within a Notes/Footnotes session
///
/// This function converts session-based footnotes to list-based format during formatting,
/// producing cleaner, more compact output.
fn normalize_footnotes(doc: &mut Document) {
    if let Some(ContentItem::Session(last_session)) = doc.root.children.as_mut_vec().last_mut() {
        let title = last_session.title.as_string();
        if title.trim().eq_ignore_ascii_case("Notes")
            || title.trim().eq_ignore_ascii_case("Footnotes")
        {
            convert_session_notes_to_list(last_session);
        }
    }
}

/// Converts session-based footnote children to a single list.
///
/// Handles three content types within a Notes session:
/// - **Numbered sessions** (e.g., "1. Note"): Converted to list items
/// - **Existing lists**: Items are merged into the output list
/// - **Blank lines**: Removed to compact the output
fn convert_session_notes_to_list(session: &mut Session) {
    let has_legacy_content = session.children.iter().any(|c| match c {
        ContentItem::Session(s) => split_numbered_title(s.title.as_string()).is_some(),
        ContentItem::List(_) | ContentItem::BlankLineGroup(_) => true,
        _ => false,
    });

    if !has_legacy_content {
        return;
    }

    let mut new_children = Vec::new();
    let mut current_list_items = Vec::new();

    // Drain children from the session
    let children_vec = session.children.as_mut_vec();
    let old_children = std::mem::take(children_vec);

    for mut child in old_children {
        // handle Session -> ListItem
        let mut handled = false;

        if let ContentItem::Session(inner_session) = &child {
            let title = inner_session.title.as_string();
            if let Some((number_part, content_part)) = split_numbered_title(title) {
                handled = true;

                let mut children_elements = Vec::new();
                for inner_child in inner_session.children.iter().cloned() {
                    if let Ok(el) = ContentElement::try_from(inner_child) {
                        children_elements.push(el);
                    }
                }

                let list_item = ListItem::with_content(
                    number_part.to_string(),
                    content_part.trim().to_string(),
                    children_elements,
                );
                current_list_items.push(list_item);
            }
        } else if let ContentItem::List(l) = &mut child {
            // Merge list items
            handled = true;
            // We need to extract items. ListContainer wraps generic content but typically ListContent::ListItem.
            // We'll iterate and filter/map.
            let items = std::mem::take(l.items.as_mut_vec());
            for item in items {
                if let ContentItem::ListItem(li) = item {
                    current_list_items.push(li);
                }
                // If it's not a ListItem (e.g. comment), we drop it for now as per refactoring goal "Clean List".
            }
        } else if let ContentItem::BlankLineGroup(_) = child {
            // Skip blank lines in Notes session to compact them
            handled = true;
        }

        if !handled {
            // If we encounter something else (e.g. Paragraph), we assume it breaks the list or is a preamble.
            // Flush current items first.
            if !current_list_items.is_empty() {
                new_children.push(ContentItem::List(List::new(std::mem::take(
                    &mut current_list_items,
                ))));
            }
            new_children.push(child);
        }
    }

    // Flush remaining
    if !current_list_items.is_empty() {
        new_children.push(ContentItem::List(List::new(current_list_items)));
    }

    *session.children.as_mut_vec() = new_children;
}

/// Splits a numbered title like "1. Note Title" into its marker and content parts.
///
/// Returns `Some(("1.", " Note Title"))` for valid numbered titles, `None` otherwise.
/// The marker includes the trailing dot to preserve the original format for list item creation.
fn split_numbered_title(title: &str) -> Option<(&str, &str)> {
    let title = title.trim();
    let number_len = title.chars().take_while(|c| c.is_ascii_digit()).count();
    if number_len > 0 && title.chars().nth(number_len) == Some('.') {
        let (num, rest) = title.split_at(number_len + 1);
        return Some((num, rest));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::ast::Paragraph;

    #[test]
    fn test_serialize_to_lex() {
        let doc = Document::with_content(vec![ContentItem::Paragraph(Paragraph::from_line(
            "Test".to_string(),
        ))]);

        let result = serialize_to_lex(&doc);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Test\n");
    }

    #[test]
    fn test_serialize_with_custom_rules() {
        let doc = Document::with_content(vec![ContentItem::Paragraph(Paragraph::from_line(
            "Test".to_string(),
        ))]);

        let rules = FormattingRules {
            indent_string: "  ".to_string(),
            ..Default::default()
        };

        let result = serialize_to_lex_with_rules(&doc, rules);
        assert!(result.is_ok());
    }

    #[test]
    fn test_format_lex_source() {
        let source = "Hello world\n";
        let formatted = format_lex_source(source);
        assert!(formatted.is_ok());
        assert_eq!(formatted.unwrap(), "Hello world\n");
    }

    #[test]
    fn test_round_trip_simple() {
        let original = "Introduction\n\n    This is a session.\n";
        let formatted = format_lex_source(original).unwrap();

        // Parse both and compare (structural equivalence)
        use lex_core::lex::transforms::standard::STRING_TO_AST;

        let doc1 = STRING_TO_AST.run(original.to_string()).unwrap();
        let doc2 = STRING_TO_AST.run(formatted.clone()).unwrap();

        // Both should parse successfully
        assert_eq!(doc1.root.children.len(), doc2.root.children.len());
    }

    #[test]
    fn test_normalize_footnotes() {
        let original = "Title\n\n    Content\n\nNotes\n\n    1. Note One\n\n    2. Note Two\n";
        // This parses as Session("Notes") -> [Session("1. Note One"), Session("2. Note Two")]
        // normally, but we want it to become a List.
        let formatted = format_lex_source(original).unwrap();

        // Verification
        use lex_core::lex::transforms::standard::STRING_TO_AST;

        let doc = STRING_TO_AST.run(formatted.clone()).unwrap();
        let last_session = doc.root.children.last().unwrap();
        if let ContentItem::Session(s) = last_session {
            assert_eq!(s.title.as_string().trim(), "Notes");
            assert_eq!(s.children.len(), 1);
            if let ContentItem::List(l) = &s.children[0] {
                assert_eq!(l.items.len(), 2);
                if let ContentItem::ListItem(item) = &l.items[0] {
                    assert_eq!(item.marker().trim(), "1.");
                    assert_eq!(item.text().trim(), "Note One");
                } else {
                    panic!("Expected ListItem, found {:?}", l.items[0]);
                }
            } else {
                panic!("Expected List, found {:?}", s.children[0]);
            }
        } else {
            panic!("Expected Session");
        }
    }
}
