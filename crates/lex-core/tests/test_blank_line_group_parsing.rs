//! Unit tests for BlankLineGroup AST node.
//!
//! These tests verify that:
//! - BlankLineGroup is a valid AST node type
//! - BlankLineGroup nodes have proper structure (count, source_tokens, location)
//! - BlankLineGroup nodes are accessible via the visitor pattern
//! - Blank lines in nested structures (definitions, sessions, lists) can contain BlankLineGroup nodes

use lex_core::lex::ast::AstNode;
use lex_core::lex::ast::Document;
use lex_core::lex::parsing::{parse_document as parse_doc, ContentItem};

fn parse_document(source: &str) -> Document {
    parse_doc(source).expect("Failed to parse document")
}

#[test]
fn test_blank_line_group_node_type_visitor() {
    let source = "# Title\n\nA\nLine 2\n\nB";
    let doc = parse_document(source);
    let root = &doc.root;

    let all_nodes: Vec<_> = root.iter_all_nodes().collect();
    let blg = all_nodes
        .iter()
        .filter_map(|item| item.as_blank_line_group())
        .next()
        .expect("Expected BlankLineGroup node");

    // Test that node_type method works
    let node_type = blg.node_type();
    assert_eq!(node_type, "BlankLineGroup");
}

#[test]
fn test_blank_line_group_display_label_visitor() {
    let source = "# Title\n\nA\nLine 2\n\nB";
    let doc = parse_document(source);
    let root = &doc.root;

    let all_nodes: Vec<_> = root.iter_all_nodes().collect();
    let blg = all_nodes
        .iter()
        .filter_map(|item| item.as_blank_line_group())
        .next()
        .expect("Expected BlankLineGroup node");

    // Test that display_label method works
    let label = blg.display_label();
    assert_eq!(
        label, "1 blank line",
        "Label should describe singular blank line"
    );
}

#[test]
fn test_blank_line_group_structure_count() {
    let source = "# Title\n\nA\nLine 2\n\nB";
    let doc = parse_document(source);
    let root = &doc.root;

    let all_nodes: Vec<_> = root.iter_all_nodes().collect();
    let blg = all_nodes
        .iter()
        .filter_map(|item| item.as_blank_line_group())
        .next()
        .expect("Expected BlankLineGroup node");

    // Verify count field exists and is accessible
    assert!(blg.count > 0, "BlankLineGroup should have count > 0");
}

#[test]
fn test_blank_line_group_structure_source_tokens() {
    let source = "# Title\n\nA\nLine 2\n\nB";
    let doc = parse_document(source);
    let root = &doc.root;

    let all_nodes: Vec<_> = root.iter_all_nodes().collect();
    let blg = all_nodes
        .iter()
        .filter_map(|item| item.as_blank_line_group())
        .next()
        .expect("Expected BlankLineGroup node");

    // Verify source_tokens field exists and is accessible
    assert!(
        !blg.source_tokens.is_empty(),
        "BlankLineGroup should have source tokens"
    );
    // Verify tokens contain BlankLine variant
    let has_blank_line_token = blg
        .source_tokens
        .iter()
        .any(|t| matches!(t, lex_core::lex::lexing::Token::BlankLine(_)));
    assert!(has_blank_line_token, "Should contain BlankLine token");
}

#[test]
fn test_blank_line_group_near_lists() {
    let source =
        "# Title\n\nIntro paragraph\nLine 2\n\n- Item 1\n    Content A\n- Item 2\n    Content B\n\nClosing paragraph";
    let doc = parse_document(source);
    let root = &doc.root;

    let list_index = root
        .children
        .iter()
        .position(|item| matches!(item, ContentItem::List(_)))
        .expect("Expected list in document");

    assert!(list_index > 0, "List should follow a blank line group");
    assert!(
        matches!(
            root.children[list_index - 1],
            ContentItem::BlankLineGroup(_)
        ),
        "Expected blank line group before list"
    );
}

#[test]
fn test_blank_line_group_in_definitions() {
    let source = "Definition:\n    First\n\n    Second";
    let doc = parse_document(source);
    let root = &doc.root;

    // Use new query API to find definitions with blank line groups
    let definitions = root.find_definitions(|def| {
        def.children
            .iter()
            .any(|child| matches!(child, ContentItem::BlankLineGroup(_)))
    });

    let definition = definitions
        .into_iter()
        .next()
        .expect("Expected definition with blank lines");
    let blg = definition
        .children
        .iter()
        .filter_map(|item| item.as_blank_line_group())
        .next()
        .expect("Definition should contain blank line group");
    assert!(blg.count > 0, "Definition should have blank lines");
}

#[test]
fn test_blank_line_group_in_sessions() {
    let source = "Title\n\n    First\n\n    Second";
    let doc = parse_document(source);
    let root = &doc.root;

    // Use new query API to find sessions with blank line groups
    let sessions = root.find_sessions(|s| {
        s.children
            .iter()
            .any(|child| matches!(child, ContentItem::BlankLineGroup(_)))
    });

    let session = sessions
        .into_iter()
        .next()
        .expect("Expected session with blank lines");
    let blg = session
        .children
        .iter()
        .filter_map(|item| item.as_blank_line_group())
        .next()
        .expect("Session should contain blank line group");
    assert!(blg.count > 0, "Session should have blank lines");
}

#[test]
fn test_blank_line_group_is_content_item_variant() {
    // Verify BlankLineGroup can be matched as a ContentItem variant
    let source = "# Title\n\nA\nLine 2\n\nB";
    let doc = parse_document(source);

    // This test verifies the variant exists in ContentItem enum
    // by successfully pattern matching it in the content
    assert!(
        doc.root
            .children
            .iter()
            .any(|item| matches!(item, ContentItem::BlankLineGroup(_))),
        "Root content should expose BlankLineGroup variant"
    );
}
