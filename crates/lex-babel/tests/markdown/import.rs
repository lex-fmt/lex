//! Import tests for Markdown format (Markdown → Lex)
//!
//! These tests verify that Markdown documents are correctly converted to Lex
//! by checking the resulting Lex AST structure.

use insta::assert_snapshot;
use lex_babel::format::Format;
use lex_babel::formats::lex::LexFormat;
use lex_babel::formats::markdown::MarkdownFormat;
use lex_babel::formats::tag::serialize_document_with_params;
use lex_core::lex::ast::ContentItem;
use std::collections::HashMap;
use std::path::PathBuf;

/// Helper to parse Markdown to Lex AST
fn md_to_lex(md: &str) -> lex_core::lex::ast::Document {
    MarkdownFormat.parse(md).expect("Should parse markdown")
}

/// Read a fixture file from the tests/fixtures directory
fn read_fixture(fixture: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(fixture);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {path:?}: {e}"))
}

/// Snapshot helper for reference Markdown fixtures
///
/// Uses `ast-full` serialization to capture complete AST structure including
/// annotations and all metadata, ensuring comprehensive regression detection
/// for complex markdown documents.
fn snapshot_md_fixture(fixture: &str, snapshot_name: &str) {
    let md = read_fixture(fixture);
    let doc = md_to_lex(&md);

    let mut params = HashMap::new();
    params.insert("ast-full".to_string(), "true".to_string());
    let serialized = serialize_document_with_params(&doc, &params);

    assert_snapshot!(snapshot_name, serialized);
}

/// Validate that Markdown→Lex text produces valid Lex that lex-core can parse.
///
/// This catches serialization bugs where lex-babel generates syntactically
/// invalid Lex output.
fn assert_lex_output_valid(md: &str, label: &str) {
    let lex_doc = md_to_lex(md);
    let lex_format = LexFormat::default();
    let lex_text = lex_format
        .serialize(&lex_doc)
        .unwrap_or_else(|e| panic!("[{label}] Failed to serialize to Lex: {e}"));

    // Parse the generated Lex text back through lex-core's parser
    let reparsed = lex_format.parse(&lex_text).unwrap_or_else(|e| {
        panic!("[{label}] Generated Lex is invalid:\n{e}\n\nLex output:\n{lex_text}")
    });

    // Basic structural sanity: reparsed document should be non-empty if input was non-empty
    if !lex_doc.root.children.is_empty() {
        assert!(
            !reparsed.root.children.is_empty(),
            "[{label}] Reparsed Lex document lost all content"
        );
    }
}

#[test]
fn test_paragraph_simple() {
    let md = "This is a simple paragraph.\n";
    let doc = md_to_lex(md);

    // Should have paragraph in root session
    assert!(!doc.root.children.is_empty());

    // Verify first element is a paragraph
    match &doc.root.children[0] {
        ContentItem::Paragraph(_) => {}
        _ => panic!("Expected paragraph element"),
    }
}

#[test]
fn test_heading_to_session() {
    // Use H2 for sessions (H1 is reserved for document title)
    let md = "## Introduction\n\nSome content here.\n";
    let doc = md_to_lex(md);

    // Should have session with title "Introduction"
    assert!(!doc.root.children.is_empty());

    match &doc.root.children[0] {
        ContentItem::Session(session) => {
            // Check title
            assert!(
                !session.title.is_empty(),
                "Session should have title from heading"
            );

            // Should have content
            assert!(
                !session.children.is_empty(),
                "Session should have paragraph content"
            );
        }
        _ => panic!("Expected session element from heading"),
    }
}

#[test]
fn test_nested_headings() {
    // Use H2/H3 for sessions (H1 is reserved for document title)
    let md = "## Level 1\n\n### Level 2\n\nContent.\n";
    let doc = md_to_lex(md);

    // Should have nested sessions
    assert!(!doc.root.children.is_empty());

    match &doc.root.children[0] {
        ContentItem::Session(session1) => {
            // First session should have nested session
            let has_nested_session = session1
                .children
                .iter()
                .any(|el| matches!(el, ContentItem::Session(_)));
            assert!(has_nested_session, "Should have nested session");
        }
        _ => panic!("Expected session element"),
    }
}

#[test]
fn test_list() {
    let md = "- First item\n- Second item\n- Third item\n";
    let doc = md_to_lex(md);

    assert!(!doc.root.children.is_empty());

    match &doc.root.children[0] {
        ContentItem::List(list) => {
            assert_eq!(list.items.len(), 3, "Should have 3 list items");
        }
        _ => panic!("Expected list element"),
    }
}

#[test]
fn test_code_block_to_verbatim() {
    let md = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```\n";
    let doc = md_to_lex(md);

    assert!(!doc.root.children.is_empty());

    match &doc.root.children[0] {
        ContentItem::VerbatimBlock(verbatim) => {
            // Should have code content (children contains VerbatimLine items)
            assert!(!verbatim.children.is_empty(), "Should have code content");
        }
        _ => panic!("Expected verbatim element from code block"),
    }
}

#[test]
fn test_inline_formatting() {
    // Test that paragraphs are created and have content
    let md = "This is **bold** and *italic* and `code` text.\n";
    let doc = md_to_lex(md);

    match &doc.root.children[0] {
        ContentItem::Paragraph(para) => {
            // Should have lines with text content
            assert!(!para.lines.is_empty(), "Paragraph should have lines");
        }
        _ => panic!("Expected paragraph"),
    }
}

#[test]
fn test_definition_imports() {
    let md = "**Term**: Description line one.\n\nAdditional paragraph.\n";
    let doc = md_to_lex(md);

    match &doc.root.children[0] {
        ContentItem::Definition(def) => {
            assert!(
                def.subject.as_string().contains("Term"),
                "Definition subject should capture term text"
            );
            assert!(
                !def.children.is_empty(),
                "Definition should have description content"
            );
        }
        other => panic!("Expected definition, found {other:?}"),
    }
}

#[test]
fn test_annotation_import() {
    let md = "<!-- lex:note type=warning -->\n\nThis is annotated content.\n\n<!-- /lex -->\n";
    let doc = md_to_lex(md);

    // Find annotation in the document
    let has_annotation = doc.root.children.iter().any(|el| {
        if let ContentItem::Annotation(anno) = el {
            assert_eq!(anno.data.label.value, "note");
            assert_eq!(anno.data.parameters.len(), 1);
            assert_eq!(anno.data.parameters[0].key, "type");
            assert_eq!(anno.data.parameters[0].value, "warning");
            assert!(!anno.children.is_empty(), "Annotation should have content");
            true
        } else {
            false
        }
    });

    assert!(has_annotation, "Document should contain annotation");
}

#[test]
fn test_annotation_round_trip() {
    // Create markdown with annotation
    let md = "<!-- lex:note type=info -->\n\nAnnotated paragraph.\n\n<!-- /lex -->\n";

    // Import to Lex
    let lex_doc = md_to_lex(md);

    // Export back to Markdown
    let md_export = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Import again
    let lex_doc2 = md_to_lex(&md_export);

    // Verify annotation is preserved
    let has_annotation = lex_doc2
        .root
        .children
        .iter()
        .any(|el| matches!(el, ContentItem::Annotation(_)));

    assert!(has_annotation, "Annotation should survive round-trip");
}

// ============================================================================
// TRIFECTA TESTS - Document Structure
// ============================================================================

#[test]
fn test_trifecta_010_round_trip() {
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/trifecta/010-paragraphs-sessions-flat-single.lex",
    )
    .expect("trifecta 010 file should exist");

    let lex_doc = lex_core::lex::transforms::standard::STRING_TO_AST
        .run(lex_src.to_string())
        .unwrap();

    let md = MarkdownFormat.serialize(&lex_doc).unwrap();
    let lex_doc2 = md_to_lex(&md);

    // Verify structural preservation: sessions with content
    let sessions: Vec<_> = lex_doc2
        .root
        .children
        .iter()
        .filter_map(|el| match el {
            ContentItem::Session(s) => Some(s),
            _ => None,
        })
        .collect();

    assert!(
        sessions.len() >= 2,
        "Should have at least 2 sessions, found {}",
        sessions.len()
    );

    // First session should have title containing "Introduction"
    assert!(
        sessions[0].title.as_string().contains("Introduction"),
        "First session should be 'Introduction', got '{}'",
        sessions[0].title.as_string()
    );

    // Sessions should have paragraph content
    for session in &sessions {
        let has_paragraphs = session
            .children
            .iter()
            .any(|c| matches!(c, ContentItem::Paragraph(_)));
        assert!(
            has_paragraphs,
            "Session '{}' should have paragraph content",
            session.title.as_string()
        );
    }
}

#[test]
fn test_trifecta_020_round_trip() {
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/trifecta/020-paragraphs-sessions-flat-multiple.lex",
    )
    .expect("trifecta 020 file should exist");

    let lex_doc = lex_core::lex::transforms::standard::STRING_TO_AST
        .run(lex_src.to_string())
        .unwrap();

    let md = MarkdownFormat.serialize(&lex_doc).unwrap();
    let lex_doc2 = md_to_lex(&md);

    // Collect root-level sessions
    let sessions: Vec<_> = lex_doc2
        .root
        .children
        .iter()
        .filter_map(|el| match el {
            ContentItem::Session(s) => Some(s),
            _ => None,
        })
        .collect();

    assert!(
        sessions.len() >= 3,
        "Should have at least 3 root sessions, found {}",
        sessions.len()
    );

    // Verify session titles survive round-trip
    let titles: Vec<String> = sessions
        .iter()
        .map(|s| s.title.as_string().to_string())
        .collect();
    assert!(
        titles.iter().any(|t| t.contains("First")),
        "Should have a session containing 'First' in titles: {titles:?}"
    );
    assert!(
        titles.iter().any(|t| t.contains("Second")),
        "Should have a session containing 'Second' in titles: {titles:?}"
    );
}

#[test]
fn test_trifecta_060_nesting_round_trip() {
    let lex_src = std::fs::read_to_string("../../comms/specs/trifecta/060-trifecta-nesting.lex")
        .expect("trifecta 060 file should exist");

    let lex_doc = lex_core::lex::transforms::standard::STRING_TO_AST
        .run(lex_src.to_string())
        .unwrap();

    let md = MarkdownFormat.serialize(&lex_doc).unwrap();
    let lex_doc2 = md_to_lex(&md);

    // Verify we have sessions (nesting structure)
    let root_sessions: Vec<_> = lex_doc2
        .root
        .children
        .iter()
        .filter_map(|el| match el {
            ContentItem::Session(s) => Some(s),
            _ => None,
        })
        .collect();

    assert!(!root_sessions.is_empty(), "Should have root-level sessions");

    // Verify nested sessions exist (at least one session has child sessions)
    let has_nested = root_sessions.iter().any(|s| {
        s.children
            .iter()
            .any(|c| matches!(c, ContentItem::Session(_)))
    });
    assert!(
        has_nested,
        "Should have nested sessions (heading hierarchy)"
    );

    // Verify lists survive round-trip
    fn has_list(items: &[ContentItem]) -> bool {
        items.iter().any(|c| match c {
            ContentItem::List(_) => true,
            ContentItem::Session(s) => has_list(&s.children),
            _ => false,
        })
    }
    assert!(
        has_list(&lex_doc2.root.children),
        "Lists should survive round-trip"
    );
}

// ============================================================================
// BENCHMARK TESTS
// ============================================================================

#[test]
fn test_kitchensink_round_trip() {
    let lex_src = std::fs::read_to_string("../../comms/specs/benchmark/010-kitchensink.lex")
        .expect("kitchensink file should exist");

    let lex_doc = lex_core::lex::transforms::standard::STRING_TO_AST
        .run(lex_src.to_string())
        .unwrap();

    // Export to Markdown
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();

    // Import back to Lex
    let lex_doc2 = md_to_lex(&md);

    // Count element types recursively
    fn count_elements(elements: &[ContentItem]) -> (usize, usize, usize, usize) {
        let mut counts = (0usize, 0usize, 0usize, 0usize); // paragraphs, sessions, lists, verbatims
        for el in elements {
            match el {
                ContentItem::Paragraph(_) => counts.0 += 1,
                ContentItem::Session(s) => {
                    counts.1 += 1;
                    let inner = count_elements(&s.children);
                    counts.0 += inner.0;
                    counts.1 += inner.1;
                    counts.2 += inner.2;
                    counts.3 += inner.3;
                }
                ContentItem::List(_) => counts.2 += 1,
                ContentItem::VerbatimBlock(_) => counts.3 += 1,
                _ => {}
            }
        }
        counts
    }

    let (paragraphs, sessions, lists, verbatims) = count_elements(&lex_doc2.root.children);

    // Kitchensink is a comprehensive document — verify meaningful counts
    assert!(
        paragraphs >= 3,
        "Kitchensink should have at least 3 paragraphs, found {paragraphs}"
    );
    assert!(
        sessions >= 2,
        "Kitchensink should have at least 2 sessions, found {sessions}"
    );
    assert!(
        lists >= 1,
        "Kitchensink should have at least 1 list, found {lists}"
    );
    assert!(
        verbatims >= 1,
        "Kitchensink should have at least 1 verbatim block, found {verbatims}"
    );

    // Verify sessions have meaningful titles
    let root_sessions: Vec<_> = lex_doc2
        .root
        .children
        .iter()
        .filter_map(|el| match el {
            ContentItem::Session(s) => Some(s),
            _ => None,
        })
        .collect();

    for session in &root_sessions {
        assert!(
            !session.title.as_string().is_empty(),
            "All root sessions should have non-empty titles"
        );
    }
}

// ============================================================================
// REFERENCE FIXTURE SNAPSHOTS
// ============================================================================

#[test]
fn test_markdown_import_commonmark_reference() {
    snapshot_md_fixture(
        "markdown-reference-commonmark.md",
        "markdown_import_commonmark_reference",
    );
}

#[test]
fn test_markdown_import_comrak_reference() {
    snapshot_md_fixture(
        "markdown-reference-comrak.md",
        "markdown_import_comrak_reference",
    );
}

#[test]
fn test_markdown_import_comrak_readme() {
    snapshot_md_fixture("comrak-readme.md", "markdown_import_comrak_readme");
}

#[test]
fn test_comrak_readme_structure() {
    let md = read_fixture("comrak-readme.md");
    let doc = md_to_lex(&md);

    let root_sessions: Vec<_> = doc
        .root
        .children
        .iter()
        .filter_map(|el| match el {
            ContentItem::Session(s) => Some(s),
            _ => None,
        })
        .collect();

    // Comrak README has sections: Installation, Usage, Security, Extensions, Plugins, etc.
    assert!(
        root_sessions.len() >= 5,
        "Comrak README should have at least 5 top-level sections, found {}",
        root_sessions.len()
    );

    let titles: Vec<String> = root_sessions
        .iter()
        .map(|s| s.title.as_string().to_string())
        .collect();
    assert!(
        titles.iter().any(|t| t.contains("Installation")),
        "Should have Installation section, got: {titles:?}"
    );
    assert!(
        titles.iter().any(|t| t.contains("Extensions")),
        "Should have Extensions section, got: {titles:?}"
    );

    // Should have code blocks (Comrak README has multiple)
    fn count_verbatims(items: &[ContentItem]) -> usize {
        items
            .iter()
            .map(|el| match el {
                ContentItem::VerbatimBlock(_) => 1,
                ContentItem::Session(s) => count_verbatims(&s.children),
                _ => 0,
            })
            .sum()
    }
    let verbatim_count = count_verbatims(&doc.root.children);
    assert!(
        verbatim_count >= 3,
        "Comrak README should have at least 3 code blocks, found {verbatim_count}"
    );

    // Should have lists (extension lists, CLI options, etc.)
    fn count_lists(items: &[ContentItem]) -> usize {
        items
            .iter()
            .map(|el| match el {
                ContentItem::List(_) => 1,
                ContentItem::Session(s) => count_lists(&s.children),
                _ => 0,
            })
            .sum()
    }
    let list_count = count_lists(&doc.root.children);
    assert!(
        list_count >= 2,
        "Comrak README should have at least 2 lists, found {list_count}"
    );
}

// ============================================================================
// LEX OUTPUT VALIDITY
// ============================================================================
//
// These tests verify that Markdown→Lex serialization produces valid Lex that
// lex-core's parser accepts. This catches a class of bugs where lex-babel
// generates syntactically broken output.

#[test]
fn test_lex_validity_simple_elements() {
    assert_lex_output_valid("Simple paragraph.\n", "paragraph");
    assert_lex_output_valid("## Heading\n\nContent.\n", "heading");
    assert_lex_output_valid("- One\n- Two\n- Three\n", "list");
    assert_lex_output_valid("```rust\nfn main() {}\n```\n", "code_block");
    assert_lex_output_valid(
        "This is **bold** and *italic* and `code` text.\n",
        "inline_formatting",
    );
}

#[test]
fn test_lex_validity_complex_document() {
    let md = "\
## Introduction

First paragraph with **bold** and *italic*.

### Subsection

- Item one
- Item two
  - Nested item

```python
def hello():
    print('world')
```

## Another Section

Final paragraph.
";
    assert_lex_output_valid(md, "complex_document");
}

/// lex-core <= 0.2.2 panics on empty-subject definitions ("Cannot compute
/// bounding box from empty token list"). Fixed in lex-fmt/core@c7c78591.
/// Remove #[ignore] once lex-core is updated past 0.2.2.
#[test]
#[ignore = "needs lex-core > 0.2.2 (fix: lex-fmt/core#c7c78591)"]
fn test_lex_validity_reference_fixtures() {
    let commonmark = read_fixture("markdown-reference-commonmark.md");
    assert_lex_output_valid(&commonmark, "commonmark_reference");

    let comrak = read_fixture("markdown-reference-comrak.md");
    assert_lex_output_valid(&comrak, "comrak_reference");
}

/// See `test_lex_validity_reference_fixtures` for details.
#[test]
#[ignore = "needs lex-core > 0.2.2 (fix: lex-fmt/core#c7c78591)"]
fn test_lex_validity_comrak_readme() {
    let readme = read_fixture("comrak-readme.md");
    assert_lex_output_valid(&readme, "comrak_readme");
}

#[test]
fn test_lex_validity_trifecta_round_trips() {
    // Lex → Markdown → Lex text → lex-core parse
    for (name, path) in [
        (
            "trifecta_010",
            "../../comms/specs/trifecta/010-paragraphs-sessions-flat-single.lex",
        ),
        (
            "trifecta_020",
            "../../comms/specs/trifecta/020-paragraphs-sessions-flat-multiple.lex",
        ),
        (
            "trifecta_060",
            "../../comms/specs/trifecta/060-trifecta-nesting.lex",
        ),
    ] {
        let lex_src =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
        let lex_doc = lex_core::lex::transforms::standard::STRING_TO_AST
            .run(lex_src.to_string())
            .unwrap();
        let md = MarkdownFormat.serialize(&lex_doc).unwrap();
        assert_lex_output_valid(&md, name);
    }
}

#[test]
fn test_lex_validity_kitchensink() {
    let lex_src = std::fs::read_to_string("../../comms/specs/benchmark/010-kitchensink.lex")
        .expect("kitchensink file should exist");
    let lex_doc = lex_core::lex::transforms::standard::STRING_TO_AST
        .run(lex_src.to_string())
        .unwrap();
    let md = MarkdownFormat.serialize(&lex_doc).unwrap();
    assert_lex_output_valid(&md, "kitchensink");
}
