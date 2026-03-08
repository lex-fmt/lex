//! Unit tests for isolated list elements
//!
//! Tests list parsing in isolation following the on-lexplore.lex guidelines:
//! - Use Lexplore to load centralized test files
//! - Use assert_ast for deep structure verification
//! - Test isolated elements (one element per test)
//! - Verify content and structure, not just counts

use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;
use lex_core::lex::testing::workspace_path;

#[test]
fn test_list_01_flat_simple_dash() {
    // list-01-flat-simple-dash.lex: Paragraph "Test:" followed by dash list
    let doc = Lexplore::list(1).parse().unwrap();

    // Document has: Paragraph + List
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(3)
            .item(0, |list_item| {
                list_item.text_contains("First item");
            })
            .item(1, |list_item| {
                list_item.text_contains("Second item");
            })
            .item(2, |list_item| {
                list_item.text_contains("Third item");
            });
    });
}

#[test]
fn test_list_02_flat_numbered() {
    // list-02-flat-numbered.lex: Paragraph + numbered list
    let doc = Lexplore::list(2).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(3)
            .item(0, |list_item| {
                list_item.text_contains("First numbered item");
            })
            .item(1, |list_item| {
                list_item.text_contains("Second numbered item");
            })
            .item(2, |list_item| {
                list_item.text_contains("Third numbered item");
            });
    });
}

#[test]
fn test_list_07_nested_simple() {
    // list-07-nested-simple.lex: Paragraph + two-level nested list
    // Tests nesting structure: outer list items with nested lists
    let doc = Lexplore::list(7).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(2) // Two outer items
            .item(0, |list_item| {
                // First outer item: has nested list with 2 items
                list_item
                    .text_contains("First outer item")
                    .child_count(1)
                    .child(0, |nested| {
                        nested
                            .assert_list()
                            .item_count(2)
                            .item(0, |inner| {
                                inner.text_contains("First nested item");
                            })
                            .item(1, |inner| {
                                inner.text_contains("Second nested item");
                            });
                    });
            })
            .item(1, |list_item| {
                // Second outer item: blank line causes the nested list
                // to be parsed as a paragraph containing the list marker text
                list_item
                    .text_contains("Second outer item")
                    .child_count(1)
                    .child(0, |para| {
                        // The nested list is parsed as a paragraph due to blank line
                        para.assert_paragraph()
                            .text_contains("- Another nested item");
                    });
            });
    });
}

#[test]
fn test_list_03_flat_alphabetical() {
    let doc = Lexplore::list(3).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(3)
            .item(0, |list_item| {
                list_item.text_contains("First letter item");
            })
            .item(1, |list_item| {
                list_item.text_contains("Second letter item");
            })
            .item(2, |list_item| {
                list_item.text_contains("Third letter item");
            });
    });
}

#[test]
fn test_list_04_flat_mixed_markers() {
    let doc = Lexplore::list(4).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(3)
            .item(0, |list_item| {
                // First item establishes the decoration style for the whole list.
                list_item
                    .marker("1.")
                    .text_starts_with("First item")
                    .child_count(0);
            })
            .item(1, |list_item| {
                list_item
                    .marker("-")
                    .text_starts_with("Second item")
                    .child_count(0);
            })
            .item(2, |list_item| {
                list_item
                    .marker("a.")
                    .text_starts_with("Third item")
                    .child_count(0);
            });
    });
}

#[test]
fn test_list_05_flat_parenthetical() {
    let doc = Lexplore::list(5).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(3)
            .item(0, |list_item| {
                list_item.text_contains("First parenthetical item");
            })
            .item(1, |list_item| {
                list_item.text_contains("Second parenthetical item");
            })
            .item(2, |list_item| {
                list_item.text_contains("Third parenthetical item");
            });
    });
}

#[test]
fn test_list_06_flat_roman_numerals() {
    let doc = Lexplore::list(6).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(3)
            .item(0, |list_item| {
                list_item.text_contains("First roman item");
            })
            .item(1, |list_item| {
                list_item.text_contains("Second roman item");
            })
            .item(2, |list_item| {
                list_item.text_contains("Third roman item");
            });
    });
}

#[test]
fn test_list_08_nested_with_paragraph() {
    let doc = Lexplore::list(8).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(2)
            .item(0, |list_item| {
                list_item
                    .text_contains("First item with nested content")
                    .child_count(3)
                    .child(0, |child| {
                        child
                            .assert_paragraph()
                            .text_contains("paragraph nested inside the list item");
                    })
                    .child(1, |child| {
                        child
                            .assert_list()
                            .item_count(2)
                            .item(0, |nested| {
                                nested.text_contains("Nested list item one");
                            })
                            .item(1, |nested| {
                                nested.text_contains("Nested list item two");
                            });
                    })
                    .child(2, |child| {
                        child
                            .assert_paragraph()
                            .text_contains("Another paragraph after the nested list");
                    });
            })
            .item(1, |list_item| {
                list_item
                    .text_contains("Second item")
                    .child_count(1)
                    .child(0, |child| {
                        child.assert_paragraph().text_contains("Final paragraph.");
                    });
            });
    });
}

#[test]
fn test_list_09_nested_three_levels() {
    let doc = Lexplore::list(9).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(2)
            .item(0, |outer| {
                outer
                    .text_contains("Outer level one")
                    .child_count(1)
                    .child(0, |child| {
                        child
                            .assert_list()
                            .item_count(2)
                            .item(0, |middle| {
                                middle
                                    .text_contains("Middle level one")
                                    .child_count(1)
                                    .child(0, |inner_list| {
                                        inner_list
                                            .assert_list()
                                            .item_count(2)
                                            .item(0, |inner| {
                                                inner.text_contains("Inner level one");
                                            })
                                            .item(1, |inner| {
                                                inner.text_contains("Inner level two");
                                            });
                                    });
                            })
                            .item(1, |middle| {
                                middle.text_contains("Middle level two");
                            });
                    });
            })
            .item(1, |outer| {
                outer.text_contains("Outer level two");
            });
    });
}

#[test]
fn test_list_10_nested_deep_only() {
    // Deep nesting with 2 items at each level (tests nested list parsing)
    let doc = Lexplore::list(10).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(2)
            .item(0, |level_one| {
                level_one.text_contains("Level one A");
            })
            .item(1, |level_one| {
                level_one
                    .text_contains("Level one B")
                    .child_count(1)
                    .child(0, |child| {
                        child
                            .assert_list()
                            .item_count(2)
                            .item(0, |level_two| {
                                level_two.text_contains("Level two A");
                            })
                            .item(1, |level_two| {
                                level_two.text_contains("Level two B").child_count(1).child(
                                    0,
                                    |grandchild| {
                                        grandchild
                                            .assert_list()
                                            .item_count(2)
                                            .item(0, |level_three| {
                                                level_three.text_contains("Level three A");
                                            })
                                            .item(1, |level_three| {
                                                level_three
                                                    .text_contains("Level three B")
                                                    .child_count(1)
                                                    .child(0, |deep| {
                                                        deep.assert_list()
                                                            .item_count(2)
                                                            .item(0, |level_four| {
                                                                level_four
                                                                    .text_contains("Level four A");
                                                            })
                                                            .item(1, |level_four| {
                                                                level_four
                                                                    .text_contains("Level four B");
                                                            });
                                                    });
                                            });
                                    },
                                );
                            });
                    });
            });
    });
}

#[test]
fn test_list_11_nested_balanced() {
    // Balanced nesting with 2 items at each level
    let doc = Lexplore::list(11).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(2)
            .item(0, |outer| {
                outer
                    .text_contains("item 1")
                    .child_count(1)
                    .child(0, |child| {
                        child
                            .assert_list()
                            .item_count(2)
                            .item(0, |nested| {
                                nested.text_contains("item 1.1");
                            })
                            .item(1, |nested| {
                                nested.text_contains("item 1.2");
                            });
                    });
            })
            .item(1, |outer| {
                outer
                    .text_contains("item 2")
                    .child_count(1)
                    .child(0, |child| {
                        child
                            .assert_list()
                            .item_count(2)
                            .item(0, |nested| {
                                nested.text_contains("item 2.1");
                            })
                            .item(1, |nested| {
                                nested.text_contains("item 2.2");
                            });
                    });
            });
    });
}

#[test]
fn test_list_12_nested_three_full_form() {
    let doc = Lexplore::list(12).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_list()
            .item_count(2)
            .item(0, |outer| {
                outer
                    .text_contains("Outer level one")
                    .child_count(1)
                    .child(0, |child| {
                        child
                            .assert_list()
                            .item_count(2)
                            .item(0, |middle| {
                                middle
                                    .text_contains("Middle level one")
                                    .child_count(1)
                                    .child(0, |inner_list| {
                                        inner_list
                                            .assert_list()
                                            .item_count(2)
                                            .item(0, |inner| {
                                                inner.text_contains("Inner level one");
                                            })
                                            .item(1, |inner| {
                                                inner.text_contains("Inner level two");
                                            });
                                    });
                            })
                            .item(1, |middle| {
                                middle.text_contains("Middle level two");
                            });
                    });
            })
            .item(1, |outer| {
                outer.text_contains("Outer level two");
            });
    });
}

#[test]
fn test_list_13_single_item_is_paragraph() {
    // Single dash-prefixed line is a paragraph, not a list (enforces 2+ item rule)
    let doc = Lexplore::list(13).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_paragraph()
            .text_contains("This looks like a list item but is actually a paragraph");
    });
}

#[test]
fn test_lists_overview_document() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/list.lex"))
        .parse()
        .unwrap();

    assert_ast(&doc)
        .item_count(9)
        .item(0, |item| {
            item.assert_session()
                .label("Introduction")
                .child_count(1)
                .child(0, |child| {
                    child
                        .assert_paragraph()
                        .text_contains("organize related items in sequence");
                });
        })
        .item(1, |item| {
            item.assert_session().label("Syntax");
        });
}
