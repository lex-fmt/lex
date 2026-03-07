//! Deep semantic assertion tests for the kitchensink benchmark document.
//!
//! This replaces the previous snapshot-only test with full structural verification.
//! Every element, its type, content, nesting, and annotations are validated against
//! the {{tag}} markers in the fixture file.

use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;

/// Parse the kitchensink benchmark document once for all tests in this module.
fn kitchensink_doc() -> lex_core::lex::parsing::Document {
    Lexplore::benchmark(10).parse().unwrap()
}

#[test]
fn kitchensink_document_title() {
    let doc = kitchensink_doc();
    assert_ast(&doc).title("Kitchensink Test Document {{paragraph}}");
}

#[test]
fn kitchensink_root_structure() {
    // Expected root items (excluding BlankLineGroups which are filtered by assert_ast):
    // [0] Paragraph — "This document includes..."
    // [1] Paragraph — two-lined paragraph
    // [2] Definition — "Root Definition:"
    // [3] Paragraph — "This is a marker annotation..."
    // [4] Session — "1. Primary Session"
    // [5] Session — "2. Second Root Session"
    // [6] Paragraph — "Final paragraph..."
    let doc = kitchensink_doc();
    assert_ast(&doc).item_count(7);
}

#[test]
fn kitchensink_opening_paragraphs() {
    let doc = kitchensink_doc();

    // [0] Single-line paragraph with inline formatting and citation
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph()
            .text_contains("all major features")
            .text_contains("kitchensink")
            .text_contains("regression test")
            .line_count(1);
    });

    // [1] Two-lined paragraph
    assert_ast(&doc).item(1, |item| {
        item.assert_paragraph()
            .text_contains("two-lined paragraph")
            .text_contains("simple _definition_ at the root level")
            .line_count(2);
    });
}

#[test]
fn kitchensink_root_definition() {
    let doc = kitchensink_doc();

    // [2] "Root Definition:" with paragraph + list
    assert_ast(&doc).item(2, |item| {
        item.assert_definition()
            .subject("Root Definition")
            .child_count(2)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("contains a paragraph and a `list`")
                    .text_contains("mixed content at the top level")
                    .line_count(1);
            })
            .child(1, |child| {
                child
                    .assert_list()
                    .item_count(2)
                    .item(0, |li| {
                        li.text_contains("Item 1 in definition");
                    })
                    .item(1, |li| {
                        li.text_contains("Item 2 in definition");
                    });
            });
    });
}

#[test]
fn kitchensink_marker_annotation_paragraph() {
    let doc = kitchensink_doc();

    // [3] Plain text line (not an annotation — no :: markers)
    assert_ast(&doc).item(3, |item| {
        item.assert_paragraph()
            .text(
                "This is a marker annotation at the root level, attached to the definition above.",
            )
            .line_count(1);
    });
}

#[test]
fn kitchensink_primary_session_structure() {
    let doc = kitchensink_doc();

    // [4] "1. Primary Session" — main container
    assert_ast(&doc).item(4, |item| {
        item.assert_session()
            .label("1. Primary Session {{session}}")
            .child_count(5) // para, list, nested session, para, verbatim
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("main container for testing nested structures")
                    .line_count(1);
            })
            .child(1, |child| {
                child
                    .assert_list()
                    .item_count(2)
                    .annotation_count(1) // :: warning severity=high :: attached to list
                    .annotation(0, |ann| {
                        ann.label("warning")
                            .has_parameter_with_value("severity", "high")
                            .child_count(1)
                            .child(0, |ann_child| {
                                ann_child.assert_paragraph().text_contains(
                                    "This is a single-line annotation inside the session.",
                                );
                            });
                    })
                    .item(0, |li| {
                        li.text_contains("Followed by a simple list");
                    })
                    .item(1, |li| {
                        li.text_contains("This list has two items");
                    });
            });
    });
}

#[test]
fn kitchensink_nested_session() {
    let doc = kitchensink_doc();

    // [4].child[2]: "1.1. Nested Session (Level 2)"
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(2, |nested| {
            nested
                .assert_session()
                .label("1.1. Nested Session (Level 2) {{session}}")
                .child_count(3) // para, definition, list at session level
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("second-level session")
                        .text_contains("definition and a list")
                        .line_count(1);
                })
                .child(1, |child| {
                    // "Nested Definition:" with paragraph and list content
                    child
                        .assert_definition()
                        .subject("Nested Definition")
                        .child_count(2)
                        .child(0, |def_para| {
                            def_para
                                .assert_paragraph()
                                .text_contains("inside a nested session")
                                .text_contains("contains a list");
                        })
                        .child(1, |def_list| {
                            def_list
                                .assert_list()
                                .item_count(2)
                                .item(0, |li| {
                                    li.text_contains("List inside a nested definition");
                                })
                                .item(1, |li| {
                                    li.text_contains("Second item");
                                });
                        });
                })
                .child(2, |child| {
                    // List at session level 2 (after the definition)
                    child
                        .assert_list()
                        .item_count(2)
                        .item(0, |li| {
                            li.text_contains("A list item at level 2")
                                .child_count(2)
                                .child(0, |nested_para| {
                                    nested_para
                                        .assert_paragraph()
                                        .text_contains("contains a nested paragraph");
                                })
                                .child(1, |nested_list| {
                                    nested_list
                                        .assert_list()
                                        .item_count(2)
                                        .item(0, |inner| {
                                            inner.text_contains("And a nested list (Level 3)");
                                        })
                                        .item(1, |inner| {
                                            inner.text_contains("With its own items");
                                        });
                                });
                        })
                        .item(1, |li| {
                            li.text_contains("Another list item at level 2");
                        });
                });
        });
    });
}

#[test]
fn kitchensink_session_back_paragraph_and_verbatim() {
    let doc = kitchensink_doc();

    // [4].child[3]: "A paragraph back at the first level of nesting"
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(3, |child| {
            child
                .assert_paragraph()
                .text("A paragraph back at the first level of nesting. {{paragraph}}")
                .line_count(1);
        });
    });

    // [4].child[4]: Verbatim block — "Code Example (Verbatim Block):"
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(4, |child| {
            child
                .assert_verbatim_block()
                .subject("Code Example (Verbatim Block)")
                .closing_label("javascript")
                .content_contains("function example()")
                .content_contains("return \"lex\"");
        });
    });
}

#[test]
fn kitchensink_second_session() {
    let doc = kitchensink_doc();

    // [5] "2. Second Root Session"
    assert_ast(&doc).item(5, |item| {
        item.assert_session()
            .label("2. Second Root Session {{session}}")
            .child_count(2) // paragraph, verbatim marker
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("tests annotations with block content")
                    .text_contains("marker-style verbatim blocks")
                    .line_count(1);
            })
            .child(1, |child| {
                // Marker verbatim block: "Image Reference (Marker Verbatim Block):"
                // The block annotation (:: todo ::) gets attached to this verbatim block
                // by the annotation attachment phase (nearest-element heuristic).
                child
                    .assert_verbatim_block()
                    .subject("Image Reference (Marker Verbatim Block)")
                    .closing_label("image")
                    .has_closing_parameter_with_value("src", "\"logo.png\"")
                    .has_closing_parameter_with_value("alt", "\"Lex Logo\"")
                    .annotation_count(1)
                    .annotation(0, |ann| {
                        ann.label("todo")
                            .has_parameter_with_value("status", "\"open\"")
                            .has_parameter_with_value("assignee", "\"team\"")
                            .child_count(3) // 2 paragraphs + list
                            .child(0, |ann_child| {
                                ann_child
                                    .assert_paragraph()
                                    .text_contains("This is a block annotation");
                            })
                            .child(1, |ann_child| {
                                ann_child
                                    .assert_paragraph()
                                    .text_contains("contains a paragraph and a list");
                            })
                            .child(2, |ann_child| {
                                ann_child
                                    .assert_list()
                                    .item_count(2)
                                    .item(0, |li| {
                                        li.text_contains("Task 1 to complete");
                                    })
                                    .item(1, |li| {
                                        li.text_contains("Task 2 to complete");
                                    });
                            });
                    });
            });
    });
}

#[test]
fn kitchensink_final_paragraph() {
    let doc = kitchensink_doc();

    // [6] Final paragraph
    assert_ast(&doc).item(6, |item| {
        item.assert_paragraph()
            .text("Final paragraph at the end of the document. {{paragraph}}")
            .line_count(1);
    });
}
