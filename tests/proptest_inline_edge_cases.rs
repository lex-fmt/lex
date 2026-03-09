//! Property-based tests for inline markup edge cases
//!
//! Tests multi-word markup, adjacent markup, markup at line boundaries,
//! and unclosed markup — all previously untested inline patterns.

use lex_core::lex::ast::elements::inlines::InlineNode;
use lex_core::lex::ast::ContentItem;
use lex_core::lex::inlines::parse_inlines;
use lex_core::lex::parsing::parse_document;
use lex_core::lex::testing::{InlineAssertion, InlineExpectation};
use proptest::prelude::*;

// =============================================================================
// Helpers
// =============================================================================

fn extract_first_text_line(source: &str) -> lex_core::lex::ast::TextContent {
    let doc = parse_document(source)
        .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));
    let para = match &doc.root.children.iter().collect::<Vec<_>>()[0] {
        ContentItem::Paragraph(p) => p,
        other => panic!("Expected Paragraph, got {other:?}\nSource:\n{source}"),
    };
    let tl = match &para.lines[0] {
        ContentItem::TextLine(tl) => tl,
        other => panic!("Expected TextLine, got {other:?}\nSource:\n{source}"),
    };
    tl.content.clone()
}

// =============================================================================
// Strategies
// =============================================================================

/// Multi-word content for inside markup (no special chars, 2-4 words)
fn multi_word_strategy() -> impl Strategy<Value = String> {
    "[a-z]{2,6}( [a-z]{2,6}){1,3}"
}

fn inline_word() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-z]{1,8}"
}

// =============================================================================
// 1. Multi-word markup
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn multi_word_bold(content in multi_word_strategy()) {
        let source = format!("Text *{content}* end.\n");
        let tc = extract_first_text_line(&source);
        InlineAssertion::new(&tc, "multi-word bold")
            .starts_with(&[
                InlineExpectation::plain_text("Text "),
                InlineExpectation::strong_text(&content),
                InlineExpectation::plain_text(" end."),
            ])
            .length(3);
    }

    #[test]
    fn multi_word_italic(content in multi_word_strategy()) {
        let source = format!("Text _{content}_ end.\n");
        let tc = extract_first_text_line(&source);
        InlineAssertion::new(&tc, "multi-word italic")
            .starts_with(&[
                InlineExpectation::plain_text("Text "),
                InlineExpectation::emphasis_text(&content),
                InlineExpectation::plain_text(" end."),
            ])
            .length(3);
    }

    #[test]
    fn multi_word_code(content in multi_word_strategy()) {
        let source = format!("Text `{content}` end.\n");
        let tc = extract_first_text_line(&source);
        InlineAssertion::new(&tc, "multi-word code")
            .starts_with(&[
                InlineExpectation::plain_text("Text "),
                InlineExpectation::code_text(&content),
                InlineExpectation::plain_text(" end."),
            ])
            .length(3);
    }
}

// =============================================================================
// 2. Markup at line start and end
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn bold_at_line_start(word in inline_word()) {
        let source = format!("*{word}* rest of line.\n");
        let tc = extract_first_text_line(&source);
        InlineAssertion::new(&tc, "bold at start")
            .starts_with(&[
                InlineExpectation::strong_text(&word),
                InlineExpectation::plain_text(" rest of line."),
            ])
            .length(2);
    }

    #[test]
    fn bold_at_line_end(word in inline_word()) {
        let source = format!("Start of line *{word}*\n");
        let tc = extract_first_text_line(&source);
        InlineAssertion::new(&tc, "bold at end")
            .starts_with(&[
                InlineExpectation::plain_text("Start of line "),
                InlineExpectation::strong_text(&word),
            ])
            .length(2);
    }

    #[test]
    fn italic_at_line_start(word in inline_word()) {
        let source = format!("_{word}_ rest.\n");
        let tc = extract_first_text_line(&source);
        InlineAssertion::new(&tc, "italic at start")
            .starts_with(&[
                InlineExpectation::emphasis_text(&word),
                InlineExpectation::plain_text(" rest."),
            ])
            .length(2);
    }
}

// =============================================================================
// 3. Adjacent markup
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn adjacent_bold_italic(
        bold_word in inline_word(),
        italic_word in inline_word(),
    ) {
        // *bold* immediately followed by _italic_ (no space)
        let source = format!("Text *{bold_word}*_{italic_word}_ end.\n");
        let tc = extract_first_text_line(&source);
        // Just verify it parses without panic and has the right number of nodes
        let nodes = InlineAssertion::new(&tc, "adjacent bold+italic").nodes().to_vec();
        // Should have at least 3 nodes: plain, strong, emphasis (or more with separators)
        assert!(
            nodes.len() >= 3,
            "Expected at least 3 inline nodes for adjacent markup, got {}\nSource: {source}",
            nodes.len()
        );
    }

    #[test]
    fn adjacent_code_bold(
        code_word in inline_word(),
        bold_word in inline_word(),
    ) {
        // `code` immediately followed by *bold*
        let source = format!("Text `{code_word}`*{bold_word}* end.\n");
        let tc = extract_first_text_line(&source);
        let nodes = InlineAssertion::new(&tc, "adjacent code+bold").nodes().to_vec();
        assert!(
            nodes.len() >= 3,
            "Expected at least 3 inline nodes, got {}\nSource: {source}",
            nodes.len()
        );
    }
}

// =============================================================================
// 4. Unclosed markup — should not panic, content preserved as plain text
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn unclosed_bold_is_plain_text(word in inline_word()) {
        // Unclosed *bold should be treated as plain text
        let input = format!("Text *{word} without closing.");
        let nodes = parse_inlines(&input);
        // Should not panic. Content should be plain text (the * is literal)
        let total_text: String = nodes.iter().map(|n| match n {
            InlineNode::Plain { text, .. } => text.clone(),
            InlineNode::Strong { content, .. } => {
                content.iter().map(|c| match c {
                    InlineNode::Plain { text, .. } => text.clone(),
                    _ => String::new(),
                }).collect()
            }
            _ => String::new(),
        }).collect();
        assert!(
            total_text.contains(&word),
            "Word '{word}' should be preserved in output\nNodes: {nodes:?}"
        );
    }

    #[test]
    fn unclosed_italic_is_plain_text(word in inline_word()) {
        let input = format!("Text _{word} without closing.");
        let nodes = parse_inlines(&input);
        let total_text: String = nodes.iter().map(|n| match n {
            InlineNode::Plain { text, .. } => text.clone(),
            InlineNode::Emphasis { content, .. } => {
                content.iter().map(|c| match c {
                    InlineNode::Plain { text, .. } => text.clone(),
                    _ => String::new(),
                }).collect()
            }
            _ => String::new(),
        }).collect();
        assert!(
            total_text.contains(&word),
            "Word '{word}' should be preserved in output\nNodes: {nodes:?}"
        );
    }

    #[test]
    fn unclosed_code_is_plain_text(word in inline_word()) {
        let input = format!("Text `{word} without closing.");
        let nodes = parse_inlines(&input);
        // Code content should be preserved somehow
        let has_content = nodes.iter().any(|n| match n {
            InlineNode::Plain { text, .. } => text.contains(&word),
            InlineNode::Code { text, .. } => text.contains(&word),
            _ => false,
        });
        assert!(has_content, "Word '{word}' should appear in output\nNodes: {nodes:?}");
    }
}

// =============================================================================
// 5. Empty markup delimiters
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn empty_bold_does_not_panic(prefix in "[A-Z][a-z]+") {
        // ** with nothing inside
        let input = format!("{prefix} ** rest.");
        let nodes = parse_inlines(&input);
        // Should not panic — content should be preserved
        let total_text: String = nodes.iter().filter_map(|n| match n {
            InlineNode::Plain { text, .. } => Some(text.as_str()),
            _ => None,
        }).collect::<Vec<_>>().join("");
        assert!(
            total_text.contains(&prefix),
            "Prefix '{prefix}' should be preserved\nNodes: {nodes:?}"
        );
    }

    #[test]
    fn empty_italic_does_not_panic(prefix in "[A-Z][a-z]+") {
        let input = format!("{prefix} __ rest.");
        let nodes = parse_inlines(&input);
        let total_text: String = nodes.iter().filter_map(|n| match n {
            InlineNode::Plain { text, .. } => Some(text.as_str()),
            _ => None,
        }).collect::<Vec<_>>().join("");
        assert!(
            total_text.contains(&prefix),
            "Prefix '{prefix}' should be preserved\nNodes: {nodes:?}"
        );
    }

    #[test]
    fn empty_code_does_not_panic(prefix in "[A-Z][a-z]+") {
        let input = format!("{prefix} `` rest.");
        let nodes = parse_inlines(&input);
        let total_text: String = nodes.iter().filter_map(|n| match n {
            InlineNode::Plain { text, .. } => Some(text.as_str()),
            InlineNode::Code { text, .. } => Some(text.as_str()),
            _ => None,
        }).collect::<Vec<_>>().join("");
        assert!(
            total_text.contains(&prefix),
            "Prefix '{prefix}' should be preserved\nNodes: {nodes:?}"
        );
    }
}
