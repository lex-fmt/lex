//! Property-based invariant tests
//!
//! Tests structural invariants that should hold for any parsed document:
//! - Parse idempotency: parsing the same source twice produces identical ASTs
//! - No content loss: all text content from the source appears in the AST
//! - Nesting depth matches indentation level

use lex_core::lex::ast::elements::inlines::InlineNode;
use lex_core::lex::ast::ContentItem;
use lex_core::lex::parsing::parse_document;
use proptest::prelude::*;

// =============================================================================
// Strategies for generating valid Lex documents
// =============================================================================

fn subject_strategy() -> impl Strategy<Value = String> {
    "[A-Z][a-zA-Z0-9 ]{1,15}"
        .prop_map(|s| s.trim_end().to_string())
        .prop_filter("must not end with colon or be empty", |s| {
            !s.ends_with(':') && !s.is_empty()
        })
}

fn paragraph_line() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ [a-z]+ [a-z]+[.]"
}

fn list_text() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ [a-z]+"
}

fn label_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,8}"
}

/// Generate a valid multi-element Lex document
fn valid_document_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Simple paragraph
        paragraph_line().prop_map(|p| format!("{p}\n")),
        // Definition
        (subject_strategy(), paragraph_line()).prop_map(|(s, c)| format!("{s}:\n    {c}\n")),
        // Session with paragraph
        (subject_strategy(), paragraph_line()).prop_map(|(s, c)| format!("{s}:\n\n    {c}\n")),
        // List
        (list_text(), list_text()).prop_map(|(a, b)| format!("\n- {a}\n- {b}\n")),
        // Verbatim
        (subject_strategy(), label_strategy(), "[a-zA-Z0-9]+")
            .prop_map(|(s, l, c)| format!("{s}:\n    {c}\n:: {l} ::\n")),
        // Table (compact)
        (subject_strategy(), list_text(), list_text()).prop_map(|(s, a, b)| {
            format!("{s}:\n    | H1 | H2 |\n    | {a} | {b} |\n:: table ::\n")
        }),
    ]
}

// =============================================================================
// Helpers
// =============================================================================

/// Recursively collect all plain text from a content item
fn collect_text(item: &ContentItem, out: &mut String) {
    match item {
        ContentItem::Paragraph(p) => {
            for line in &p.lines {
                collect_text(line, out);
            }
        }
        ContentItem::TextLine(tl) => {
            if let Some(nodes) = tl.content.inlines() {
                for node in nodes {
                    collect_inline_text(node, out);
                }
            }
        }
        ContentItem::Session(s) => {
            // Session title — use raw string as inlines may not be parsed
            out.push_str(s.title.as_string());
            for child in s.children.iter() {
                collect_text(child, out);
            }
        }
        ContentItem::Definition(d) => {
            out.push_str(d.subject.as_string());
            for child in d.children.iter() {
                collect_text(child, out);
            }
        }
        ContentItem::List(l) => {
            for item in l.items.iter() {
                collect_text(item, out);
            }
        }
        ContentItem::ListItem(li) => {
            for tc in &li.text {
                out.push_str(tc.as_string());
            }
            for child in li.children.iter() {
                collect_text(child, out);
            }
        }
        ContentItem::VerbatimBlock(v) => {
            out.push_str(v.subject.as_string());
            for child in v.children.iter() {
                collect_text(child, out);
            }
        }
        ContentItem::VerbatimLine(vl) => {
            out.push_str(vl.content.as_string());
        }
        ContentItem::Table(t) => {
            out.push_str(t.subject.as_string());
            for row in t.all_rows() {
                for cell in &row.cells {
                    out.push_str(cell.content.as_string());
                    // Recurse into block children
                    for child in cell.children.iter() {
                        collect_text(child, out);
                    }
                }
            }
        }
        ContentItem::BlankLineGroup(_) => {}
        ContentItem::Annotation(_) => {}
    }
}

fn collect_inline_text(node: &InlineNode, out: &mut String) {
    match node {
        InlineNode::Plain { text, .. } => out.push_str(text),
        InlineNode::Strong { content, .. } | InlineNode::Emphasis { content, .. } => {
            for child in content.iter() {
                collect_inline_text(child, out);
            }
        }
        InlineNode::Code { text, .. } | InlineNode::Math { text, .. } => out.push_str(text),
        InlineNode::Reference { data, .. } => out.push_str(&data.raw),
    }
}

/// Count the maximum nesting depth of a document
fn max_depth(item: &ContentItem, current: usize) -> usize {
    match item {
        ContentItem::Session(s) => {
            let mut max = current;
            for child in s.children.iter() {
                max = max.max(max_depth(child, current + 1));
            }
            max
        }
        ContentItem::Definition(d) => {
            let mut max = current;
            for child in d.children.iter() {
                max = max.max(max_depth(child, current + 1));
            }
            max
        }
        ContentItem::List(l) => {
            let mut max = current;
            for item in l.items.iter() {
                max = max.max(max_depth(item, current + 1));
            }
            max
        }
        ContentItem::ListItem(li) => {
            let mut max = current;
            for child in li.children.iter() {
                max = max.max(max_depth(child, current + 1));
            }
            max
        }
        _ => current,
    }
}

// =============================================================================
// 1. Parse idempotency: parsing same source twice gives same AST
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn parse_is_deterministic(source in valid_document_strategy()) {
        let doc1 = parse_document(&source);
        let doc2 = parse_document(&source);

        match (doc1, doc2) {
            (Ok(d1), Ok(d2)) => {
                // Same number of root children
                let items1: Vec<&ContentItem> = d1.root.children.iter().collect();
                let items2: Vec<&ContentItem> = d2.root.children.iter().collect();
                prop_assert_eq!(
                    items1.len(),
                    items2.len(),
                    "Parse produced different number of root items on same source"
                );

                // Same text content
                let mut text1 = String::new();
                let mut text2 = String::new();
                for item in items1.iter() {
                    collect_text(item, &mut text1);
                }
                for item in items2.iter() {
                    collect_text(item, &mut text2);
                }
                prop_assert_eq!(
                    text1, text2,
                    "Parse produced different text content on same source"
                );
            }
            (Err(_), Err(_)) => {} // Both fail — that's consistent
            (Ok(_), Err(e)) | (Err(e), Ok(_)) => {
                prop_assert!(false, "Inconsistent parse results: {e}");
            }
        }
    }
}

// =============================================================================
// 2. No content loss: text words from source appear in AST
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn paragraph_text_preserved(line in paragraph_line()) {
        let source = format!("{line}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}"));

        let mut text = String::new();
        for item in doc.root.children.iter() {
            collect_text(item, &mut text);
        }

        // Each word from the source should appear in the collected text
        for word in line.split_whitespace() {
            let word_clean = word.trim_end_matches('.');
            if !word_clean.is_empty() {
                prop_assert!(
                    text.contains(word_clean),
                    "Word '{}' from source not found in AST text '{}'\nSource: {}",
                    word_clean, text, source
                );
            }
        }
    }

    #[test]
    fn definition_subject_preserved(
        subject in subject_strategy(),
        content in paragraph_line(),
    ) {
        let source = format!("{subject}:\n    {content}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}"));

        let mut text = String::new();
        for item in doc.root.children.iter() {
            collect_text(item, &mut text);
        }

        // Subject text should be in the AST
        prop_assert!(
            text.contains(&subject),
            "Subject '{}' not found in AST text '{}'\nSource: {}",
            subject, text, source
        );
    }

    #[test]
    fn list_item_text_preserved(
        item1 in list_text(),
        item2 in list_text(),
    ) {
        let source = format!("\n- {item1}\n- {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}"));

        let mut text = String::new();
        for item in doc.root.children.iter() {
            collect_text(item, &mut text);
        }

        for word in item1.split_whitespace() {
            prop_assert!(
                text.contains(word),
                "Word '{}' from item1 not found in AST\nSource: {}",
                word, source
            );
        }
        for word in item2.split_whitespace() {
            prop_assert!(
                text.contains(word),
                "Word '{}' from item2 not found in AST\nSource: {}",
                word, source
            );
        }
    }
}

// =============================================================================
// 3. Nesting depth is bounded by indentation
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn nesting_depth_bounded_by_indentation(
        source in valid_document_strategy(),
    ) {
        let doc = parse_document(&source);
        if let Ok(doc) = doc {
            // Count max indentation levels in source (groups of 4 spaces)
            let max_indent = source.lines()
                .map(|line| {
                    let spaces = line.len() - line.trim_start().len();
                    spaces / 4
                })
                .max()
                .unwrap_or(0);

            // AST depth should not exceed indent levels + 1 (root)
            let ast_depth = doc.root.children.iter()
                .map(|item| max_depth(item, 0))
                .max()
                .unwrap_or(0);

            prop_assert!(
                ast_depth <= max_indent + 1,
                "AST depth {} exceeds indentation depth {} + 1\nSource:\n{}",
                ast_depth, max_indent, source
            );
        }
    }
}
