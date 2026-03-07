//! Unit tests for isolated annotation elements
//!
//! Tests annotation parsing in isolation following the on-lexplore.lex guidelines:
//! - Use Lexplore to load centralized test files
//! - Use assert_ast for deep structure verification
//! - Test isolated elements (one element per test)
//! - Verify content and structure, not just counts

use lex_core::lex::parsing::{parse_document, ContentItem, Document};
use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;
use lex_core::lex::testing::parse_without_annotation_attachment;
use lex_core::lex::testing::workspace_path;

/// Helper to parse annotation files by number without running annotation attachment
/// (so annotations remain in content tree for testing)
fn parse_annotation_without_attachment(
    number: usize,
) -> Result<Document, Box<dyn std::error::Error>> {
    let source = Lexplore::annotation(number).source();
    parse_without_annotation_attachment(&source)
        .map_err(|e| Box::new(std::io::Error::other(e)) as Box<dyn std::error::Error>)
}

#[test]
fn test_annotation_01_flat_marker_simple() {
    // annotation-01-flat-marker-simple.lex: Simple marker annotation ":: note ::"
    let doc = parse_annotation_without_attachment(1).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_annotation().label("note");
    });
}

#[test]
fn test_annotation_02_flat_marker_with_params() {
    // annotation-02-flat-marker-with-params.lex: Marker with parameter "severity=high"
    let doc = parse_annotation_without_attachment(2).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_annotation()
            .label("warning")
            .parameter_count(1)
            .parameter(0, "severity", "high");
    });
}

#[test]
fn test_annotation_03_flat_inline_text() {
    // annotation-03-flat-inline-text.lex: Single-line annotation with inline text
    let doc = parse_annotation_without_attachment(3).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_annotation()
            .label("note")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("Important information");
            });
    });
}

#[test]
fn test_annotation_04_flat_inline_with_params() {
    // annotation-04-flat-inline-with-params.lex: Single-line annotation with params and inline text
    let doc = parse_annotation_without_attachment(4).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_annotation()
            .label("warning")
            .parameter_count(1)
            .parameter(0, "severity", "high")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("Check this carefully");
            });
    });
}

#[test]
fn test_annotation_05_flat_block_paragraph() {
    // annotation-05-flat-block-paragraph.lex: Block annotation with paragraph content
    let doc = parse_annotation_without_attachment(5).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_annotation()
            .label("note")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("important note that requires a detailed explanation");
            });
    });
}

#[test]
fn test_annotation_06_flat_block_multi_paragraph() {
    // annotation-06-flat-block-multi-paragraph.lex: Block annotation spanning two paragraphs
    let doc = parse_annotation_without_attachment(6).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_annotation()
            .label("note")
            .parameter_count(1)
            .parameter(0, "author", "\"Jane Doe\"")
            .child_count(2)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("important note that requires a detailed explanation");
            })
            .child(1, |child| {
                child
                    .assert_paragraph()
                    .text_contains("span multiple paragraphs");
            });
    });
}

#[test]
fn test_annotation_07_flat_block_with_list() {
    // annotation-07-flat-block-with-list.lex: Block annotation mixing paragraph and list content
    let doc = parse_annotation_without_attachment(7).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_annotation()
            .label("warning")
            .parameter_count(1)
            .parameter(0, "severity", "critical")
            .child_count(2)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("The following items must be addressed before deployment");
            })
            .child(1, |child| {
                child
                    .assert_list()
                    .item_count(3)
                    .item(0, |li| {
                        li.text_contains("Security vulnerabilities");
                    })
                    .item(1, |li| {
                        li.text_contains("Performance issues");
                    })
                    .item(2, |li| {
                        li.text_contains("Documentation gaps");
                    });
            });
    });
}

#[test]
fn test_annotation_08_nested_with_list_and_paragraph() {
    // annotation-08-nested-with-list-and-paragraph.lex: Paragraph + list + paragraph inside annotation
    let doc = parse_annotation_without_attachment(8).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_annotation()
            .label("note")
            .parameter_count(1)
            .parameter(0, "author", "\"Jane\"")
            .child_count(3)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("multiple types of content");
            })
            .child(1, |child| {
                child
                    .assert_list()
                    .item_count(3)
                    .item(0, |li| {
                        li.text_contains("First item");
                    })
                    .item(1, |li| {
                        li.text_contains("Second item");
                    })
                    .item(2, |li| {
                        li.text_contains("Third item");
                    });
            })
            .child(2, |child| {
                child
                    .assert_paragraph()
                    .text_contains("A paragraph after the list");
            });
    });
}

#[test]
fn test_annotation_09_nested_definition_inside() {
    // annotation-09-nested-definition-inside.lex: Definition entries inside annotation block
    let doc = parse_annotation_without_attachment(9).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_annotation()
            .label("documentation")
            .child_count(4)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("documents some terms");
            })
            .child(1, |child| {
                child
                    .assert_definition()
                    .subject("API")
                    .child(0, |def_para| {
                        def_para
                            .assert_paragraph()
                            .text_contains("Application Programming Interface");
                    });
            })
            .child(2, |child| {
                child
                    .assert_definition()
                    .subject("REST")
                    .child(0, |def_para| {
                        def_para
                            .assert_paragraph()
                            .text_contains("Representational State Transfer");
                    });
            })
            .child(3, |child| {
                child.assert_paragraph().text_contains("Final notes");
            });
    });
}

#[test]
fn test_annotation_10_nested_complex() {
    // annotation-10-nested-complex.lex: Mixed paragraphs, nested lists, and parameters
    let doc = parse_annotation_without_attachment(10).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_annotation()
            .label("review")
            .parameter_count(1)
            .parameter(0, "status", "pending")
            .child_count(4)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("Review findings and recommendations");
            })
            .child(1, |child| {
                child.assert_paragraph().text("Issues Found:");
            })
            .child(2, |child| {
                child
                    .assert_list()
                    .item_count(2)
                    .item(0, |li| {
                        li.text_contains("Performance bottleneck")
                            .child_count(2)
                            .child(0, |para| {
                                para.assert_paragraph()
                                    .text_contains("needs immediate attention");
                            })
                            .child(1, |nested| {
                                nested
                                    .assert_list()
                                    .item_count(2)
                                    .item(0, |inner| {
                                        inner.text_contains("Memory leak in handler");
                                    })
                                    .item(1, |inner| {
                                        inner.text_contains("Slow database queries");
                                    });
                            });
                    })
                    .item(1, |li| {
                        li.text_contains("Security concerns")
                            .child_count(1)
                            .child(0, |para| {
                                para.assert_paragraph().text_contains("Review required");
                            });
                    });
            })
            .child(3, |child| {
                child
                    .assert_paragraph()
                    .text_contains("Conclusion paragraph");
            });
    });
}

#[test]
fn test_annotation_requires_label() {
    let doc = parse_document(":: severity=high ::\n").expect("parser should succeed");

    let has_annotation = doc
        .root
        .children
        .iter()
        .any(|item| matches!(item, ContentItem::Annotation(_)));

    assert!(
        !has_annotation,
        "Parameter-only annotations must not be recognized as Annotation nodes"
    );
}

#[test]
fn test_annotations_overview_document() {
    // annotation.lex: Specification overview document for annotations
    let doc = Lexplore::from_path(workspace_path("specs/v1/elements/annotation.lex"))
        .parse()
        .unwrap();

    assert_ast(&doc)
        .item(0, |item| {
            item.assert_session()
                .label("Introduction")
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("Annotations are a core element");
                })
                .child(1, |child| {
                    child.assert_paragraph().text_contains("provide labels");
                })
                .child(2, |child| {
                    child.assert_paragraph().text("Core features:");
                })
                .child(3, |child| {
                    child.assert_list().item_count(4);
                });
        })
        .item(1, |item| {
            item.assert_session().label("Syntax Forms:");
        });
}
