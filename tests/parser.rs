//! Integration tests for the parser.

use lex_core::lex::parsing::{parse_document, ContentItem};
use lex_core::lex::testing::lexplore::Lexplore;
use lex_core::lex::testing::workspace_path;
use lex_core::lex::testing::{
    assert_ast, InlineAssertion, InlineExpectation, ReferenceExpectation, TextMatch,
};

#[test]
fn test_real_content_extraction() {
    // Test that we extract real content, not placeholder strings
    let input = "First paragraph with numbers 123 and symbols (like this).\n\nSecond paragraph.\n\n1. Session Title\n\n    Session content here.\n\n";

    let doc = parse_document(input).expect("Failed to parse");

    assert_ast(&doc)
        .item_count(2)
        .item(0, |item| {
            item.assert_paragraph()
                .text("Second paragraph.")
                .line_count(1);
        })
        .item(1, |item| {
            item.assert_session()
                .label("1. Session Title")
                .child_count(1)
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text("Session content here.")
                        .line_count(1);
                });
        });
}

#[test]
fn test_dialog_parsing() {
    // Tests that dash-prefixed lines without proper list formatting are parsed as a paragraph
    let source = Lexplore::paragraph(9).source();
    let doc = parse_document(&source).unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_paragraph()
            .text("- Hi mom!!.\n- Hi kiddo.")
            .line_count(2);
    });
}

// Session tests have been moved to elements/sessions.rs
// List tests have been moved to elements/lists.rs
// Definition tests have been moved to elements/definitions.rs

// ==================== TRIFECTA TESTS ====================
// Testing paragraphs + sessions + lists together

#[test]
fn test_trifecta_000_paragraphs() {
    // Test simple paragraphs only document
    let source = Lexplore::trifecta(0).source();
    let doc = parse_document(&source).unwrap();

    // Should have 6 paragraphs total
    assert_ast(&doc).item_count(6);

    // Item 0: Single line paragraph
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph()
            .text("This is a simple paragraph with just one line. {{paragraph}}")
            .line_count(1);
    });

    // Item 1: Multi-line paragraph (3 lines)
    assert_ast(&doc).item(1, |item| {
        item.assert_paragraph()
            .text_contains("multi-line paragraph")
            .text_contains("second line")
            .text_contains("third line")
            .text_contains("{{paragraph}}")
            .line_count(3);
    });

    // Item 2: Paragraph after blank line
    assert_ast(&doc).item(2, |item| {
        item.assert_paragraph()
            .text("Another paragraph follows after a blank line. {{paragraph}}")
            .line_count(1);
    });

    // Item 3: Paragraph with special characters
    assert_ast(&doc).item(3, |item| {
        item.assert_paragraph()
            .text_contains("special characters")
            .text_contains("!@#$%^&*()_+-=[]{}|;':\",./<>?")
            .text_contains("{{paragraph}}")
            .line_count(1);
    });

    // Item 4: Paragraph with numbers
    assert_ast(&doc).item(4, |item| {
        item.assert_paragraph()
            .text_contains("numbers")
            .text_contains("123")
            .text_contains("456")
            .text_contains("789")
            .text_contains("{{paragraph}}")
            .line_count(1);
    });

    // Item 5: Paragraph with mixed content
    assert_ast(&doc).item(5, |item| {
        item.assert_paragraph()
            .text_contains("mixed content")
            .text_contains("quick brown fox")
            .text_contains("123 ABC def!")
            .text_contains("{{paragraph}}")
            .line_count(1);
    });
}

#[test]
fn test_trifecta_010_paragraphs_sessions_flat_single() {
    // Test paragraphs combined with a single session
    let source = Lexplore::trifecta(10).source();
    let doc = parse_document(&source).unwrap();

    // Should have 5 items: 1 opening para, 1 session, 1 para, 1 session, 1 para
    assert_ast(&doc).item_count(5);

    // Item 0: Description paragraph
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph()
            .text_contains("combination of paragraphs and a single session")
            .text_contains("{{paragraph}}")
            .line_count(1);
    });

    // Item 1: First session with 2 paragraphs
    assert_ast(&doc).item(1, |item| {
        item.assert_session()
            .label("1. Introduction {{session-title}}")
            .child_count(2)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("content of the session")
                    .text_contains("indented relative to the session title")
                    .text_contains("{{paragraph}}")
                    .line_count(1);
            })
            .child(1, |child| {
                child
                    .assert_paragraph()
                    .text_contains("multiple paragraphs")
                    .text_contains("properly indented")
                    .text_contains("{{paragraph}}")
                    .line_count(1);
            });
    });

    // Item 2: Root level paragraph
    assert_ast(&doc).item(2, |item| {
        item.assert_paragraph()
            .text_contains("comes after the session")
            .text_contains("root level")
            .text_contains("{{paragraph}}")
            .line_count(1);
    });

    // Item 3: Second session
    assert_ast(&doc).item(3, |item| {
        item.assert_session()
            .label("Another Session {{session-title}}")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("multiple sessions at the same level")
                    .text_contains("{{paragraph}}")
                    .line_count(1);
            });
    });

    // Item 4: Final root paragraph
    assert_ast(&doc).item(4, |item| {
        item.assert_paragraph()
            .text("Final paragraph at the root level. {{paragraph}}")
            .line_count(1);
    });
}

#[test]
fn test_trifecta_020_paragraphs_sessions_flat_multiple() {
    // Test multiple sessions at root level with paragraphs between them
    let source = Lexplore::trifecta(20).source();
    let doc = parse_document(&source).unwrap();

    // Should have 8 items: 1 opening para, 4 sessions, 3 interstitial paras
    assert_ast(&doc).item_count(8);

    // Item 0: Description
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph()
            .text_contains("multiple sessions at the root level")
            .text_contains("{{paragraph}}")
            .line_count(1);
    });

    // Item 1: First Session with 2 paragraphs
    assert_ast(&doc).item(1, |item| {
        item.assert_session()
            .label("1. First Session {{session-title}}")
            .child_count(2)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text("This is the content of the first session. {{paragraph}}")
                    .line_count(1);
            })
            .child(1, |child| {
                child
                    .assert_paragraph()
                    .text("It can have multiple paragraphs. {{paragraph}}")
                    .line_count(1);
            });
    });

    // Item 2: Second Session
    assert_ast(&doc).item(2, |item| {
        item.assert_session()
            .label("2. Second Session {{session-title}}")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text("The second session also has content. {{paragraph}}")
                    .line_count(1);
            });
    });

    // Item 3: Paragraph between sessions
    assert_ast(&doc).item(3, |item| {
        item.assert_paragraph()
            .text("A paragraph between sessions. {{paragraph}}")
            .line_count(1);
    });

    // Item 4: Third Session
    assert_ast(&doc).item(4, |item| {
        item.assert_session()
            .label("3. Third Session {{session-title}}")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("different amounts of content")
                    .text_contains("{{paragraph}}")
                    .line_count(1);
            });
    });

    // Item 5: Another paragraph
    assert_ast(&doc).item(5, |item| {
        item.assert_paragraph()
            .text("Another paragraph. {{paragraph}}")
            .line_count(1);
    });

    // Item 6: Session with nested session (note: this is actually parsed as nested)
    assert_ast(&doc).item(6, |item| {
        item.assert_session()
            .label("4. Session Without Numbering {{session-title}}")
            .child_count(1) // Contains one nested session
            .child(0, |child| {
                child
                    .assert_session()
                    .label("Session titles don't require numbering markers. {{session-title}}")
                    .child_count(1)
                    .child(0, |para| {
                        para.assert_paragraph()
                            .text_contains("They just need to be followed by a blank line")
                            .text_contains("{{paragraph}}")
                            .line_count(1);
                    });
            });
    });

    // Item 7: Final paragraph
    assert_ast(&doc).item(7, |item| {
        item.assert_paragraph()
            .text("Final paragraph at the root level. {{paragraph}}")
            .line_count(1);
    });
}

#[test]
fn test_trifecta_030_sessions_nested_multiple() {
    // Test sessions with nesting at various levels
    let source = Lexplore::trifecta(30).source();
    let doc = parse_document(&source).unwrap();

    // Should have 4 items: 1 opening para, 2 root sessions, 1 final para
    assert_ast(&doc).item_count(4);

    // Item 0: Description
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph()
            .text_contains("sessions with nesting at various levels")
            .text_contains("{{paragraph}}")
            .line_count(1);
    });

    // Item 1: First root session with complex nesting
    assert_ast(&doc).item(1, |item| {
        item.assert_session()
            .label("1. Root Session {{session-title}}")
            .child_count(4); // para, subsession 1.1, subsession 1.2, para
    });

    // Verify first paragraph in root session
    assert_ast(&doc).item(1, |item| {
        item.assert_session().child(0, |child| {
            child
                .assert_paragraph()
                .text_contains("first nesting level")
                .text_contains("{{paragraph}}")
                .line_count(1);
        });
    });

    // Verify first sub-session (1.1)
    assert_ast(&doc).item(1, |item| {
        item.assert_session().child(1, |child| {
            child
                .assert_session()
                .label("1.1. First Sub-session {{session-title}}")
                .child_count(2) // 2 paragraphs
                .child(0, |para| {
                    para.assert_paragraph()
                        .text_contains("second nesting level")
                        .text_contains("{{paragraph}}")
                        .line_count(1);
                })
                .child(1, |para| {
                    para.assert_paragraph()
                        .text("It can have multiple paragraphs. {{paragraph}}")
                        .line_count(1);
                });
        });
    });

    // Verify second sub-session (1.2) with deeper nesting
    assert_ast(&doc).item(1, |item| {
        item.assert_session().child(2, |child| {
            child
                .assert_session()
                .label("1.2. Second Sub-session {{session-title}}")
                .child_count(2) // para + deeply nested session
                .child(0, |para| {
                    para.assert_paragraph()
                        .text_contains("Another sub-session at the same level")
                        .text_contains("{{paragraph}}")
                        .line_count(1);
                })
                .child(1, |nested| {
                    nested
                        .assert_session()
                        .label("1.2.1. Deeply Nested Session {{session-title}}")
                        .child_count(2)
                        .child(0, |para| {
                            para.assert_paragraph()
                                .text_contains("third nesting level")
                                .text_contains("{{paragraph}}")
                                .line_count(1);
                        })
                        .child(1, |para| {
                            para.assert_paragraph()
                                .text_contains("nested arbitrarily deep")
                                .text_contains("{{paragraph}}")
                                .line_count(1);
                        });
                });
        });
    });

    // Verify paragraph back at first nesting level
    assert_ast(&doc).item(1, |item| {
        item.assert_session().child(3, |child| {
            child
                .assert_paragraph()
                .text("Back to the first nesting level. {{paragraph}}")
                .line_count(1);
        });
    });

    // Item 2: Second root session
    assert_ast(&doc).item(2, |item| {
        item.assert_session()
            .label("2. Another Root Session {{session-title}}")
            .child_count(2); // para + subsession
    });

    // Verify second root session content
    assert_ast(&doc).item(2, |item| {
        item.assert_session()
            .child(0, |para| {
                para.assert_paragraph()
                    .text_contains("root level alongside the first one")
                    .text_contains("{{paragraph}}")
                    .line_count(1);
            })
            .child(1, |subsession| {
                subsession
                    .assert_session()
                    .label("2.1. Its Sub-session {{session-title}}")
                    .child_count(1)
                    .child(0, |para| {
                        para.assert_paragraph()
                            .text_contains("different numbering schemes")
                            .text_contains("{{paragraph}}")
                            .line_count(1);
                    });
            });
    });

    // Item 3: Final root paragraph
    assert_ast(&doc).item(3, |item| {
        item.assert_paragraph()
            .text("Final paragraph at the root level. {{paragraph}}")
            .line_count(1);
    });
}

#[test]
fn test_trifecta_040_lists() {
    // Test various list formats and decorations
    let source = Lexplore::trifecta(40).source();
    let doc = parse_document(&source).unwrap();

    // Should have 15 items total (paragraphs + lists)
    assert_ast(&doc).item_count(15);

    // Item 0: Description
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph()
            .text_contains("various list formats and decorations")
            .text_contains("{{paragraph}}")
            .line_count(1);
    });

    // Item 1: "Plain dash lists:" paragraph
    assert_ast(&doc).item(1, |item| {
        item.assert_paragraph()
            .text("Plain dash lists: {{paragraph}}");
    });

    // Item 2: Plain dash list
    assert_ast(&doc).item(2, |item| {
        item.assert_list().item_count(3);
    });

    // Item 3: "Numerical lists:" paragraph
    assert_ast(&doc).item(3, |item| {
        item.assert_paragraph()
            .text("Numerical lists: {{paragraph}}");
    });

    // Item 4: Numerical list
    assert_ast(&doc).item(4, |item| {
        item.assert_list().item_count(3);
    });

    // Item 5: "Alphabetical lists:" paragraph
    assert_ast(&doc).item(5, |item| {
        item.assert_paragraph()
            .text("Alphabetical lists: {{paragraph}}");
    });

    // Item 6: Alphabetical list
    assert_ast(&doc).item(6, |item| {
        item.assert_list().item_count(3);
    });

    // Item 7: "Mixed decoration lists" paragraph
    assert_ast(&doc).item(7, |item| {
        item.assert_paragraph()
            .text_contains("Mixed decoration lists")
            .text_contains("{{paragraph}}");
    });

    // Item 8: Mixed decoration list
    assert_ast(&doc).item(8, |item| {
        item.assert_list().item_count(3);
    });

    // Item 9: "Parenthetical numbering:" paragraph
    assert_ast(&doc).item(9, |item| {
        item.assert_paragraph()
            .text("Parenthetical numbering: {{paragraph}}");
    });

    // Item 10: Parenthetical list
    assert_ast(&doc).item(10, |item| {
        item.assert_list().item_count(3);
    });

    // Item 11: "Roman numerals:" paragraph
    assert_ast(&doc).item(11, |item| {
        item.assert_paragraph()
            .text("Roman numerals: {{paragraph}}");
    });

    // Item 12: Roman numeral list
    assert_ast(&doc).item(12, |item| {
        item.assert_list().item_count(3);
    });

    // Item 13: "Lists with longer content:" paragraph
    assert_ast(&doc).item(13, |item| {
        item.assert_paragraph()
            .text("Lists with longer content: {{paragraph}}");
    });

    // Item 14: Longer content list
    assert_ast(&doc).item(14, |item| {
        item.assert_list().item_count(3);
    });
}

#[test]
fn test_trifecta_050_paragraph_lists() {
    // Test disambiguation between paragraphs and lists
    let source = Lexplore::trifecta(50).source();
    let doc = parse_document(&source).unwrap();

    // Based on treeviz output, should have 15 items
    assert_ast(&doc).item_count(15);

    // Item 0: Description
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph()
            .text_contains("disambiguation between paragraphs and lists")
            .text_contains("{{paragraph}}")
            .line_count(1);
    });

    // Item 1: Multi-line paragraph with single dash item (illegal list)
    assert_ast(&doc).item(1, |item| {
        item.assert_paragraph()
            .text_contains("Single item with dash")
            .text_contains("- This is not a list")
            .line_count(2);
    });

    // Item 2: Multi-line paragraph with single numbered item (illegal list)
    assert_ast(&doc).item(2, |item| {
        item.assert_paragraph()
            .text_contains("Single item with number")
            .text_contains("1. This is also not a list")
            .line_count(2);
    });

    // Item 3: Multi-line paragraph with two list items (becomes a paragraph because no blank line before)
    assert_ast(&doc).item(3, |item| {
        item.assert_paragraph()
            .text_contains("Lists require at least two items")
            .text_contains("- First item")
            .text_contains("- Second item")
            .line_count(3);
    });

    // Item 4: Header paragraph
    assert_ast(&doc).item(4, |item| {
        item.assert_paragraph()
            .text_contains("Paragraph followed by list WITH blank line")
            .line_count(1);
    });

    // Item 5: Actual list (has blank line before it)
    assert_ast(&doc).item(5, |item| {
        item.assert_list().item_count(2);
    });

    // Item 6: Header paragraph
    assert_ast(&doc).item(6, |item| {
        item.assert_paragraph()
            .text_contains("List followed by paragraph without blank line")
            .line_count(1);
    });

    // Item 7: List
    assert_ast(&doc).item(7, |item| {
        item.assert_list().item_count(2);
    });

    // Item 8: Paragraph after list
    assert_ast(&doc).item(8, |item| {
        item.assert_paragraph()
            .text_contains("This paragraph follows after blank line")
            .line_count(1);
    });

    // Item 9: Multi-line paragraph with dash item
    assert_ast(&doc).item(9, |item| {
        item.assert_paragraph()
            .text_contains("Blank lines between list items")
            .text_contains("- This is not")
            .line_count(2);
    });

    // Item 10: Single line paragraph with dash
    assert_ast(&doc).item(10, |item| {
        item.assert_paragraph()
            .text("- A list {{paragraph}}")
            .line_count(1);
    });

    // Item 11: Header paragraph
    assert_ast(&doc).item(11, |item| {
        item.assert_paragraph()
            .text_contains("Proper list with blank lines around it")
            .line_count(1);
    });

    // Item 12: Proper list
    assert_ast(&doc).item(12, |item| {
        item.assert_list().item_count(2);
    });

    // Item 13: Paragraph after list
    assert_ast(&doc).item(13, |item| {
        item.assert_paragraph()
            .text("Paragraph after proper list. {{paragraph}}")
            .line_count(1);
    });

    // Item 14: Multi-line paragraph containing what looks like list items
    assert_ast(&doc).item(14, |item| {
        item.assert_paragraph()
            .text_contains("Valid mixed decoration list")
            .text_contains("- First item")
            .text_contains("1. Second item")
            .text_contains("a. Third item")
            .line_count(4);
    });
}

#[test]
fn test_trifecta_flat_simple() {
    // Test flat structure with all three elements
    // Renamed from 050 to 070 to avoid duplicate numbers
    let source = Lexplore::from_path(workspace_path(
        "comms/specs/trifecta/070-trifecta-flat-simple.lex",
    ))
    .source();
    let doc = parse_document(&source).unwrap();

    // Item 0: Opening paragraph
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph()
            .text_contains("all three core elements");
    });

    // Item 1: Session with only paragraphs
    assert_ast(&doc).item(1, |item| {
        item.assert_session()
            .label_contains("Session with Paragraph Content")
            .child_count(2)
            .child(0, |child| {
                child
                    .assert_paragraph() // "Session with Paragraph Content"
                    .text_contains("starts with a paragraph");
            })
            .child(1, |child| {
                child
                    .assert_paragraph() // "multiple paragraphs"
                    .text_contains("multiple paragraphs");
            });
    });

    // Item 2: Session with only a list
    assert_ast(&doc).item(2, |item| {
        item.assert_session()
            .label_contains("Session with List Content")
            .child_count(1)
            .child(0, |child| {
                child.assert_list().item_count(3);
            });
    });

    // Item 3: Session with mixed content (para + list + para)
    assert_ast(&doc).item(3, |item| {
        item.assert_session()
            .label_contains("Session with Mixed Content")
            .child_count(3)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("starts with a paragraph");
            })
            .child(1, |child| {
                child.assert_list().item_count(2);
            })
            .child(2, |child| {
                child
                    .assert_paragraph()
                    .text_contains("ends with another paragraph");
            });
    });

    // Item 4: Root level paragraph
    assert_ast(&doc).item(4, |item| {
        item.assert_paragraph().text_contains("root level");
    });

    // Item 5: Root level list
    assert_ast(&doc).item(5, |item| {
        item.assert_list().item_count(2);
    });

    // Item 6: Session with list + para + list
    assert_ast(&doc).item(6, |item| {
        item.assert_session()
            .label_contains("Another Session")
            .child_count(3)
            .child(0, |child| {
                child.assert_list().item_count(2);
            })
            .child(1, |child| {
                child.assert_paragraph().text_contains("has a paragraph");
            })
            .child(2, |child| {
                child.assert_list().item_count(2);
            });
    });
}

#[test]
fn test_trifecta_nesting() {
    // Test nested structure with all three elements
    let source = Lexplore::trifecta(60).source();
    let doc = parse_document(&source).unwrap();

    // Item 0: Opening paragraph
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph() // "various levels of nesting"
            .text_contains("various levels of nesting");
    });

    // Item 1: Root session with nested sessions and mixed content
    // The structure has been updated to include nested lists, which may affect the child count
    assert_ast(&doc).item(1, |item| {
        item.assert_session()
            .label_contains("1. Root Session")
            .child_count(5); // para, subsession, subsession, para, list
    });

    // Verify first child of root session is paragraph
    assert_ast(&doc).item(1, |item| {
        item.assert_session().child(0, |child| {
            child.assert_paragraph().text_contains("nested elements");
        });
    });

    // Verify first nested session (1.1)
    assert_ast(&doc).item(1, |item| {
        item.assert_session().child(1, |child| {
            child
                .assert_session()
                .label_contains("1.1. Sub-session")
                .child_count(2) // para + list
                .child(0, |para| {
                    para.assert_paragraph();
                })
                .child(1, |list| {
                    list.assert_list().item_count(2);
                });
        });
    });

    // Verify deeply nested session (1.2 containing 1.2.1)
    assert_ast(&doc).item(1, |item| {
        item.assert_session().child(2, |child| {
            child
                .assert_session()
                .label_contains("1.2. Sub-session with List")
                .child_count(3) // list, para, nested session
                .child(2, |nested| {
                    nested
                        .assert_session()
                        .label_contains("1.2.1. Deeply Nested")
                        .child_count(3); // para + list + list
                });
        });
    });

    // Verify the deeply nested session has 2 lists
    assert_ast(&doc).item(1, |item| {
        item.assert_session().child(2, |subsession| {
            subsession.assert_session().child(2, |deeply_nested| {
                deeply_nested
                    .assert_session()
                    .child(1, |first_list| {
                        first_list.assert_list().item_count(2);
                    })
                    .child(2, |second_list| {
                        second_list.assert_list().item_count(2);
                    });
            });
        });
    });

    // Item 2: Another root session with different nesting
    assert_ast(&doc).item(2, |item| {
        item.assert_session()
            .label_contains("2. Another Root Session")
            .child_count(2); // para + subsession
    });

    // Verify even deeper nesting (2.1.1)
    assert_ast(&doc).item(2, |item| {
        item.assert_session().child(1, |subsession| {
            subsession
                .assert_session()
                .label_contains("2.1. Mixed Content")
                .child_count(4) // list, para, list, nested session
                .child(3, |deeply_nested| {
                    deeply_nested
                        .assert_session()
                        .label_contains("2.1.1. Even Deeper")
                        .child_count(4); // para, list, para, list
                });
        });
    });

    // Final root paragraph
    assert_ast(&doc).item(3, |item| {
        item.assert_paragraph()
            .text_contains("Final root level paragraph");
    });
}

// Nested list tests have been moved to elements/lists.rs
// Definition tests have been moved to elements/definitions.rs

#[test]
fn test_verified_ensemble_with_definitions() {
    // Comprehensive ensemble test with all core elements including definitions
    // Using definition-90-document-simple.lex which tests definitions in context
    let source = Lexplore::definition(90).source();
    let doc = parse_document(&source).unwrap();

    // Item 0: Opening paragraph
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph() // "all core elements"
            .text_contains("all core elements");
    });

    // Item 1: Introduction definition (with para + list)
    assert_ast(&doc).item(1, |item| {
        item.assert_definition()
            .subject("Introduction")
            .child_count(2)
            .child(0, |child| {
                child.assert_paragraph().text_contains("ensemble test");
            })
            .child(1, |child| {
                child.assert_list().item_count(4);
            });
    });

    // Item 2: Simple Elements Section session
    assert_ast(&doc).item(2, |item| {
        item.assert_session()
            .label("1. Simple Elements Section {{session}}")
            .child_count(5); // para + 2 definitions + para + list
    });

    // Item 3: Nested Elements Section session
    assert_ast(&doc).item(3, |item| {
        item.assert_session()
            .label("2. Nested Elements Section {{session}}")
            .child_count(3); // para + 2 subsections (2.1 and 2.2)
    });
}

// Annotation and verbatim block tests have been moved to their respective element modules:
// - elements/annotations.rs for annotation tests
// - elements/verbatim.rs for verbatim block tests

// ==================== BENCHMARK TESTS ====================
// Comprehensive kitchensink test that includes all element types

#[test]
fn test_benchmark_010_kitchensink() {
    // Comprehensive test including all major features:
    // - Paragraphs (single/multi-line)
    // - Definitions (root and nested)
    // - Sessions (flat and nested, up to 2 levels)
    // - Lists (simple and nested, up to 3 levels)
    // - Annotations (marker and block)
    // - Verbatim blocks (subject and marker style)

    let source = Lexplore::benchmark(10).source();
    let doc = parse_document(&source).unwrap();

    // Document has 7 root items
    assert_ast(&doc).item_count(7);

    // Item 0: Description paragraph
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph()
            .text_contains("*all major features*")
            .text_contains("{{paragraph}}")
            .line_count(1);
    });

    // Item 1: Multi-line paragraph
    assert_ast(&doc).item(1, |item| {
        item.assert_paragraph()
            .text_contains("two-lined paragraph")
            .text_contains("_definition_ at the root level")
            .line_count(2);
    });

    // Item 2: Root definition with mixed content (paragraph + list)
    assert_ast(&doc).item(2, |item| {
        item.assert_definition()
            .subject("Root Definition")
            .child_count(2)
            .child(0, |para| {
                para.assert_paragraph()
                    .text_contains("contains a paragraph and a `list`")
                    .text_contains("{{definition}}")
                    .line_count(1);
            })
            .child(1, |list| {
                list.assert_list().item_count(2);
            });
    });

    // Item 3: Paragraph between root elements
    assert_ast(&doc).item(3, |item| {
        item.assert_paragraph()
            .text_contains("marker annotation at the root level")
            .line_count(1);
    });

    // Item 4: Primary Session (Level 1) with complex nested content
    assert_ast(&doc).item(4, |item| {
        item.assert_session()
            .label("1. Primary Session {{session}}")
            .child_count(5); // para, list, nested session, para, verbatim
    });

    // Verify first paragraph in primary session
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(0, |para| {
            para.assert_paragraph()
                .text_contains("main container for testing nested structures")
                .text_contains("{{paragraph}}")
                .line_count(1);
        });
    });

    // Verify list in primary session
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(1, |list| {
            list.assert_list().item_count(2);
        });
    });

    // Verify marker annotation attaches to the session list
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(1, |list| {
            list.assert_list()
                .annotation_count(1)
                .annotation(0, |annotation| {
                    annotation.label("warning").child_count(1).child(0, |para| {
                        para.assert_paragraph()
                            .text_contains("single-line annotation inside the session");
                    });
                });
        });
    });

    // Verify nested session (Level 2)
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(2, |nested_session| {
            nested_session
                .assert_session()
                .label("1.1. Nested Session (Level 2) {{session}}")
                .child_count(3); // para, definition, list with nested content
        });
    });

    // Verify paragraph in nested session
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(2, |nested_session| {
            nested_session.assert_session().child(0, |para| {
                para.assert_paragraph()
                    .text_contains("second-level session")
                    .text_contains("{{paragraph}}")
                    .line_count(1);
            });
        });
    });

    // Verify nested definition inside nested session
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(2, |nested_session| {
            nested_session.assert_session().child(1, |def| {
                def.assert_definition()
                    .subject("Nested Definition")
                    .child_count(2) // para + list
                    .child(0, |para| {
                        para.assert_paragraph()
                            .text_contains("inside a nested session")
                            .text_contains("{{definition}}")
                            .line_count(1);
                    })
                    .child(1, |list| {
                        list.assert_list().item_count(2);
                    });
            });
        });
    });

    // Verify nested list (Level 2) with deeply nested content (Level 3)
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(2, |nested_session| {
            nested_session.assert_session().child(2, |list| {
                list.assert_list().item_count(2).item(0, |item| {
                    // First list item has nested paragraph + nested list (Level 3)
                    item.child_count(2)
                        .child(0, |para| {
                            para.assert_paragraph()
                                .text_contains("list item contains a nested paragraph")
                                .text_contains("{{paragraph}}")
                                .line_count(1);
                        })
                        .child(1, |nested_list| {
                            nested_list.assert_list().item_count(2);
                        });
                });
            });
        });
    });

    // Verify paragraph back at first level
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(3, |para| {
            para.assert_paragraph()
                .text_contains("paragraph back at the first level")
                .text_contains("{{paragraph}}")
                .line_count(1);
        });
    });

    // Verify verbatim block with subject line
    assert_ast(&doc).item(4, |item| {
        item.assert_session().child(4, |verbatim| {
            verbatim
                .assert_verbatim_block()
                .subject("Code Example (Verbatim Block)")
                .line_count(4)
                .content_contains("return \"lex\";");
        });
    });

    // Item 5: Second Root Session with annotations and verbatim
    assert_ast(&doc).item(5, |item| {
        item.assert_session()
            .label("2. Second Root Session {{session}}")
            .child_count(2); // para, marker verbatim
    });

    // Verify paragraph in second session
    assert_ast(&doc).item(5, |item| {
        item.assert_session().child(0, |para| {
            para.assert_paragraph()
                .text_contains("annotations with block content")
                .text_contains("{{paragraph}}")
                .line_count(1);
        });
    });

    // Block annotation now attaches to the following verbatim block
    assert_ast(&doc).item(5, |item| {
        item.assert_session().child(1, |verbatim| {
            verbatim
                .assert_verbatim_block()
                .annotation_count(1)
                .annotation(0, |annotation| {
                    annotation
                        .label("todo")
                        .child_count(3)
                        .child(0, |para| {
                            para.assert_paragraph()
                                .text_contains("block annotation")
                                .text_contains("{{paragraph}}")
                                .line_count(1);
                        })
                        .child(1, |para| {
                            para.assert_paragraph()
                                .text_contains("contains a paragraph and a list")
                                .text_contains("{{paragraph}}")
                                .line_count(1);
                        })
                        .child(2, |list| {
                            list.assert_list().item_count(2);
                        });
                });
        });
    });

    // Verify marker-style verbatim block
    assert_ast(&doc).item(5, |item| {
        item.assert_session().child(1, |verbatim| {
            verbatim
                .assert_verbatim_block()
                .subject("Image Reference (Marker Verbatim Block)")
                .line_count(0); // Marker style has no content lines
        });
    });

    // Inline parsing checks: description paragraph and definition list items
    let description = doc
        .root
        .children
        .iter()
        .find_map(|item| match item {
            ContentItem::Paragraph(para) if para.text().contains("*all major features*") => {
                Some(para)
            }
            _ => None,
        })
        .expect("expected description paragraph");
    let description_line = match description.lines.first() {
        Some(ContentItem::TextLine(line)) => line,
        _ => panic!("expected text line in description paragraph"),
    };
    InlineAssertion::new(&description_line.content, "kitchensink:description[0]").starts_with(&[
        InlineExpectation::plain_text("This document includes "),
        InlineExpectation::strong_text("all major features"),
        InlineExpectation::plain(TextMatch::StartsWith(
            " of the lex language to serve as a comprehensive \"kitchensink\"".into(),
        )),
        InlineExpectation::reference(ReferenceExpectation::citation_with_locator(
            vec![TextMatch::Exact("spec2025".into())],
            Some(TextMatch::Exact("pp. 45-46".into())),
        )),
        InlineExpectation::plain_text(". {{paragraph}}"),
    ]);

    let root_definition = doc
        .root
        .children
        .iter()
        .find_map(|item| match item {
            ContentItem::Definition(def) if def.subject.as_string().contains("Root Definition") => {
                Some(def)
            }
            _ => None,
        })
        .expect("expected root definition");
    let list = root_definition
        .children
        .iter()
        .find_map(|child| match child {
            ContentItem::List(list) => Some(list),
            _ => None,
        })
        .expect("definition should contain a list");
    let mut list_items = list.items.iter().filter_map(|item| match item {
        ContentItem::ListItem(li) => Some(li),
        _ => None,
    });

    let first_item = list_items.next().expect("missing first list item");
    let first_text = first_item
        .text
        .first()
        .expect("list item missing inline text content");
    InlineAssertion::new(first_text, "kitchensink:definition:list[0]").starts_with(&[
        InlineExpectation::plain_text("Item 1 in definition referencing "),
        InlineExpectation::reference(ReferenceExpectation::tk(Some(TextMatch::Exact(
            "rootlist".into(),
        )))),
        InlineExpectation::plain(TextMatch::StartsWith(". {{list-item}}".into())),
    ]);

    let second_item = list_items.next().expect("missing second list item");
    let second_text = second_item
        .text
        .first()
        .expect("list item missing inline text content");
    InlineAssertion::new(second_text, "kitchensink:definition:list[1]").starts_with(&[
        InlineExpectation::plain_text("Item 2 in definition with note "),
        InlineExpectation::reference(ReferenceExpectation::footnote_number(42)),
        InlineExpectation::plain(TextMatch::StartsWith(". {{list-item}}".into())),
    ]);

    // Item 6: Final root paragraph
    assert_ast(&doc).item(6, |item| {
        item.assert_paragraph()
            .text("Final paragraph at the end of the document. {{paragraph}}")
            .line_count(1);
    });
}
