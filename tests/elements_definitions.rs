//! Unit tests for isolated definition elements
//!
//! Tests definition parsing in isolation following the on-lexplore.lex guidelines:
//! - Use Lexplore to load centralized test files
//! - Use assert_ast for deep structure verification
//! - Test isolated elements (one element per test)
//! - Verify content and structure, not just counts

use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;
use lex_core::lex::testing::workspace_path;

#[test]
fn test_definition_01_flat_simple() {
    // definition-01-flat-simple.lex: Definition with single paragraph content
    let doc = Lexplore::definition(1).parse().unwrap();

    // Document: Definition + trailing paragraph "Something to finish the element"
    assert_ast(&doc)
        .item_count(2)
        .item(0, |item| {
            item.assert_definition()
                .subject("Cache")
                .child_count(1)
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("Temporary storage for frequently accessed data");
                });
        })
        .item(1, |item| {
            item.assert_paragraph()
                .text_contains("Something to finish the element");
        });
}

#[test]
fn test_definition_02_flat_multi_paragraph() {
    // definition-02-flat-multi-paragraph.lex: Definition with multiple paragraphs
    let doc = Lexplore::definition(2).parse().unwrap();

    assert_ast(&doc)
        .item_count(2)
        .item(0, |item| {
            item.assert_definition()
                .subject("Microservice")
                .child_count(2) // Two paragraphs in definition
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("architectural style");
                })
                .child(1, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("independently deployable");
                });
        })
        .item(1, |item| {
            item.assert_paragraph()
                .text_contains("Something to finish the element");
        });
}

#[test]
fn test_definition_03_flat_with_list() {
    // definition-03-flat-with-list.lex: Due to blank line after "HTTP Methods:",
    // parser treats this as an unnumbered Session, not a Definition.
    // Definitions require content immediately after the colon (no blank line).
    let doc = Lexplore::definition(3).parse().unwrap();

    assert_ast(&doc)
        .item_count(2)
        .item(0, |item| {
            // Parsed as Session because of blank line after colon
            item.assert_session()
                .label("HTTP Methods:")
                .child_count(1)
                .child(0, |child| {
                    child
                        .assert_list()
                        .item_count(4)
                        .item(0, |list_item| {
                            list_item.text_contains("GET: Retrieve resources");
                        })
                        .item(1, |list_item| {
                            list_item.text_contains("POST: Create resources");
                        })
                        .item(2, |list_item| {
                            list_item.text_contains("PUT: Update resources");
                        })
                        .item(3, |list_item| {
                            list_item.text_contains("DELETE: Remove resources");
                        });
                });
        })
        .item(1, |item| {
            item.assert_paragraph()
                .text_contains("Something to finish the element");
        });
}

#[test]
fn test_definition_05_nested_with_list() {
    // definition-05-nested-with-list.lex: Definition with paragraphs + list content
    let doc = Lexplore::definition(5).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition()
            .subject("Programming Concepts")
            .child_count(3)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("fundamental ideas in programming");
            })
            .child(1, |child| {
                child
                    .assert_list()
                    .item_count(3)
                    .item(0, |li| {
                        li.text_contains("Variables");
                    })
                    .item(1, |li| {
                        li.text_contains("Functions");
                    })
                    .item(2, |li| {
                        li.text_contains("Loops");
                    });
            })
            .child(2, |child| {
                child
                    .assert_paragraph()
                    .text_contains("core building blocks");
            });
    });
}

#[test]
fn test_definition_06_nested_definitions() {
    // definition-06-nested-definitions.lex: Nested definition hierarchy (Authentication -> OAuth -> OAuth 2.0)
    let doc = Lexplore::definition(6).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition()
            .subject("Authentication")
            .child(0, |child| {
                child.assert_paragraph().text_contains("verifying identity");
            })
            .child(1, |child| {
                child
                    .assert_definition()
                    .subject("OAuth")
                    .child(0, |grandchild| {
                        grandchild
                            .assert_paragraph()
                            .text_contains("access delegation");
                    })
                    .child(1, |grandchild| {
                        grandchild
                            .assert_definition()
                            .subject("OAuth 2.0")
                            .child(0, |leaf| {
                                leaf.assert_paragraph().text_contains("current version");
                            });
                    });
            })
            .child(2, |child| {
                child.assert_definition().subject("JWT").child(0, |leaf| {
                    leaf.assert_paragraph().text_contains("JSON Web Tokens");
                });
            });
    });
}

#[test]
fn test_definition_07_nested_deep_only() {
    // definition-07-nested-deep-only.lex: Deeply nested definitions chain
    let doc = Lexplore::definition(7).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition()
            .subject("Computer Science")
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("computation and information");
            })
            .child(1, |child| {
                child
                    .assert_definition()
                    .subject("Algorithms")
                    .child(0, |grandchild| {
                        grandchild
                            .assert_paragraph()
                            .text_contains("Step-by-step procedures");
                    })
                    .child(1, |grandchild| {
                        grandchild
                            .assert_definition()
                            .subject("Sorting")
                            .child(0, |leaf| {
                                leaf.assert_paragraph().text_contains("Organizing data");
                            })
                            .child(1, |leaf| {
                                leaf.assert_definition()
                                    .subject("QuickSort")
                                    .child(0, |detail| {
                                        detail
                                            .assert_paragraph()
                                            .text_contains("divide-and-conquer");
                                    });
                            });
                    });
            });
    });
}

#[test]
fn test_definition_90_document_simple() {
    let doc = Lexplore::definition(90).parse().unwrap();

    // Ensemble document: title paragraph, description paragraph, definition, 3 sessions,
    // trailing paragraph, trailing definition, final paragraph
    assert_ast(&doc)
        .title("Ensemble Test with Definitions {{paragraph}}")
        .item(0, |item| {
            item.assert_paragraph()
                .text_contains("tests all core elements");
        })
        .item(1, |item| {
            item.assert_definition()
                .subject("Introduction")
                .child_count(2)
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("demonstrates how definitions integrate");
                })
                .child(1, |child| {
                    child
                        .assert_list()
                        .item_count(4)
                        .item(0, |li| {
                            li.text_contains("Paragraphs provide narrative content");
                        })
                        .item(3, |li| {
                            li.text_contains("Definitions explain terms");
                        });
                });
        })
        .item(2, |item| {
            // "1. Simple Elements Section"
            item.assert_session()
                .label("1. Simple Elements Section {{session}}")
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("each element in isolation");
                })
                .child(1, |child| {
                    child
                        .assert_definition()
                        .subject("API Endpoint")
                        .child(0, |def_para| {
                            def_para
                                .assert_paragraph()
                                .text_contains("URL that provides access");
                        });
                })
                .child(2, |child| {
                    child
                        .assert_definition()
                        .subject("Database Types")
                        .child(0, |def_list| {
                            def_list.assert_list().item_count(3).item(0, |li| {
                                li.text_contains("Relational databases");
                            });
                        });
                })
                .child(3, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("simple list at the session level");
                })
                .child(4, |child| {
                    child.assert_list().item_count(3).item(0, |li| {
                        li.text_contains("First item");
                    });
                });
        })
        .item(3, |item| {
            // "2. Nested Elements Section"
            item.assert_session()
                .label("2. Nested Elements Section {{session}}")
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("more complex nesting patterns");
                })
                .child(1, |child| {
                    // "2.1. Subsection with Definitions"
                    child
                        .assert_session()
                        .label("2.1. Subsection with Definitions {{session}}")
                        .child(0, |sc| {
                            sc.assert_definition()
                                .subject("Microservice")
                                .child(0, |p| {
                                    p.assert_paragraph().text_contains("architectural style");
                                });
                        });
                });
        })
        .item(4, |item| {
            // "3. Deep Nesting Section"
            item.assert_session()
                .label("3. Deep Nesting Section {{session}}")
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("deeper nesting levels");
                })
                .child(1, |child| {
                    child
                        .assert_session()
                        .label("3.1. Level One {{session}}")
                        .child(1, |grandchild| {
                            grandchild
                                .assert_definition()
                                .subject("Design Pattern")
                                .child(0, |p| {
                                    p.assert_paragraph().text_contains("reusable solution");
                                });
                        });
                });
        });
}

#[test]
fn test_definitions_overview_document() {
    // definitions.lex: Specification overview covering syntax/disambiguation
    let doc = Lexplore::from_path(workspace_path("specs/v1/elements/definition.lex"))
        .parse()
        .unwrap();

    // Verify the document parses successfully with expected structure
    assert_ast(&doc)
        .item_count(6)
        .item(0, |item| {
            item.assert_session().label("Introduction");
        })
        .item(1, |item| {
            item.assert_session().label("Syntax");
        })
        .item(2, |item| {
            item.assert_session().label("Disambiguation from Sessions");
        })
        .item(3, |item| {
            item.assert_session().label("Content Structure");
        })
        .item(4, |item| {
            item.assert_session().label("Block Termination");
        })
        .item(5, |item| {
            item.assert_session().label("Examples");
        });
}

#[test]
fn test_definition_10_session_nesting_issue() {
    // Minimal reproduction for "Definition cannot contain Session" error
    // Mimics structure from definitions.lex: Session containing multiple Definitions
    let doc = Lexplore::definition(10).parse().unwrap();

    // Expected structure:
    // Item 0: Session("Syntax") containing:
    //   - Definition("Subject") with content
    //   - Paragraph("Key rule...")
    //   - Definition("Subject line") with list
    //   - Definition("Content") with list
    // Item 1: Session("Disambiguation from Sessions") with paragraph
    assert_ast(&doc)
        .item_count(2)
        .item(0, |item| {
            item.assert_session().label("Syntax");
        })
        .item(1, |item| {
            item.assert_session()
                .label("Disambiguation from Sessions")
                .child_count(1)
                .child(0, |child| {
                    child.assert_paragraph().text_contains("Some content here");
                });
        });
}

#[test]
fn test_definition_100_document_nested_sessions() {
    // Tests that sessions require blank lines BEFORE the subject (not just after)
    // This prevents incorrect Session parsing inside Definitions
    let doc = Lexplore::definition(100).parse().unwrap();

    // Expected structure:
    // Session("Disambiguation from Sessions") containing:
    //   - Paragraph("Definitions vs Sessions - the blank line rule:")
    //   - BlankLineGroup
    //   - Definition("Definition (no blank line)") containing:
    //     - Definition("API Endpoint") with paragraph
    //   - Definition("Session (has blank line)") containing:
    //     - Paragraph("API Endpoint:") - correctly NOT a Session!
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("Disambiguation from Sessions")
            .child_count(3) // Paragraph, Definition, Definition
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("Definitions vs Sessions")
                    .text_contains("blank line rule");
            })
            .child(1, |child| {
                // "Definition (no blank line):" — is a definition
                child
                    .assert_definition()
                    .subject("Definition (no blank line)");
            })
            .child(2, |child| {
                // "Session (has blank line):" — also a definition at this level
                child
                    .assert_definition()
                    .subject("Session (has blank line)");
            });
    });
}
