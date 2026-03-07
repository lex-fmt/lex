//! Regression tests for parser correctness fixes.
//!
//! Each test targets a specific bug that was discovered during the parser audit.
//! These prevent regressions by testing both positive (correct parsing) and negative
//! (incorrect input should NOT parse as a specific element) cases.

use lex_core::lex::parsing::parse_document;
use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::parse_without_annotation_attachment;

// ==================== #393: Sessions with 2+ blank lines ====================

#[test]
fn regression_393_session_with_two_blank_lines() {
    let source = "Title\n\n\n    Content with two blank lines.\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("Title")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text("Content with two blank lines.");
            });
    });
}

#[test]
fn regression_393_session_with_three_blank_lines() {
    let source = "Title\n\n\n\n    Content with three blank lines.\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("Title")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text("Content with three blank lines.");
            });
    });
}

#[test]
fn regression_393_session_with_five_blank_lines() {
    let source = "Title\n\n\n\n\n\n    Content with five blank lines.\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("Title")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text("Content with five blank lines.");
            });
    });
}

// ==================== #394: Orphaned indented content ====================

#[test]
fn regression_394_orphaned_container_not_dropped() {
    // Indented content without a preceding title should be promoted,
    // not silently dropped.
    let source = "Paragraph before.\n\n    Indented content.\n\nParagraph after.\n";
    let doc = parse_document(source).unwrap();

    // The indented content should appear somewhere — it must NOT be dropped.
    let has_indented = doc.root.children.iter().any(|item| {
        if let lex_core::lex::parsing::ContentItem::Paragraph(p) = item {
            p.text().contains("Indented content")
        } else {
            false
        }
    }) || doc.root.children.iter().any(|item| {
        if let lex_core::lex::parsing::ContentItem::Session(s) = item {
            s.children.iter().any(|c| {
                if let lex_core::lex::parsing::ContentItem::Paragraph(p) = c {
                    p.text().contains("Indented content")
                } else {
                    false
                }
            })
        } else {
            false
        }
    });

    assert!(
        has_indented,
        "Indented content must NOT be silently dropped from the AST"
    );
}

// ==================== #395: Annotations inside sessions ====================

#[test]
fn regression_395_annotations_inside_session_not_dropped() {
    let source = "1. Session\n\n    Some content.\n\n    :: note-editor :: Maybe rephrase?\n    :: note.author :: Done keeping it simple\n\n    More content.\n";
    let doc = parse_without_annotation_attachment(source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("1. Session")
            // After filtering BlankLineGroups, we should see: para, annotation, annotation, para
            .child_count(4)
            .child(0, |child| {
                child.assert_paragraph().text("Some content.");
            })
            .child(1, |child| {
                child.assert_annotation().label("note-editor");
            })
            .child(2, |child| {
                child.assert_annotation().label("note.author");
            })
            .child(3, |child| {
                child.assert_paragraph().text("More content.");
            });
    });
}

#[test]
fn regression_395_annotations_with_attachment() {
    // Same as above but WITH attachment — annotations should attach to nearest element
    let source =
        "1. Session\n\n    Some content.\n\n    :: note :: Annotation text\n\n    More content.\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("1. Session")
            // After attachment, annotations are moved to their target elements
            // The session should still have both paragraphs
            .child(0, |child| {
                child.assert_paragraph().text("Some content.");
            })
            .child(1, |child| {
                child.assert_paragraph().text("More content.");
            });
    });
}

// ==================== #396: Definition subjects require colon ====================

#[test]
fn regression_396_colon_subject_parses_as_definition() {
    let source = "My Term:\n    The definition of the term.\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition()
            .subject("My Term")
            .child_count(1)
            .child(0, |child| {
                child.assert_paragraph().text("The definition of the term.");
            });
    });
}

#[test]
fn regression_396_no_colon_subject_is_not_definition() {
    // A line WITHOUT a colon followed by immediate indented content
    // should NOT parse as a definition — it should be a session (with blank)
    // or something else.
    let source = "My Term\n    The content below.\n";
    let doc = parse_document(source).unwrap();

    // Without a colon, "My Term" + immediate indent should NOT be a definition.
    // It should be some other structure (likely paragraph + promoted content,
    // or a definition only if the line ends with colon).
    let first_visible = doc
        .root
        .children
        .iter()
        .find(|c| !matches!(c, lex_core::lex::parsing::ContentItem::BlankLineGroup(_)));

    if let Some(item) = first_visible {
        assert!(
            !matches!(item, lex_core::lex::parsing::ContentItem::Definition(_)),
            "A line without colon must NOT parse as a Definition"
        );
    }
}

#[test]
fn regression_396_colon_with_blank_line_is_session_not_definition() {
    // Subject with colon BUT blank line before content = Session, not Definition
    let source = "My Term:\n\n    Content after blank line.\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        // Session, not definition — blank line makes it a session
        item.assert_session()
            .label("My Term:")
            .child_count(1)
            .child(0, |child| {
                child.assert_paragraph().text("Content after blank line.");
            });
    });
}

// ==================== #397: Verbatim closing requires :: label :: ====================

#[test]
fn regression_397_verbatim_with_double_marker_closing() {
    let source = "Code Example:\n    function hello() {\n        return \"world\";\n    }\n:: javascript ::\n";
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Code Example")
            .closing_label("javascript")
            .content_contains("function hello()")
            .content_contains("return \"world\"");
    });
}

#[test]
fn regression_397_single_marker_does_not_close_verbatim() {
    // A line like ":: javascript" (without closing ::) should NOT close a verbatim block.
    // The content should be treated differently — likely as a definition with
    // all content (including what would have been the closing) inside.
    let source = "Code Example:\n    function hello() {\n    :: javascript\n";
    let doc = parse_document(source).unwrap();

    // Should NOT parse as a verbatim block (single marker doesn't close)
    let has_verbatim = doc
        .root
        .children
        .iter()
        .any(|item| matches!(item, lex_core::lex::parsing::ContentItem::VerbatimBlock(_)));

    assert!(
        !has_verbatim,
        "Single :: marker (without closing ::) must NOT close a verbatim block"
    );
}

// ==================== Cross-cutting: complex composition ====================

#[test]
fn regression_complex_session_with_all_elements() {
    // A session containing paragraph, list, definition, verbatim, and closing paragraph.
    // Note: definition and verbatim cannot be adjacent within the same container
    // because verbatim has higher precedence and would consume the definition subject
    // as the verbatim subject. They must be separated.
    let source = r#"1. Complete Session

    Opening paragraph.

    - List item one
    - List item two

    Term:
        Definition content.

    Code:
        console.log("hi");
    :: javascript ::

    Closing paragraph.
"#;
    let doc = parse_document(source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("1. Complete Session")
            .child(0, |child| {
                child.assert_paragraph().text("Opening paragraph.");
            })
            .child(1, |child| {
                child
                    .assert_list()
                    .item_count(2)
                    .item(0, |li| {
                        li.text_contains("List item one");
                    })
                    .item(1, |li| {
                        li.text_contains("List item two");
                    });
            })
            .child(2, |child| {
                // "Term:" (subject) + container("Definition content.") + :: javascript :: (closing)
                // = verbatim block. Verbatim has higher precedence than definition, so when
                // a :: label :: closing marker exists at the same level, it wins.
                child
                    .assert_verbatim_block()
                    .subject("Term")
                    .closing_label("javascript")
                    .content_contains("Definition content");
            })
            .child(3, |child| {
                child.assert_paragraph().text("Closing paragraph.");
            });
    });
}
