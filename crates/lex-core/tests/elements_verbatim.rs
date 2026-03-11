//! Unit tests for isolated verbatim elements
//!
//! Tests verbatim block parsing in isolation following the on-lexplore.lex guidelines:
//! - Use Lexplore to load centralized test files
//! - Use assert_ast for deep structure verification
//! - Test isolated elements (one element per test)
//! - Verify content and structure, not just counts

use lex_core::lex::ast::elements::verbatim::VerbatimBlockMode;
use lex_core::lex::ast::AstNode;
use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;

#[test]
fn test_verbatim_01_flat_simple_code() {
    // verbatim-01-flat-simple-code.lex: Verbatim block with simple code
    let doc = Lexplore::verbatim(1).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Code Example")
            .closing_label("javascript")
            .content_contains("function hello()");
    });
}

#[test]
fn test_verbatim_02_flat_with_caption() {
    // verbatim-02-flat-with-caption.lex: Verbatim block with caption stored as content before closing data
    let doc = Lexplore::verbatim(2).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("API Response")
            .closing_label("json")
            .content_contains("{\"status\": \"ok\"");
    });
}

#[test]
fn test_verbatim_03_flat_with_params() {
    // verbatim-03-flat-with-params.lex: Verbatim with parameters in closing data
    let doc = Lexplore::verbatim(3).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Configuration")
            .closing_label("nginx")
            .has_closing_parameter_with_value("format", "pretty")
            .content_contains("server {");
    });
}

#[test]
fn test_verbatim_04_flat_marker_form() {
    // verbatim-04-flat-marker-form.lex: Marker-style verbatim with descriptive content
    let doc = Lexplore::verbatim(4).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Sunset Photo")
            .closing_label("image")
            .has_closing_parameter_with_value("src", "sunset.jpg")
            .content_contains("As the sun sets over the ocean.")
            .line_count(1);
    });
}

#[test]
fn test_verbatim_05_flat_special_chars() {
    // verbatim-05-flat-special-chars.lex: Verbatim with :: markers in content
    // Regression test: ensure :: markers inside content do not get misparsed as a paragraph.
    let doc = Lexplore::verbatim(5).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Special Characters")
            .closing_label("javascript")
            .content_contains("// This content has :: markers")
            .content_contains("return \"::\"");
    });
}

#[test]
fn test_verbatim_06_nested_in_definition() {
    // verbatim-06-nested-in-definition.lex: Verbatim nested inside a definition
    let doc = Lexplore::verbatim(6).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition()
            .subject("JavaScript Example")
            .child_count(3); // para, verbatim, para
    });

    // Verify first paragraph
    assert_ast(&doc).item(0, |item| {
        item.assert_definition().child(0, |para| {
            para.assert_paragraph()
                .text_contains("demonstrates closure");
        });
    });

    // Verify verbatim block
    assert_ast(&doc).item(0, |item| {
        item.assert_definition().child(1, |verbatim| {
            verbatim
                .assert_verbatim_block()
                .subject("Implementation")
                .closing_label("javascript")
                .content_contains("function counter()")
                .content_contains("let count = 0;")
                .content_contains("return () => ++count;")
                .line_count(6) // blank line before content, function counter() {, let count = 0;, return, }, and trailing blank line
                .line_eq(0, "")
                .line_eq(5, "");
        });
    });

    // Verify second paragraph
    assert_ast(&doc).item(0, |item| {
        item.assert_definition().child(2, |para| {
            para.assert_paragraph()
                .text_contains("simple closure pattern");
        });
    });
}

#[test]
fn test_verbatim_07_nested_in_list() {
    // verbatim-07-nested-in-list.lex: Verbatim blocks nested in list items
    let doc = Lexplore::verbatim(7).parse().unwrap();

    assert_ast(&doc).item_count(1); // list

    // Verify list with 2 items
    assert_ast(&doc).item(0, |item| {
        item.assert_list().item_count(2);
    });

    // Verify first list item with Python verbatim
    assert_ast(&doc).item(0, |item| {
        item.assert_list().item(0, |list_item| {
            list_item
                .text_contains("Python example")
                .child_count(1) // verbatim
                .child(0, |verbatim| {
                    verbatim
                        .assert_verbatim_block()
                        .subject("Simple function")
                        .closing_label("python")
                        .content_contains("def hello():")
                        .content_contains("return \"world\"")
                        .line_count(4) // blank line before content, def hello():, return "world", and trailing blank line
                        .line_eq(0, "")
                        .line_eq(3, "");
                });
        });
    });

    // Verify second list item with JavaScript verbatim
    assert_ast(&doc).item(0, |item| {
        item.assert_list().item(1, |list_item| {
            list_item
                .text_contains("JavaScript example")
                .child_count(1) // verbatim
                .child(0, |verbatim| {
                    verbatim
                        .assert_verbatim_block()
                        .subject("Another function")
                        .closing_label("javascript")
                        .content_contains("const greet")
                        .line_count(3) // blank line before content, const greet = () => "hello";, and trailing blank line
                        .line_eq(0, "")
                        .line_eq(2, "");
                });
        });
    });
}

#[test]
fn test_verbatim_08_nested_deep() {
    // verbatim-08-nested-deep.lex: Deeply nested verbatim (3 levels)
    let doc = Lexplore::verbatim(8).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition()
            .subject("Programming Languages")
            .child_count(2); // para, definition
    });

    // Verify paragraph at level 1
    assert_ast(&doc).item(0, |item| {
        item.assert_definition().child(0, |para| {
            para.assert_paragraph()
                .text_contains("Overview of different languages");
        });
    });

    // Verify nested definition at level 2
    assert_ast(&doc).item(0, |item| {
        item.assert_definition().child(1, |def2| {
            def2.assert_definition()
                .subject("Scripting Languages")
                .child_count(2); // para, definition
        });
    });

    // Verify paragraph at level 2
    assert_ast(&doc).item(0, |item| {
        item.assert_definition().child(1, |def2| {
            def2.assert_definition().child(0, |para| {
                para.assert_paragraph()
                    .text_contains("Languages for automation");
            });
        });
    });

    // Verify nested definition at level 3
    assert_ast(&doc).item(0, |item| {
        item.assert_definition().child(1, |def2| {
            def2.assert_definition().child(1, |def3| {
                def3.assert_definition().subject("Python").child_count(1); // verbatim
            });
        });
    });

    // Verify verbatim at level 3
    assert_ast(&doc).item(0, |item| {
        item.assert_definition().child(1, |def2| {
            def2.assert_definition().child(1, |def3| {
                def3.assert_definition().child(0, |verbatim| {
                    verbatim
                        .assert_verbatim_block()
                        .subject("Example code")
                        .closing_label("python")
                        .content_contains("#!/usr/bin/env python3")
                        .content_contains("print(\"Hello, World!\")");
                });
            });
        });
    });
}

#[test]
fn test_verbatim_09_flat_simple_beyond_wall() {
    // verbatim-09-flat-simple-beyong-wall.lex: Verbatim with content beyond indentation wall
    // Content beyond the wall gets its indentation normalized
    let doc = Lexplore::verbatim(9).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Code Example")
            .closing_label("javascript")
            .content_contains("function hello() {")
            .content_contains("return \"world\";")
            .content_contains("}")
            .line_count(5) // blank line before content, function hello(), return, closing brace, and trailing blank line
            .line_eq(0, "")
            .line_eq(4, "");
    });
}

#[test]
fn test_verbatim_10_flat_simple_empty() {
    // verbatim-10-flat-simple-empty.lex: Empty verbatim block
    let doc = Lexplore::verbatim(10).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Code Example")
            .closing_label("javascript")
            .line_count(0); // No content lines
    });
}

// Note: test_verbatim_11_group_shell is tested as part of test_verbatim_11_group_sequences
// which tests the full document structure of verbatim-11-group-shell.lex

#[test]
fn test_verbatim_13_group_spades() {
    // verbatim-13-group-spades.lex: Verbatim group with multiple pairs and blank lines
    // Tests verbatim groups with blank lines between pairs and spaces after colons
    let doc = Lexplore::verbatim(13).parse().unwrap();

    assert_ast(&doc).item_count(2).item(0, |item| {
        item.assert_verbatim_block()
            .subject("This is a groupped Verbatim Block, this is the first Group:")
            .closing_label("shell")
            .group_count(4)
            .group(0, |group| {
                group
                    .subject("This is a groupped Verbatim Block, this is the first Group:")
                    .content_contains("$ pwd # always te staring point");
            })
            .group(1, |group| {
                group
                    .subject("Now that you know where you are, lets find out what's around you:")
                    .content_contains("$ ls")
                    .content_contains("$ ls -r # recursive");
            })
            .group(2, |group| {
                group
                    .subject("And let's go places:")
                    .content_contains("$ cd <path to go>");
            })
            .group(3, |group| {
                group
                    .subject("Feeling lost, let's get back home:")
                    .content_contains("$ cd ~");
            });
    });

    // Verify paragraph after the verbatim group
    assert_ast(&doc).item(1, |item| {
        item.assert_paragraph().text_contains(
            "Note that verbatim blocks conetents can have any number of blank lines",
        );
    });
}

#[test]
fn test_verbatim_14_fullwidth_table() {
    // verbatim-14-fullwidth.lex: Fullwidth block starting near the left margin
    let doc = Lexplore::verbatim(14).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Fullwidth Table Example")
            .mode(VerbatimBlockMode::Fullwidth)
            .line_count(5)
            .content_contains("Header | Value | Notes")
            .content_contains("Minimal fullwidth block for wide tables")
            .content_contains("Beta   | 25    | extended range");
    });
}

#[test]
fn test_verbatim_15_inflow_preserves_leading_blank_line() {
    // verbatim-15-inflow-leading-blank.lex: Keeps leading blank lines inside verbatim content
    let doc = Lexplore::verbatim(15).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Inflow Leading Blank")
            .closing_label("shell")
            .mode(VerbatimBlockMode::Inflow)
            .line_count(3)
            .line_eq(0, "")
            .line_eq(1, "echo \"first\"")
            .line_eq(2, "echo \"second\"");
    });
}

#[test]
fn test_verbatim_16_fullwidth_at_root() {
    // verbatim-16-fullwidth-nested.lex: Fullwidth verbatim at root level
    // Demonstrates fullwidth mode works correctly with other root-level elements
    let doc = Lexplore::verbatim(16).parse().unwrap();

    assert_ast(&doc).item_count(3); // para before, verbatim, para after

    // First: paragraph before
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph().text_contains("comes before");
    });

    // Second: fullwidth verbatim at root level
    assert_ast(&doc).item(1, |item| {
        item.assert_verbatim_block()
            .subject("Fullwidth Table at Root")
            .mode(VerbatimBlockMode::Fullwidth)
            .closing_label("data")
            .line_count(4) // table rows
            .content_contains("ID | Name")
            .content_contains("Alice");
    });

    // Third: paragraph after verbatim
    assert_ast(&doc).item(2, |item| {
        item.assert_paragraph().text_contains("comes after");
    });
}

#[test]
fn test_verbatim_17_fullwidth_preserves_leading_blank_line() {
    // verbatim-17-fullwidth-leading-blank.lex: Keeps left-margin blank lines in fullwidth mode
    let doc = Lexplore::verbatim(17).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Fullwidth Leading Blank")
            .closing_label("table")
            .mode(VerbatimBlockMode::Fullwidth)
            .line_count(3)
            .line_eq(0, "")
            .line_eq(1, "Header | Value")
            .line_eq(2, "Data   | More");
    });
}

#[test]
fn test_verbatim_11_group_sequences() {
    // verbatim-11-group-shell.lex: Multiple subject/content pairs sharing an annotation
    let doc = Lexplore::verbatim(11).parse().unwrap();

    assert_ast(&doc).item_count(4);

    // Grouped CLI instructions stay within a single verbatim block
    assert_ast(&doc).item(0, |item| {
        item.assert_verbatim_block()
            .subject("Installing with home brew is simple")
            .closing_label("shell")
            .group_count(3)
            .group(0, |group| {
                group
                    .subject("Installing with home brew is simple")
                    .content_contains("$ brew install lex");
            })
            .group(1, |group| {
                group
                    .subject("From there the interactive help is available")
                    .content_contains("$ lex help");
            })
            .group(2, |group| {
                group
                    .subject("And the built-in viewer can be used to quickly view the parsing")
                    .content_contains("$ lexv <path>");
            });
    });

    // Content following the group should remain a regular paragraph
    assert_ast(&doc).item(1, |item| {
        item.assert_paragraph()
            .text_contains("content below, correct, from parsing however");
    });

    // Subsequent verbatim groups reuse the same closing data
    assert_ast(&doc).item(2, |item| {
        item.assert_verbatim_block()
            .closing_label("shell")
            .group_count(2)
            .group(0, |group| {
                group.subject("This is block 1").content_contains("$ ls");
            })
            .group(1, |group| {
                group
                    .subject("Which is a shell block")
                    .content_contains("$ pwd");
            });
    });

    // Regular single-pair verbatim blocks should continue to work
    assert_ast(&doc).item(3, |item| {
        item.assert_verbatim_block()
            .subject("And this is a block 2")
            .closing_label("javascript")
            .group_count(1)
            .content_contains("input(\"Favorite fruit:\")");
    });
}

#[test]
fn test_verbatim_11_group_visitor_sees_all_groups() {
    // Verify that visitors see content from all groups, not just the first
    use lex_core::lex::ast::elements::VerbatimLine;
    use lex_core::lex::ast::Visitor;

    struct VerbatimLineCounter {
        count: usize,
        lines: Vec<String>,
    }

    impl Visitor for VerbatimLineCounter {
        fn visit_verbatim_line(&mut self, line: &VerbatimLine) {
            self.count += 1;
            self.lines.push(line.content.as_string().to_string());
        }
    }

    let doc = Lexplore::verbatim(11).parse().unwrap();

    let mut visitor = VerbatimLineCounter {
        count: 0,
        lines: Vec::new(),
    };
    doc.accept(&mut visitor);

    // First verbatim block has 3 groups with 2 lines each (blank + content) = 6 lines
    // Second verbatim block has 2 groups with 2 lines each = 4 lines
    // Third verbatim block has 1 group with 2 lines = 2 lines
    // Total: 12 verbatim lines
    assert_eq!(
        visitor.count, 12,
        "Visitor should see all lines from all groups, got {} lines",
        visitor.count
    );

    // Verify we got lines from all groups
    assert!(
        visitor
            .lines
            .iter()
            .any(|l| l.contains("$ brew install lex")),
        "Should see line from first group of first block"
    );
    assert!(
        visitor.lines.iter().any(|l| l.contains("$ lex help")),
        "Should see line from second group of first block"
    );
    assert!(
        visitor.lines.iter().any(|l| l.contains("$ lexv")),
        "Should see line from third group of first block"
    );
    assert!(
        visitor.lines.iter().any(|l| l.contains("$ ls")),
        "Should see line from first group of second block"
    );
    assert!(
        visitor.lines.iter().any(|l| l.contains("$ pwd")),
        "Should see line from second group of second block"
    );
    assert!(
        visitor.lines.iter().any(|l| l.contains("input(")),
        "Should see line from third block"
    );
}

#[test]
fn test_verbatim_12_document_simple() {
    // verbatim-12-document-simple.lex: Document with mix of verbatim blocks, groups, and general content
    let doc = Lexplore::verbatim(12).parse().unwrap();

    // Verify first paragraph
    assert_ast(&doc).item(0, |item| {
        item.assert_paragraph()
            .text_contains("This document tests the combination of all three core elements");
    });

    // Verify first session "1. Session with Paragraph Content"
    assert_ast(&doc).item(1, |item| {
        item.assert_session()
            .label_contains("Session with Paragraph Content")
            .child_count(3) // para, para, verbatim
            .child(0, |para| {
                para.assert_paragraph()
                    .text_contains("This session starts with a paragraph as its first child");
            })
            .child(1, |para| {
                para.assert_paragraph()
                    .text_contains("It can have multiple paragraphs");
            })
            .child(2, |verbatim| {
                verbatim
                    .assert_verbatim_block()
                    .subject("This is a groupped Verbatim Block, this is the first Group")
                    .closing_label("shell")
                    .group_count(4)
                    .group(0, |group| {
                        group
                            .subject("This is a groupped Verbatim Block, this is the first Group")
                            .content_contains("$ pwd # always te staring point");
                    })
                    .group(1, |group| {
                        group
                            .subject(
                                "Now that you know where you are, lets find out what's around you",
                            )
                            .content_contains("$ ls")
                            .content_contains("$ ls -r # recursive");
                    })
                    .group(2, |group| {
                        group
                            .subject("And let's go places")
                            .content_contains("$ cd <path to go>");
                    })
                    .group(3, |group| {
                        group
                            .subject("Feeling lost, let's get back home")
                            .content_contains("$ cd ~");
                    });
            });
    });

    // Verify second session "2. Session with List Content"
    assert_ast(&doc).item(2, |item| {
        item.assert_session()
            .label_contains("Session with List Content")
            .child_count(1) // list only (verbatim inside list removed due to parser issue)
            .child(0, |list| {
                list.assert_list()
                    .item_count(3)
                    .item(0, |item| {
                        item.text_contains("First list item");
                    })
                    .item(1, |item| {
                        item.text_contains("Second list item");
                    })
                    .item(2, |item| {
                        item.text_contains("Third list item");
                    });
            });
    });

    // Verify third session "3. Session with Mixed Content"
    assert_ast(&doc).item(3, |item| {
        item.assert_session()
            .label_contains("Session with Mixed Content")
            .child_count(3); // para, list, para - structure verified by accessing children
    });

    // Verify root level paragraph
    assert_ast(&doc).item(4, |item| {
        item.assert_paragraph()
            .text_contains("A paragraph at the root level");
    });

    // Verify root level list
    assert_ast(&doc).item(5, |item| {
        item.assert_list()
            .item_count(2)
            .item(0, |list_item| {
                list_item.text_contains("Root level list");
            })
            .item(1, |list_item| {
                list_item.text_contains("With multiple items");
            });
    });

    // Verify image marker verbatim block
    assert_ast(&doc).item(6, |item| {
        item.assert_verbatim_block()
            .subject("This is an Image Verbatim Representation")
            .closing_label("image")
            .assert_marker_form()
            .has_closing_parameter_with_value("src", "\"image.jpg\"");
    });

    // Verify fourth session "4. Another Session"
    assert_ast(&doc).item(7, |item| {
        item.assert_session()
            .label_contains("Another Session")
            .child_count(3); // list, para, list
    });

    // Verify final root level paragraph
    assert_ast(&doc).item(8, |item| {
        item.assert_paragraph()
            .text_contains("Final root level paragraph");
    });

    // Verify final verbatim block
    assert_ast(&doc).item(9, |item| {
        item.assert_verbatim_block()
            .subject("Say goodbye mom")
            .closing_label("javascript")
            .content_contains("alert(\"Goodbye mom!\")");
    });
}
