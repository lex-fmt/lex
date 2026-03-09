//! Property-based "confusion boundary" tests
//!
//! These tests generate content that is near the boundary between element types,
//! verifying the parser does NOT misclassify elements. This is the highest
//! bug-finding-potential category: inputs that look like one element but should
//! parse as another.

use lex_core::lex::ast::ContentItem;
use lex_core::lex::parsing::parse_document;
use lex_core::lex::testing::assert_ast;
use proptest::prelude::*;

// =============================================================================
// Strategies for "confusing" content
// =============================================================================

/// Text that ends with a colon but should NOT be a definition (no indented child)
fn text_ending_with_colon() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ [a-z]+ [a-z]+:"
}

/// Text that starts with a dash but should be paragraph text (not a list)
fn text_starting_with_dash() -> impl Strategy<Value = String> {
    "- [a-z]+ [a-z]+"
}

/// Text that contains :: but is not an annotation
fn text_with_double_colon() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ :: [a-z]+ [a-z]+"
}

/// Text that looks like a numbered marker but is inside a paragraph
fn text_with_number_dot() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ 1\\. [a-z]+ [a-z]+"
}

fn paragraph_line() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ [a-z]+ [a-z]+[.]"
}

/// Paragraph text containing colons mid-line (should remain paragraph, not trigger definition)
fn text_with_mid_colons() -> impl Strategy<Value = String> {
    prop_oneof![
        "[A-Z][a-z]+: [a-z]+ [a-z]+[.]",
        "[A-Z][a-z]+ [a-z]+: [a-z]+ [a-z]+[.]",
        "[A-Z][a-z]+: [a-z]+: [a-z]+[.]",
    ]
}

/// Paragraph text containing dashes (should remain paragraph, not trigger list)
fn text_with_dashes() -> impl Strategy<Value = String> {
    prop_oneof![
        "[A-Z][a-z]+ - [a-z]+ [a-z]+[.]",
        "[A-Z][a-z]+-[a-z]+ [a-z]+[.]",
        "[A-Z][a-z]+ [a-z]+ -- [a-z]+[.]",
    ]
}

fn subject_strategy() -> impl Strategy<Value = String> {
    "[A-Z][a-zA-Z0-9 ]{1,20}"
        .prop_map(|s| s.trim_end().to_string())
        .prop_filter("must not end with colon or be empty", |s| {
            !s.ends_with(':') && !s.is_empty()
        })
}

fn label_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,8}"
}

// =============================================================================
// 1. Colon-ending text without children → NOT a definition
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn colon_text_without_indented_child_is_paragraph(
        text in text_ending_with_colon(),
    ) {
        // Text ending with colon but no indented content after → paragraph
        let source = format!("{text}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        // Should be paragraph OR session with no children — not a definition
        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        for item in &items {
            assert!(
                !matches!(item, ContentItem::Definition(_)),
                "Text ending with colon but no indented child should NOT be a Definition\nSource:\n{source}"
            );
        }
    }

    #[test]
    fn colon_text_followed_by_unindented_text_is_not_definition(
        text in text_ending_with_colon(),
        next in paragraph_line(),
    ) {
        // Text with colon followed by non-indented text → two separate items
        let source = format!("{text}\n{next}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        for item in &items {
            assert!(
                !matches!(item, ContentItem::Definition(_)),
                "Colon text followed by unindented text should NOT be a Definition\nSource:\n{source}"
            );
        }
    }
}

// =============================================================================
// 1b. Colons and dashes mid-text should not trigger false detection
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn mid_colon_text_stays_paragraph(text in text_with_mid_colons()) {
        // Colons in the middle of running text → still paragraph
        let source = format!("{text}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        // At root level, this might parse as session if colon is at end.
        // But mid-colon text should never produce a Definition.
        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        for item in &items {
            assert!(
                !matches!(item, ContentItem::Definition(_)),
                "Mid-colon text should NOT be a Definition\nSource:\n{source}"
            );
        }
    }

    #[test]
    fn dash_mid_text_stays_paragraph(text in text_with_dashes()) {
        // Dashes in the middle of text → still paragraph, not list
        let source = format!("{text}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        for item in &items {
            assert!(
                !matches!(item, ContentItem::List(_)),
                "Dash in middle of text should NOT be a List\nSource:\n{source}"
            );
        }
    }

    #[test]
    fn double_colon_sequence_mid_text_not_annotation(
        prefix in "[A-Z][a-z]+",
        suffix in "[a-z]+ [a-z]+[.]",
    ) {
        // :: in the middle of a word or sentence
        let source = format!("{prefix}::{suffix}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        for item in &items {
            assert!(
                !matches!(item, ContentItem::Annotation(_)),
                ":: mid-word should NOT be an Annotation\nSource:\n{source}"
            );
        }
    }
}

// =============================================================================
// 2. Dash text not preceded by blank line → NOT a list (at root)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn dash_after_paragraph_without_blank_line_is_not_list(
        intro in paragraph_line(),
        dash_text in text_starting_with_dash(),
    ) {
        // Paragraph followed immediately by dash text (no blank line) → paragraph continues
        let source = format!("{intro}\n{dash_text}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        // Should not have a List as a top-level item
        let list_count = items
            .iter()
            .filter(|item| matches!(item, ContentItem::List(_)))
            .count();
        assert_eq!(
            list_count, 0,
            "Dash text not preceded by blank line should NOT be a List\nSource:\n{source}"
        );
    }

    #[test]
    fn dash_after_blank_line_with_two_items_is_list(
        item1 in "[A-Z][a-z]+ [a-z]+",
        item2 in "[A-Z][a-z]+ [a-z]+",
    ) {
        // Blank line + two dash items → should be a list
        let source = format!("\n- {item1}\n- {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let has_list = items
            .iter()
            .any(|item| matches!(item, ContentItem::List(_)));
        assert!(
            has_list,
            "Dash items preceded by blank line should be a List\nSource:\n{source}"
        );
    }
}

// =============================================================================
// 3. Double-colon content that is NOT an annotation
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn double_colon_mid_text_is_not_annotation(
        text in text_with_double_colon(),
    ) {
        // Text with :: in the middle → paragraph, not annotation
        let source = format!("{text}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        for item in &items {
            assert!(
                !matches!(item, ContentItem::Annotation(_)),
                ":: in middle of text should NOT be an Annotation\nSource:\n{source}"
            );
        }
    }
}

// =============================================================================
// 4. Number-dot in running text → NOT a list
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn number_dot_mid_sentence_is_not_list(
        text in text_with_number_dot(),
    ) {
        let source = format!("{text}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        for item in &items {
            assert!(
                !matches!(item, ContentItem::List(_)),
                "Number-dot inside a sentence should NOT be a List\nSource:\n{source}"
            );
        }
    }
}

// =============================================================================
// 5. Definition vs Session disambiguation edge cases
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn multiple_blank_lines_still_session(
        subject in subject_strategy(),
        content in paragraph_line(),
        blank_count in 2..5usize,
    ) {
        // Multiple blank lines between subject: and indented content → still session
        let blanks = "\n".repeat(blank_count);
        let source = format!("{subject}:\n{blanks}    {content}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session();
            });
    }

    #[test]
    fn definition_requires_immediate_indent(
        subject in subject_strategy(),
        content in paragraph_line(),
    ) {
        // Subject: followed IMMEDIATELY by indented content (no blank line) → definition
        let source = format!("{subject}:\n    {content}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_definition()
                    .subject(&subject)
                    .child_count(1);
            });
    }
}

// =============================================================================
// 6. :: inside verbatim should NOT trigger annotation/closing detection
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn double_colon_inside_verbatim_is_preserved(
        verbatim_subject in subject_strategy(),
        label in label_strategy(),
    ) {
        // Content with :: inside a verbatim block should be preserved as-is
        let source = format!(
            "{verbatim_subject}:\n    def foo :: bar\n    :: baz ::\n:: {label} ::\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_verbatim_block()
                    .subject(&verbatim_subject)
                    .closing_label(&label)
                    .content_contains("def foo :: bar");
            });
    }
}

// =============================================================================
// 7. Unicode text should parse cleanly as paragraphs
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn unicode_paragraph_text(
        text in prop_oneof![
            // CJK
            "[\u{4e00}-\u{4e10}]{2,10}",
            // Accented latin
            "[a-z\u{00e0}-\u{00ff}]{2,15}",
            // Mixed
            "[A-Z][a-z]+ [\u{4e00}-\u{4e10}]{2,5} [a-z]+",
        ]
    ) {
        let source = format!("{text}\n");
        let doc = parse_document(&source);
        // Should not panic — may or may not parse as paragraph depending on content
        assert!(doc.is_ok(), "Unicode text should not cause parse failure: {source}");
    }
}

// =============================================================================
// 8. Multi-line paragraphs (text continuation without structure)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn multi_line_paragraph(
        line1 in paragraph_line(),
        line2 in paragraph_line(),
        line3 in paragraph_line(),
    ) {
        // Multiple consecutive text lines form a single paragraph
        let source = format!("{line1}\n{line2}\n{line3}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_paragraph()
                    .line_count(3);
            });
    }

    #[test]
    fn blank_line_separates_paragraphs(
        line1 in paragraph_line(),
        line2 in paragraph_line(),
    ) {
        // Inside a session, blank line creates separate paragraphs
        let source = format!("Title:\n\n    {line1}\n\n    {line2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        // Session should contain at least 2 paragraphs
        let session = doc.root.children.iter()
            .find_map(|i| i.as_session())
            .expect("Expected session");
        let para_count = session.children.iter()
            .filter(|c| matches!(c, ContentItem::Paragraph(_)))
            .count();
        assert_eq!(para_count, 2,
            "Expected 2 paragraphs separated by blank line\nSource:\n{source}");
    }
}
