//! Unit tests for isolated session elements
//!
//! Tests session parsing in isolation following the on-lexplore.lex guidelines:
//! - Use Lexplore to load centralized test files
//! - Use assert_ast for deep structure verification
//! - Test isolated elements (one element per test)
//! - Verify content and structure, not just counts

use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;

#[test]
fn test_session_01_flat_simple() {
    // session-01-flat-simple.lex: Session with title "Introduction" and one paragraph
    let doc = Lexplore::session(1).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("Introduction")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("simple session with a title");
            });
    });
}

#[test]
fn test_session_02_flat_numbered_title() {
    // session-02-flat-numbered-title.lex: Session with numbered title "1. Introduction:"
    let doc = Lexplore::session(2).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("1. Introduction:")
            .child_count(1)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("numbered title marker");
            });
    });
}

#[test]
fn test_session_05_nested_simple() {
    // session-05-nested-simple.lex: Document with paragraphs and nested sessions
    let doc = Lexplore::session(5).parse().unwrap();

    // Document structure: Para, Session, Para, Session, Para
    assert_ast(&doc)
        .item_count(5)
        .item(0, |item| {
            item.assert_paragraph()
                .text_contains("combination of paragraphs");
        })
        .item(1, |item| {
            item.assert_session()
                .label("1. Introduction {{session-title}}")
                .child_count(2)
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("content of the session");
                })
                .child(1, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("multiple paragraphs");
                });
        })
        .item(2, |item| {
            item.assert_paragraph()
                .text_contains("paragraph comes after the session");
        })
        .item(3, |item| {
            item.assert_session()
                .label("Another Session {{session-title}}")
                .child_count(1)
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("multiple sessions at the same level");
                });
        })
        .item(4, |item| {
            item.assert_paragraph()
                .text_contains("Final paragraph at the root level");
        });
}

#[test]
fn test_session_03_flat_multiple_paragraphs() {
    let doc = Lexplore::session(3).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("Background:")
            .child_count(3)
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("contains multiple paragraphs");
            })
            .child(1, |child| {
                child
                    .assert_paragraph()
                    .text_contains("Each paragraph is indented");
            })
            .child(2, |child| {
                child.assert_paragraph().text_contains("third paragraph");
            });
    });
}

#[test]
fn test_session_04_flat_alphanumeric_title() {
    let doc = Lexplore::session(4).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("A. First Section:")
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("alphabetical markers in their titles");
            });
    });
}

#[test]
fn test_session_07_paragraphs_sessions_flat_multiple() {
    let doc = Lexplore::session(7).parse().unwrap();

    assert_ast(&doc)
        .item_count(8)
        .item(1, |item| {
            item.assert_session()
                .label("1. First Session {{session-title}}")
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("content of the first session");
                });
        })
        .item(4, |item| {
            item.assert_session()
                .label("3. Third Session {{session-title}}")
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("different amounts of content");
                });
        })
        .item(6, |item| {
            item.assert_session()
                .label("4. Session Without Numbering {{session-title}}")
                .child(0, |child| {
                    child
                        .assert_session()
                        .label("Session titles don't require numbering markers. {{session-title}}")
                        .child(0, |grandchild| {
                            grandchild
                                .assert_paragraph()
                                .text_contains("followed by a blank line");
                        });
                });
        });
}

#[test]
fn test_session_08_paragraphs_sessions_nested_multiple() {
    let doc = Lexplore::session(8).parse().unwrap();

    assert_ast(&doc)
        .item_count(4)
        .item(1, |item| {
            item.assert_session()
                .label("1. Root Session {{session-title}}")
                .child(1, |child| {
                    child
                        .assert_session()
                        .label("1.1. First Sub-session {{session-title}}")
                        .child(0, |grandchild| {
                            grandchild
                                .assert_paragraph()
                                .text_contains("second nesting level");
                        });
                });
        })
        .item(2, |item| {
            item.assert_session()
                .label("2. Another Root Session {{session-title}}")
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("root level alongside the first");
                });
        });
}

#[test]
fn test_session_09_flat_colon_title() {
    // session-09-flat-colon-title.lex: Session title ending with colon (bug #212)
    // Tests that sessions can have colons in their titles, distinguished from definitions
    // by blank lines between title and content
    let doc = Lexplore::session(9).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("Subject Title:")
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("session whose title ends with a colon");
            });
    });
}

#[test]
fn test_session_10_sandwich_document() {
    // Validates the session-list-session "sandwich" document exercises:
    // - Consecutive sessions without blank separators
    // - Lists at the document root between sessions
    // - Sessions nesting lists as their content
    let doc = Lexplore::session(10).parse().unwrap();

    assert_ast(&doc)
        // Blank line groups are filtered out by assert_ast, so only the structural
        // elements remain: session, session, list, session.
        .item_count(4)
        .item(0, |item| {
            item.assert_session()
                .label("1. Session Title")
                .child(0, |child| {
                    child
                        .assert_session()
                        .label("1.1. Session Title")
                        .child(0, |grandchild| {
                            grandchild
                                .assert_paragraph()
                                .text_contains("1.1.1 Session Title");
                        });
                });
        })
        .item(1, |item| {
            item.assert_session()
                .label("2. Session Title")
                .child(0, |child| {
                    child.assert_paragraph().text_contains("2.1 Session Title");
                });
        })
        .item(2, |item| {
            item.assert_list()
                .item_count(2)
                .item(0, |list_item| {
                    list_item.text_contains("And this is a list");
                })
                .item(1, |list_item| {
                    list_item.text_contains("scond list item");
                });
        })
        .item(3, |item| {
            item.assert_session()
                .label("3. Session title")
                .child(0, |child| {
                    child
                        .assert_list()
                        .item_count(2)
                        .item(0, |list_item| {
                            list_item.text_contains("list item, first");
                        })
                        .item(1, |list_item| {
                            list_item.text_contains("second list item");
                        });
                });
        });
}
