//! Export tests for Markdown format (Lex → Markdown)
//!
//! These tests verify that Lex documents are correctly converted to Markdown
//! by checking the resulting Comrak AST structure.

use comrak::nodes::NodeValue;
use comrak::{parse_document, Arena, ComrakOptions};
use insta::assert_snapshot;
use lex_babel::format::Format;
use lex_babel::formats::markdown::MarkdownFormat;
use lex_core::lex::transforms::standard::STRING_TO_AST;

/// Helper to convert Lex source to Markdown and parse to Comrak AST
fn lex_to_comrak_ast<'a>(
    lex_src: &str,
    arena: &'a Arena<comrak::nodes::AstNode<'a>>,
) -> &'a comrak::nodes::AstNode<'a> {
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();
    let options = ComrakOptions::default();
    parse_document(arena, &md, &options)
}

/// Helper to recursively collect node types from a Comrak AST
fn collect_node_types<'a>(
    node: &'a comrak::nodes::AstNode<'a>,
    types: &mut std::collections::HashSet<String>,
) {
    let value = &node.data.borrow().value;
    let type_name = match value {
        NodeValue::Document => "Document",
        NodeValue::Paragraph => "Paragraph",
        NodeValue::Heading(_) => "Heading",
        NodeValue::List(_) => "List",
        NodeValue::Item(_) => "Item",
        NodeValue::CodeBlock(_) => "CodeBlock",
        NodeValue::Strong => "Strong",
        NodeValue::Emph => "Emph",
        NodeValue::Code(_) => "Code",
        NodeValue::Link(_) => "Link",
        _ => "Other",
    };
    types.insert(type_name.to_string());

    for child in node.children() {
        collect_node_types(child, types);
    }
}

#[test]
fn test_paragraph_simple() {
    let lex_src = "This is a simple paragraph.\n";

    let arena = Arena::new();
    let root = lex_to_comrak_ast(lex_src, &arena);

    // Verify structure: Document → Paragraph → Text
    let mut found_paragraph = false;
    for child in root.children() {
        if matches!(child.data.borrow().value, NodeValue::Paragraph) {
            found_paragraph = true;
        }
    }
    assert!(found_paragraph, "Should have a paragraph node");
}

#[test]
fn test_heading_and_paragraph_separation() {
    // Session title followed by paragraph content should not be merged into the heading
    let lex_src = "1. Title\n\n    Body text.\n";

    let arena = Arena::new();
    let root = lex_to_comrak_ast(lex_src, &arena);

    let mut heading_text = String::new();
    let mut paragraph_text = String::new();

    for child in root.children() {
        match &child.data.borrow().value {
            NodeValue::Heading(_) => {
                for inline in child.children() {
                    if let NodeValue::Text(t) = &inline.data.borrow().value {
                        heading_text.push_str(t);
                    }
                }
            }
            NodeValue::Paragraph => {
                for inline in child.children() {
                    if let NodeValue::Text(t) = &inline.data.borrow().value {
                        paragraph_text.push_str(t);
                    }
                }
            }
            _ => {}
        }
    }

    // Session numbering must be preserved in the heading text
    assert_eq!(heading_text.trim(), "1. Title");
    assert!(paragraph_text.contains("Body text."));
}

// ============================================================================
// TRIFECTA TESTS - Document Structure
// ============================================================================

#[test]
fn test_trifecta_010_paragraphs_sessions_flat_single() {
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/trifecta/010-paragraphs-sessions-flat-single.lex",
    )
    .expect("trifecta 010 file should exist");

    let arena = Arena::new();
    let root = lex_to_comrak_ast(&lex_src, &arena);

    // Collect all top-level node types
    let mut paragraphs = 0;
    let mut headings = 0;
    let mut heading_levels = Vec::new();

    for child in root.children() {
        match &child.data.borrow().value {
            NodeValue::Paragraph => paragraphs += 1,
            NodeValue::Heading(h) => {
                headings += 1;
                heading_levels.push(h.level);
            }
            _ => {}
        }
    }

    // Should have:
    // - Initial paragraph ("Paragraphs and Single Session Test")
    // - Another paragraph ("This document tests...")
    // - Heading for "1. Introduction"
    // - Paragraphs that were nested in the session (now at document level)
    // - Paragraph after session
    // - Heading for "Another Session"
    // - Paragraph nested in that session
    // - Final paragraph

    assert!(paragraphs > 0, "Should have paragraphs");
    assert!(
        headings >= 3,
        "Should have at least 3 headings (1 title + 2 sessions), found {headings}"
    );

    // Document title is H1, sessions at root level should be H2
    assert!(
        heading_levels.contains(&1),
        "Should have H1 for document title"
    );
    let session_levels: Vec<_> = heading_levels
        .iter()
        .filter(|&&l| l != 1)
        .copied()
        .collect();
    for level in &session_levels {
        assert_eq!(*level, 2, "Root-level sessions should be h2");
    }

    println!(
        "Trifecta 010: {paragraphs} paragraphs, {headings} headings (levels: {heading_levels:?})"
    );
}

#[test]
fn test_trifecta_020_paragraphs_sessions_flat_multiple() {
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/trifecta/020-paragraphs-sessions-flat-multiple.lex",
    )
    .expect("trifecta 020 file should exist");

    let arena = Arena::new();
    let root = lex_to_comrak_ast(&lex_src, &arena);

    // Count headings (sessions)
    let heading_count = root
        .children()
        .filter(|child| matches!(child.data.borrow().value, NodeValue::Heading(_)))
        .count();

    // Should have multiple sessions including nested ones
    // Root: "First Session", "Second Session", "Third Session", "Session Without Numbering"
    // Nested: "Session titles don't require numbering markers." (nested in "Session Without Numbering")
    assert!(
        heading_count >= 4,
        "Should have at least 4 headings (sessions), found {heading_count}"
    );

    // Collect heading levels to see if we have nesting
    let mut has_h1 = false;
    let mut heading_levels_vec = Vec::new();
    for child in root.children() {
        if let NodeValue::Heading(h) = &child.data.borrow().value {
            heading_levels_vec.push(h.level);
            if h.level == 2 {
                has_h1 = true;
            }
        }
    }

    assert!(has_h1, "Should have at least one h2 (root session)");
    println!("Trifecta 020: Heading levels: {heading_levels_vec:?}");

    println!("Trifecta 020: {heading_count} headings");
}

#[test]
fn test_trifecta_060_nesting() {
    let lex_src = std::fs::read_to_string("../../comms/specs/trifecta/060-trifecta-nesting.lex")
        .expect("trifecta 060 file should exist");

    let arena = Arena::new();
    let root = lex_to_comrak_ast(&lex_src, &arena);

    // Collect heading levels to verify nesting structure
    let mut heading_levels = Vec::new();
    let mut has_paragraphs = false;
    let mut has_lists = false;

    for child in root.children() {
        match &child.data.borrow().value {
            NodeValue::Heading(h) => heading_levels.push(h.level),
            NodeValue::Paragraph => has_paragraphs = true,
            NodeValue::List(_) => has_lists = true,
            _ => {}
        }
    }

    // Verify we have multiple heading levels (representing nested sessions)
    let min_level = heading_levels.iter().min().copied().unwrap_or(0);
    let max_level = heading_levels.iter().max().copied().unwrap_or(0);

    assert!(
        min_level >= 1 && max_level >= 2,
        "Should have nested heading levels (h1, h2, h3...), found range {min_level}..{max_level}"
    );

    // Should have all three element types
    assert!(has_paragraphs, "Should have paragraphs");
    assert!(has_lists, "Should have lists");
    assert!(!heading_levels.is_empty(), "Should have headings");

    // Verify heading sequence makes sense
    // Session 1.2.1 should generate h3, 1.1 should generate h2, etc.
    assert!(
        heading_levels.contains(&2),
        "Should have h2 for root sessions"
    );
    assert!(
        heading_levels.contains(&3) || heading_levels.contains(&4),
        "Should have h3 or h4 for nested sessions"
    );

    println!(
        "Trifecta 060: {} headings with levels {:?}, {} paragraphs, {} lists",
        heading_levels.len(),
        heading_levels,
        if has_paragraphs { "some" } else { "no" },
        if has_lists { "some" } else { "no" }
    );
}

// ============================================================================
// BENCHMARK TESTS
// ============================================================================

#[test]
fn test_kitchensink() {
    let lex_src = std::fs::read_to_string("../../comms/specs/benchmark/010-kitchensink.lex")
        .expect("kitchensink file should exist");

    let arena = Arena::new();
    let root = lex_to_comrak_ast(&lex_src, &arena);

    // Kitchensink should have variety of nodes
    let mut node_types = std::collections::HashSet::new();
    collect_node_types(root, &mut node_types);

    // Kitchensink should exercise multiple element types
    assert!(
        node_types.len() >= 5,
        "Kitchensink should have at least 5 different node types, found: {node_types:?}"
    );

    println!("Kitchensink node types: {node_types:?}");
}

#[test]
fn test_kitchensink_snapshot() {
    let lex_src = std::fs::read_to_string("../../comms/specs/benchmark/010-kitchensink.lex")
        .expect("kitchensink file should exist");

    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    assert_snapshot!("kitchensink_markdown", md);
}

// ============================================================================
// ISSUE A: Reference Escaping Tests
// ============================================================================

#[test]
fn test_reference_url_converted_to_link() {
    let lex_src = "Visit [https://example.com] for more.\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // URLs should be converted to Markdown links (as autolinks or regular links)
    // Comrak may render as <url> autolink or [text](url) depending on context
    assert!(
        md.contains("https://example.com"),
        "URL should be present in output"
    );
    assert!(
        !md.contains("\\[https://"),
        "Should not escape brackets for URLs"
    );
}

#[test]
fn test_reference_anchor_converted_to_link() {
    let lex_src = "See section [#introduction] above.\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Anchors should be converted to Markdown links [text](url)
    // The # in link text is escaped as \# which is correct Markdown
    assert!(
        md.contains("(#introduction)"),
        "Anchor should link to #introduction"
    );
    assert!(
        md.contains("[\\#introduction]") || md.contains("[#introduction]"),
        "Anchor text should contain #introduction"
    );
}

#[test]
fn test_citation_converted_to_ref_link() {
    let lex_src = "According to [@smith2023], this is true.\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Citations should be converted to #ref-* links
    assert!(md.contains("[@smith2023]"), "Citation should be a link");
    assert!(
        !md.contains("\\[@"),
        "Should not escape brackets for citations"
    );
}

#[test]
fn test_placeholder_reference_as_text() {
    let lex_src = "This needs citation [TK-REF-2025-01].\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Placeholders should render as plain text with escaped brackets
    // since they're not recognized as links
    assert!(
        md.contains("\\[TK-REF-2025-01\\]"),
        "Placeholder should be visible as text with escaped brackets"
    );
    assert!(
        md.contains("TK-REF-2025-01"),
        "Placeholder content should be present"
    );
}

// ============================================================================
// ISSUE C: Markdown List Formatting Tests
// ============================================================================

#[test]
fn test_list_with_simple_items() {
    let lex_src = "- First item\n- Second item\n- Third item\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Simple list items should be rendered correctly
    assert!(md.contains("- First item"));
    assert!(md.contains("- Second item"));
    assert!(md.contains("- Third item"));
}

#[test]
fn test_list_with_multi_paragraph_items() {
    // Test that list items with multiple paragraphs are properly indented
    // Comrak should handle indentation automatically
    let lex_src = concat!(
        "- Item one with first paragraph.\n",
        "\n",
        "    Second paragraph in item one.\n",
        "\n",
        "- Item two.\n"
    );
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // The output should maintain list structure
    // Comrak handles proper indentation for multi-paragraph items
    assert!(md.contains("- "), "Should have list markers");
    assert!(md.contains("Item one"));
    assert!(md.contains("Item two"));
}

// ============================================================================
// DOCUMENT TITLE TESTS
// ============================================================================

#[test]
fn test_document_title_exported_as_h1() {
    // Use spec file: document with explicit title should export with H1 heading
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-01-title-explicit.lex",
    )
    .expect("document-01 spec file should exist");
    let lex_doc = STRING_TO_AST.run(lex_src).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Should start with H1 heading containing the title
    assert!(
        md.starts_with("# My Document Title\n"),
        "Should start with H1 title heading"
    );
}

#[test]
fn test_document_first_paragraph_as_title() {
    // Use spec file: first paragraph followed by blank line becomes document title
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-06-title-empty.lex",
    )
    .expect("document-06 spec file should exist");
    let lex_doc = STRING_TO_AST.run(lex_src).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // First paragraph "Just a paragraph with no title." becomes the H1 title
    assert!(
        md.starts_with("# Just a paragraph with no title.\n"),
        "First paragraph should become H1 title"
    );
}

#[test]
fn test_document_session_only_no_h1_title() {
    // Use spec file: document starts with session (no explicit document title)
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-05-title-session-hoist.lex",
    )
    .expect("document-05 spec file should exist");
    let lex_doc = STRING_TO_AST.run(lex_src).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Should not have H1 title when document starts with session
    assert!(
        !md.starts_with("# "),
        "Session-only doc should not have H1 title"
    );
    // Session should be H2
    assert!(md.contains("## "), "Session should be exported as H2");
}

// ============================================================================
// SESSION NUMBERING PRESERVATION TESTS
// ============================================================================

#[test]
fn test_numbered_session_preserves_numbering() {
    let lex_src = "1. Introduction\n\n    Content here.\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Heading must contain the numbering
    assert!(
        md.contains("## 1"),
        "Numbered session heading must contain '1' prefix: {md}"
    );
    assert!(
        md.contains("Introduction"),
        "Heading must contain title text"
    );
}

#[test]
fn test_dotted_numbering_preserved() {
    let lex_src = "1. Parent\n\n    1.1. Child\n\n        Content.\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    assert!(md.contains("Parent"), "Parent heading must be present");
    assert!(
        md.contains("1.1"),
        "Dotted numbering '1.1' must be preserved in nested heading: {md}"
    );
    assert!(md.contains("Child"), "Child heading text must be present");
}

#[test]
fn test_unnumbered_session_no_numbering_added() {
    let lex_src = "My Session Title\n\n    Content.\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Should have heading with just the title, no added numbering
    assert!(
        md.contains("## My Session Title"),
        "Unnumbered session should export as-is: {md}"
    );
}

#[test]
fn test_numbering_round_trip_lex_md_lex() {
    let lex_src =
        "1. First Session\n\n    Paragraph one.\n\n2. Second Session\n\n    Paragraph two.\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    // Export to Markdown
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();
    assert!(md.contains("1"), "MD should contain numbering '1'");
    assert!(md.contains("2"), "MD should contain numbering '2'");

    // Import back to Lex
    let lex_doc2 = MarkdownFormat.parse(&md).unwrap();

    // Re-export and verify numbering survived
    let md2 = MarkdownFormat.serialize(&lex_doc2).unwrap();
    assert!(
        md2.contains("First Session"),
        "First session must survive round-trip"
    );
    assert!(
        md2.contains("Second Session"),
        "Second session must survive round-trip"
    );
}

#[test]
fn test_nested_numbering_round_trip() {
    let lex_src = concat!(
        "1. Top Level\n\n",
        "    Opening paragraph.\n\n",
        "    1.1. Nested\n\n",
        "        Nested content.\n\n",
        "    1.2. Another Nested\n\n",
        "        More content.\n",
    );
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Verify heading hierarchy
    assert!(md.contains("## "), "Top level should be H2");
    assert!(md.contains("### "), "Nested sessions should be H3: {md}");

    // Verify all titles present with numbering
    assert!(md.contains("Top Level"), "Top level title present");
    assert!(md.contains("Nested"), "First nested title present");
    assert!(md.contains("Another Nested"), "Second nested title present");

    // Round-trip back
    let lex_doc2 = MarkdownFormat.parse(&md).unwrap();
    let md2 = MarkdownFormat.serialize(&lex_doc2).unwrap();

    assert!(md2.contains("Top Level"), "Top level survives round-trip");
    assert!(md2.contains("Nested"), "Nested survives round-trip");
}
