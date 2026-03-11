//! Unit tests for isolated paragraph elements
//!
//! Tests paragraph parsing in isolation following the on-lexplore.lex guidelines:
//! - Use Lexplore to load centralized test files
//! - Use assert_ast for deep structure verification
//! - Test isolated elements (one element per test)
//! - Verify content and structure, not just counts

use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;

#[test]
fn test_paragraph_01_flat_oneline() {
    // paragraph-01-flat-oneline.lex: "This is a simple paragraph with just one line."
    let doc = Lexplore::paragraph(1).parse().unwrap();

    // Verify the document contains exactly one paragraph with expected content
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_paragraph()
            .text("This is a simple paragraph with just one line.")
            .line_count(1);
    });
}

#[test]
fn test_paragraph_02_flat_multiline() {
    // paragraph-02-flat-multiline.lex: Three lines of text
    let doc = Lexplore::paragraph(2).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_paragraph()
            .text_contains("This is a multi-line paragraph")
            .text_contains("It continues on the second line")
            .text_contains("And even has a third line")
            .line_count(3);
    });
}

#[test]
fn test_paragraph_03_flat_special_chars() {
    // paragraph-03-flat-special-chars.lex: Tests that special characters are preserved
    let doc = Lexplore::paragraph(3).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_paragraph()
            .text_contains("!@#$%^&*()_+-=[]{}|;':\",./<>?")
            .text_contains("special characters")
            .line_count(1);
    });
}

#[test]
fn test_paragraph_04_flat_numbers() {
    let doc = Lexplore::paragraph(4).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_paragraph()
            .text_contains("123")
            .text_contains("456")
            .text_contains("789");
    });
}

#[test]
fn test_paragraph_05_flat_mixed_content() {
    let doc = Lexplore::paragraph(5).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_paragraph()
            .text_contains("quick brown fox")
            .text_contains("123 ABC def");
    });
}

#[test]
fn test_paragraph_06_nested_in_session() {
    let doc = Lexplore::paragraph(6).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session()
            .label("Introduction:")
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("paragraph nested inside a session")
                    .text_contains("spans multiple lines");
            });
    });
}

#[test]
fn test_paragraph_07_nested_in_definition() {
    let doc = Lexplore::paragraph(7).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition().subject("Cache").child(0, |child| {
            child
                .assert_paragraph()
                .text_contains("nested inside a definition");
        });
    });
}

#[test]
fn test_paragraph_08_nested_deeply() {
    let doc = Lexplore::paragraph(8).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_definition()
            .subject("Authentication")
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("paragraph in the first definition");
            })
            .child(1, |child| {
                child
                    .assert_definition()
                    .subject("OAuth")
                    .child(0, |grandchild| {
                        grandchild
                            .assert_paragraph()
                            .text_contains("paragraph in the nested definition");
                    })
                    .child(1, |grandchild| {
                        grandchild
                            .assert_definition()
                            .subject("JWT")
                            .child(0, |leaf| {
                                leaf.assert_paragraph()
                                    .text_contains("deeply nested three levels down");
                            });
                    });
            });
    });
}

#[test]
fn test_paragraph_09_dialog() {
    let doc = Lexplore::paragraph(9).parse().unwrap();

    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_paragraph()
            .text_contains("Hi mom")
            .text_contains("Hi kiddo");
    });
}
