use lex_babel::format::Format;
use lex_babel::formats::markdown::MarkdownFormat;
use lex_core::lex::ast::ContentItem;

#[test]
fn test_frontmatter_import() {
    let md = r#"---
title: My Document
author: Me
---

# Content
Start of document.
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");

    // Expecting an Annotation with label "frontmatter" at the root
    let root_children = &doc.root.children;
    assert!(!root_children.is_empty());

    let first = &root_children[0];
    if let ContentItem::Annotation(ann) = first {
        assert_eq!(ann.data.label.value, "frontmatter");

        // Check parameters
        let title = ann
            .data
            .parameters
            .iter()
            .find(|p| p.key == "title")
            .map(|p| p.value.as_str());
        assert_eq!(title, Some("My Document"));

        let author = ann
            .data
            .parameters
            .iter()
            .find(|p| p.key == "author")
            .map(|p| p.value.as_str());
        assert_eq!(author, Some("Me"));
    } else {
        panic!("Expected frontmatter annotation as first item");
    }
}

#[test]
fn test_frontmatter_export() {
    let md = r#"---
title: Export Test
tags: [a, b]
---

Content.
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");
    let output = MarkdownFormat
        .serialize(&doc)
        .expect("Failed to serialize markdown");

    println!("Output:\n{output}");

    assert!(output.starts_with("---\n"));
    assert!(output.contains("title: Export Test"));
    assert!(output.contains("tags: [a, b]"));
    assert!(output.contains("---\n\nContent."));
}
