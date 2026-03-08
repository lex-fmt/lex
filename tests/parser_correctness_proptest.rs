//! Property-based correctness tests for the lex parser
//!
//! Unlike parser_proptest.rs which only checks crash-safety (never panics),
//! these tests generate valid lex source, parse it, and verify the resulting
//! AST structure is correct. This covers element types and nesting patterns
//! that were previously untested.

use lex_core::lex::assembling::AttachRoot;
use lex_core::lex::ast::elements::sequence_marker::{DecorationStyle, Form, Separator};
use lex_core::lex::ast::ContentItem;
use lex_core::lex::parsing::engine::parse_from_flat_tokens;
use lex_core::lex::parsing::{parse_document, Document};
use lex_core::lex::testing::{assert_ast, InlineAssertion, InlineExpectation};
use lex_core::lex::transforms::standard::LEXING;
use lex_core::lex::transforms::Runnable;
use proptest::prelude::*;

// =============================================================================
// Helpers
// =============================================================================

fn ensure_trailing_newline(source: &str) -> String {
    if !source.is_empty() && !source.ends_with('\n') {
        format!("{source}\n")
    } else {
        source.to_string()
    }
}

fn parse_annotation_without_attachment(source: &str) -> Result<Document, String> {
    let source = ensure_trailing_newline(source);
    let tokens = LEXING.run(source.clone()).map_err(|e| e.to_string())?;
    let root = parse_from_flat_tokens(tokens, &source)?;
    AttachRoot::new().run(root).map_err(|e| e.to_string())
}

// =============================================================================
// Strategies
// =============================================================================

/// Generate valid label strings (for annotations, verbatim closing)
fn label_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,8}"
}

/// Generate simple text (no special lex characters)
fn simple_text_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9 ]{0,30}".prop_filter("must not end with colon", |s| {
        !s.trim_end().ends_with(':') && !s.trim().is_empty()
    })
}

/// Generate subject text (used for definitions, sessions, verbatim)
/// Trailing spaces are trimmed since the parser strips them from subjects/titles.
fn subject_strategy() -> impl Strategy<Value = String> {
    "[A-Z][a-zA-Z0-9 ]{1,20}"
        .prop_map(|s| s.trim_end().to_string())
        .prop_filter("must not end with colon or be empty", |s| {
            !s.ends_with(':') && !s.is_empty()
        })
}

/// Generate a paragraph line (simple text, no markers)
fn paragraph_line_strategy() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ [a-z]+ [a-z]+[.]"
}

/// Generate a list item text (no colons, no markers)
fn list_item_text_strategy() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ [a-z]+"
}

// =============================================================================
// 1. Definition Correctness
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn definition_simple(
        subject in subject_strategy(),
        content in paragraph_line_strategy(),
    ) {
        // Definition: subject followed immediately by indented content (no blank line)
        let source = format!("{subject}:\n    {content}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse definition: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_definition()
                    .subject(&subject)
                    .child_count(1)
                    .child(0, |child| {
                        child.assert_paragraph()
                            .text_contains(content.trim_end_matches('.'));
                    });
            });
    }

    #[test]
    fn definition_multi_paragraph(
        subject in subject_strategy(),
        para1 in paragraph_line_strategy(),
        para2 in paragraph_line_strategy(),
    ) {
        let source = format!("{subject}:\n    {para1}\n\n    {para2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_definition()
                    .subject(&subject)
                    .child_count(2);
            });
    }

    #[test]
    fn definition_with_list(
        subject in subject_strategy(),
        intro in paragraph_line_strategy(),
        item1 in list_item_text_strategy(),
        item2 in list_item_text_strategy(),
    ) {
        // Definition with paragraph then list (list needs blank line before it)
        let source = format!("{subject}:\n    {intro}\n\n    - {item1}\n    - {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_definition()
                    .subject(&subject)
                    .child_count(2)
                    .child(0, |child| { child.assert_paragraph(); })
                    .child(1, |child| {
                        child.assert_list()
                            .item_count(2)
                            .item(0, |li| { li.text_contains(&item1); })
                            .item(1, |li| { li.text_contains(&item2); });
                    });
            });
    }

    #[test]
    fn definition_nested(
        outer_subject in subject_strategy(),
        inner_subject in subject_strategy(),
        content in paragraph_line_strategy(),
    ) {
        let source = format!(
            "{outer_subject}:\n    {inner_subject}:\n        {content}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_definition()
                    .subject(&outer_subject)
                    .child(0, |child| {
                        child.assert_definition()
                            .subject(&inner_subject)
                            .child_count(1);
                    });
            });
    }
}

// =============================================================================
// 2. Annotation Correctness
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn annotation_marker_form(label in label_strategy()) {
        // Marker annotation: :: label :: (verify label and params are parsed correctly)
        let source = format!(":: {label} ::\n\nSome paragraph. {{{{paragraph}}}}\n");
        let doc = parse_annotation_without_attachment(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_annotation()
                    .label(&label)
                    .parameter_count(0);
            });
    }

    #[test]
    fn annotation_single_line(
        label in label_strategy(),
        text in simple_text_strategy(),
    ) {
        let source = format!(":: {label} :: {text}\n\nSome paragraph. {{{{paragraph}}}}\n");
        let doc = parse_annotation_without_attachment(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_annotation()
                    .label(&label)
                    .parameter_count(0)
                    .child_count(1)
                    .child(0, |child| {
                        child.assert_paragraph()
                            .text_contains(text.trim());
                    });
            });
    }

    #[test]
    fn annotation_block_form(
        label in label_strategy(),
        content in paragraph_line_strategy(),
    ) {
        let source = format!(":: {label} ::\n    {content}\n::\n\nSome paragraph. {{{{paragraph}}}}\n");
        let doc = parse_annotation_without_attachment(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_annotation()
                    .label(&label)
                    .child_count(1)
                    .child(0, |child| {
                        child.assert_paragraph();
                    });
            });
    }

    #[test]
    fn annotation_with_parameters(
        label in label_strategy(),
        key in "[a-z][a-z0-9_]{0,6}",
        value in "[a-z0-9]+",
    ) {
        let source = format!(":: {label} {key}={value} ::\n\nSome text. {{{{paragraph}}}}\n");
        let doc = parse_annotation_without_attachment(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_annotation()
                    .label(&label)
                    .parameter_count(1)
                    .has_parameter_with_value(&key, &value);
            });
    }

    #[test]
    fn annotation_block_with_paragraph(
        label in label_strategy(),
        para1 in paragraph_line_strategy(),
        para2 in paragraph_line_strategy(),
    ) {
        // Block annotation with multiple paragraphs
        let source = format!(
            ":: {label} ::\n    {para1}\n\n    {para2}\n::\n\nSome text. {{{{paragraph}}}}\n"
        );
        let doc = parse_annotation_without_attachment(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_annotation()
                    .label(&label)
                    .child_count(2)
                    .child(0, |child| { child.assert_paragraph(); })
                    .child(1, |child| { child.assert_paragraph(); });
            });
    }
}

// =============================================================================
// 3. Verbatim Block Correctness
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn verbatim_block_simple(
        subject in subject_strategy(),
        label in label_strategy(),
        line1 in "[a-zA-Z][a-zA-Z0-9 ]*",
        line2 in "[a-zA-Z][a-zA-Z0-9 ]*",
    ) {
        let source = format!(
            "{subject}:\n    {line1}\n    {line2}\n:: {label} ::\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_verbatim_block()
                    .subject(&subject)
                    .closing_label(&label)
                    .line_count(2)
                    .line_eq(0, &line1)
                    .line_eq(1, &line2);
            });
    }

    #[test]
    fn verbatim_block_with_parameters(
        subject in subject_strategy(),
        label in label_strategy(),
        key in "[a-z][a-z0-9_]{0,6}",
        value in "[a-z0-9]+",
        content in "[a-zA-Z][a-zA-Z0-9 ]*",
    ) {
        let source = format!(
            "{subject}:\n    {content}\n:: {label} {key}={value} ::\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_verbatim_block()
                    .subject(&subject)
                    .closing_label(&label)
                    .has_closing_parameter_with_value(&key, &value)
                    .line_count(1)
                    .line_eq(0, &content);
            });
    }

    #[test]
    fn verbatim_preserves_special_chars(
        subject in subject_strategy(),
        label in label_strategy(),
    ) {
        // Verbatim should preserve content that looks like lex syntax
        let special_content = "function() { return \"hello\"; }";
        let source = format!(
            "{subject}:\n    {special_content}\n:: {label} ::\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_verbatim_block()
                    .subject(&subject)
                    .closing_label(&label)
                    .content_contains("function()");
            });
    }
}

// =============================================================================
// 4. Nested Sessions
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn session_simple(
        title in subject_strategy(),
        content in paragraph_line_strategy(),
    ) {
        // Session: title with colon, blank line, then indented content
        let source = format!("{title}:\n\n    {content}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let expected_label = format!("{title}:");
        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .label(&expected_label)
                    .child_count(1)
                    .child(0, |child| {
                        child.assert_paragraph();
                    });
            });
    }

    #[test]
    fn session_nested(
        outer_title in subject_strategy(),
        inner_title in subject_strategy(),
        content in paragraph_line_strategy(),
    ) {
        let source = format!(
            "{outer_title}:\n\n    {inner_title}:\n\n        {content}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let outer_label = format!("{outer_title}:");
        let inner_label = format!("{inner_title}:");
        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .label(&outer_label)
                    .child(0, |child| {
                        child.assert_session()
                            .label(&inner_label)
                            .child_count(1)
                            .child(0, |grandchild| {
                                grandchild.assert_paragraph();
                            });
                    });
            });
    }

    #[test]
    fn session_three_levels_deep(
        t1 in subject_strategy(),
        t2 in subject_strategy(),
        t3 in subject_strategy(),
        content in paragraph_line_strategy(),
    ) {
        let source = format!(
            "{t1}:\n\n    {t2}:\n\n        {t3}:\n\n            {content}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let l1 = format!("{t1}:");
        let l2 = format!("{t2}:");
        let l3 = format!("{t3}:");
        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .label(&l1)
                    .child(0, |c1| {
                        c1.assert_session()
                            .label(&l2)
                            .child(0, |c2| {
                                c2.assert_session()
                                    .label(&l3)
                                    .child_count(1);
                            });
                    });
            });
    }

    #[test]
    fn session_with_multiple_children(
        title in subject_strategy(),
        para1 in paragraph_line_strategy(),
        para2 in paragraph_line_strategy(),
        item1 in list_item_text_strategy(),
        item2 in list_item_text_strategy(),
    ) {
        let source = format!(
            "{title}:\n\n    {para1}\n\n    - {item1}\n    - {item2}\n\n    {para2}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let expected_label = format!("{title}:");
        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .label(&expected_label)
                    .child_count(3)
                    .child(0, |c| { c.assert_paragraph(); })
                    .child(1, |c| {
                        c.assert_list()
                            .item_count(2);
                    })
                    .child(2, |c| { c.assert_paragraph(); });
            });
    }
}

// =============================================================================
// 5. Nested List Items
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn list_items_with_nested_paragraph(
        item1 in list_item_text_strategy(),
        item2 in list_item_text_strategy(),
        nested_text in paragraph_line_strategy(),
    ) {
        // List item with indented paragraph child
        let source = format!(
            "\n- {item1}\n    {nested_text}\n- {item2}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_list()
                    .item_count(2)
                    .item(0, |li| {
                        li.text_contains(&item1)
                            .child_count(1)
                            .child(0, |child| {
                                child.assert_paragraph();
                            });
                    })
                    .item(1, |li| {
                        li.text_contains(&item2);
                    });
            });
    }

    #[test]
    fn list_items_with_nested_definition(
        item1 in list_item_text_strategy(),
        item2 in list_item_text_strategy(),
        def_subject in subject_strategy(),
        def_content in paragraph_line_strategy(),
    ) {
        // List items with nested definition (more reliable than nested lists
        // since nested lists require careful blank line handling)
        let source = format!(
            "\n- {item1}\n    {def_subject}:\n        {def_content}\n- {item2}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_list()
                    .item_count(2)
                    .item(0, |li| {
                        li.text_contains(&item1)
                            .child(0, |child| {
                                child.assert_definition()
                                    .subject(&def_subject);
                            });
                    });
            });
    }
}

// =============================================================================
// 6. Definition vs Session Disambiguation
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn definition_vs_session_blank_line_rule(
        subject in subject_strategy(),
        content in paragraph_line_strategy(),
    ) {
        // Without blank line → Definition
        let def_source = format!("{subject}:\n    {content}\n");
        let def_doc = parse_document(&def_source)
            .unwrap_or_else(|e| panic!("Failed to parse definition: {e}\nSource:\n{def_source}"));

        assert_ast(&def_doc)
            .item(0, |item| {
                item.assert_definition()
                    .subject(&subject);
            });

        // With blank line → Session
        let sess_source = format!("{subject}:\n\n    {content}\n");
        let sess_doc = parse_document(&sess_source)
            .unwrap_or_else(|e| panic!("Failed to parse session: {e}\nSource:\n{sess_source}"));

        let expected_label = format!("{subject}:");
        assert_ast(&sess_doc)
            .item(0, |item| {
                item.assert_session()
                    .label(&expected_label);
            });
    }
}

// =============================================================================
// 7. Mixed Content Documents
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn mixed_definition_and_paragraph(
        subject in subject_strategy(),
        def_content in paragraph_line_strategy(),
        paragraph in paragraph_line_strategy(),
    ) {
        let source = format!(
            "{subject}:\n    {def_content}\n\n{paragraph}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item_count(2)
            .item(0, |item| {
                item.assert_definition()
                    .subject(&subject);
            })
            .item(1, |item| {
                item.assert_paragraph()
                    .text_contains(paragraph.trim_end_matches('.'));
            });
    }

    #[test]
    fn session_containing_definition(
        session_title in subject_strategy(),
        def_subject in subject_strategy(),
        content in paragraph_line_strategy(),
    ) {
        let source = format!(
            "{session_title}:\n\n    {def_subject}:\n        {content}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let session_label = format!("{session_title}:");
        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .label(&session_label)
                    .child(0, |child| {
                        child.assert_definition()
                            .subject(&def_subject)
                            .child_count(1);
                    });
            });
    }

    #[test]
    fn session_with_verbatim(
        session_title in subject_strategy(),
        verbatim_subject in subject_strategy(),
        label in label_strategy(),
        code_line in "[a-zA-Z0-9 ]+",
    ) {
        let source = format!(
            "{session_title}:\n\n    {verbatim_subject}:\n        {code_line}\n    :: {label} ::\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let session_label = format!("{session_title}:");
        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .label(&session_label)
                    .child(0, |child| {
                        child.assert_verbatim_block()
                            .subject(&verbatim_subject)
                            .closing_label(&label);
                    });
            });
    }

    #[test]
    fn definition_inside_list_item(
        item1 in list_item_text_strategy(),
        item2 in list_item_text_strategy(),
        def_subject in subject_strategy(),
        def_content in paragraph_line_strategy(),
    ) {
        let source = format!(
            "\n- {item1}\n    {def_subject}:\n        {def_content}\n- {item2}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_list()
                    .item_count(2)
                    .item(0, |li| {
                        li.text_contains(&item1)
                            .child(0, |child| {
                                child.assert_definition()
                                    .subject(&def_subject);
                            });
                    });
            });
    }
}

// =============================================================================
// 8. Inline Markup Correctness (Priority 3)
// =============================================================================

/// Generate a word suitable for inline markup content (no special chars)
fn inline_word_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-z]{1,8}"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn inline_bold(word in inline_word_strategy()) {
        let source = format!("This has *{word}* here.\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let para = match &doc.root.children.iter().collect::<Vec<_>>()[0] {
            ContentItem::Paragraph(p) => p,
            other => panic!("Expected Paragraph, got {other:?}"),
        };
        let text_line = match &para.lines[0] {
            ContentItem::TextLine(tl) => tl,
            other => panic!("Expected TextLine, got {other:?}"),
        };
        InlineAssertion::new(&text_line.content, "bold test")
            .starts_with(&[
                InlineExpectation::plain_text("This has "),
                InlineExpectation::strong_text(&word),
                InlineExpectation::plain_text(" here."),
            ])
            .length(3);
    }

    #[test]
    fn inline_italic(word in inline_word_strategy()) {
        let source = format!("This has _{word}_ here.\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let para = match &doc.root.children.iter().collect::<Vec<_>>()[0] {
            ContentItem::Paragraph(p) => p,
            other => panic!("Expected Paragraph, got {other:?}"),
        };
        let text_line = match &para.lines[0] {
            ContentItem::TextLine(tl) => tl,
            other => panic!("Expected TextLine, got {other:?}"),
        };
        InlineAssertion::new(&text_line.content, "italic test")
            .starts_with(&[
                InlineExpectation::plain_text("This has "),
                InlineExpectation::emphasis_text(&word),
                InlineExpectation::plain_text(" here."),
            ])
            .length(3);
    }

    #[test]
    fn inline_code(word in inline_word_strategy()) {
        let source = format!("This has `{word}` here.\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let para = match &doc.root.children.iter().collect::<Vec<_>>()[0] {
            ContentItem::Paragraph(p) => p,
            other => panic!("Expected Paragraph, got {other:?}"),
        };
        let text_line = match &para.lines[0] {
            ContentItem::TextLine(tl) => tl,
            other => panic!("Expected TextLine, got {other:?}"),
        };
        InlineAssertion::new(&text_line.content, "code test")
            .starts_with(&[
                InlineExpectation::plain_text("This has "),
                InlineExpectation::code_text(&word),
                InlineExpectation::plain_text(" here."),
            ])
            .length(3);
    }

    #[test]
    fn inline_math(word in inline_word_strategy()) {
        let source = format!("This has #{word}# here.\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let para = match &doc.root.children.iter().collect::<Vec<_>>()[0] {
            ContentItem::Paragraph(p) => p,
            other => panic!("Expected Paragraph, got {other:?}"),
        };
        let text_line = match &para.lines[0] {
            ContentItem::TextLine(tl) => tl,
            other => panic!("Expected TextLine, got {other:?}"),
        };
        InlineAssertion::new(&text_line.content, "math test")
            .starts_with(&[
                InlineExpectation::plain_text("This has "),
                InlineExpectation::math_text(&word),
                InlineExpectation::plain_text(" here."),
            ])
            .length(3);
    }

    #[test]
    fn inline_bold_and_italic(
        bold_word in inline_word_strategy(),
        italic_word in inline_word_strategy(),
    ) {
        let source = format!("Has *{bold_word}* and _{italic_word}_ end.\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let para = match &doc.root.children.iter().collect::<Vec<_>>()[0] {
            ContentItem::Paragraph(p) => p,
            other => panic!("Expected Paragraph, got {other:?}"),
        };
        let text_line = match &para.lines[0] {
            ContentItem::TextLine(tl) => tl,
            other => panic!("Expected TextLine, got {other:?}"),
        };
        InlineAssertion::new(&text_line.content, "bold+italic test")
            .starts_with(&[
                InlineExpectation::plain_text("Has "),
                InlineExpectation::strong_text(&bold_word),
                InlineExpectation::plain_text(" and "),
                InlineExpectation::emphasis_text(&italic_word),
                InlineExpectation::plain_text(" end."),
            ])
            .length(5);
    }

    #[test]
    fn inline_nested_emphasis_inside_bold(
        outer in inline_word_strategy(),
        inner in inline_word_strategy(),
    ) {
        // Bold containing emphasis: *outer _inner_ outer*
        let source = format!("Text *{outer} _{inner}_ {outer}* end.\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let para = match &doc.root.children.iter().collect::<Vec<_>>()[0] {
            ContentItem::Paragraph(p) => p,
            other => panic!("Expected Paragraph, got {other:?}"),
        };
        let text_line = match &para.lines[0] {
            ContentItem::TextLine(tl) => tl,
            other => panic!("Expected TextLine, got {other:?}"),
        };
        // The strong node should contain: plain, emphasis, plain
        InlineAssertion::new(&text_line.content, "nested emphasis in bold")
            .starts_with(&[
                InlineExpectation::plain_text("Text "),
                InlineExpectation::strong(vec![
                    InlineExpectation::plain_text(format!("{outer} ")),
                    InlineExpectation::emphasis_text(&inner),
                    InlineExpectation::plain_text(format!(" {outer}")),
                ]),
                InlineExpectation::plain_text(" end."),
            ])
            .length(3);
    }

    #[test]
    fn inline_code_inside_bold(
        bold_word in inline_word_strategy(),
        code_word in inline_word_strategy(),
    ) {
        // Bold containing code: *bold `code` bold*
        let source = format!("Text *{bold_word} `{code_word}` {bold_word}* end.\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let para = match &doc.root.children.iter().collect::<Vec<_>>()[0] {
            ContentItem::Paragraph(p) => p,
            other => panic!("Expected Paragraph, got {other:?}"),
        };
        let text_line = match &para.lines[0] {
            ContentItem::TextLine(tl) => tl,
            other => panic!("Expected TextLine, got {other:?}"),
        };
        InlineAssertion::new(&text_line.content, "code inside bold")
            .starts_with(&[
                InlineExpectation::plain_text("Text "),
                InlineExpectation::strong(vec![
                    InlineExpectation::plain_text(format!("{bold_word} ")),
                    InlineExpectation::code_text(&code_word),
                    InlineExpectation::plain_text(format!(" {bold_word}")),
                ]),
                InlineExpectation::plain_text(" end."),
            ])
            .length(3);
    }
}

// =============================================================================
// 9. Blank Line Grouping (Priority 4)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn blank_line_grouping(
        title in subject_strategy(),
        para1 in paragraph_line_strategy(),
        blank_count in 1..5usize,
        para2 in paragraph_line_strategy(),
    ) {
        // BlankLineGroups are visible inside sessions (at root level, text + blank
        // line triggers session detection, so we test inside a session)
        let indent = "    ";
        let blanks = "\n".repeat(blank_count);
        let source = format!("{title}:\n\n{indent}{para1}\n{blanks}{indent}{para2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        // Get the session's children
        let session_items: Vec<&ContentItem> = match &doc.root.children.iter().collect::<Vec<_>>()[0] {
            ContentItem::Session(s) => s.children.iter().collect(),
            other => panic!("Expected Session, got {other:?}"),
        };

        // Should have blank line group(s) between paragraphs
        let blank_groups: Vec<_> = session_items
            .iter()
            .filter(|item| matches!(item, ContentItem::BlankLineGroup(_)))
            .collect();
        assert!(
            !blank_groups.is_empty(),
            "Expected at least one BlankLineGroup for {blank_count} blank lines\nSource:\n{source}",
        );

        // Should have exactly 2 paragraphs
        let paragraphs: Vec<_> = session_items
            .iter()
            .filter(|item| matches!(item, ContentItem::Paragraph(_)))
            .collect();
        assert_eq!(paragraphs.len(), 2, "Expected 2 paragraphs\nSource:\n{source}");
    }
}

// =============================================================================
// 10. List Marker Variety (Priority 4)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn list_marker_dash(
        item1 in list_item_text_strategy(),
        item2 in list_item_text_strategy(),
    ) {
        let source = format!("\n- {item1}\n- {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.style, DecorationStyle::Plain);
        assert_eq!(marker.form, Form::Short);
    }

    #[test]
    fn list_marker_numerical_period(
        item1 in list_item_text_strategy(),
        item2 in list_item_text_strategy(),
    ) {
        let source = format!("\n1. {item1}\n2. {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.style, DecorationStyle::Numerical);
        assert_eq!(marker.separator, Separator::Period);
        assert_eq!(marker.form, Form::Short);
    }

    #[test]
    fn list_marker_alphabetical_period(
        item1 in list_item_text_strategy(),
        item2 in list_item_text_strategy(),
    ) {
        let source = format!("\na. {item1}\nb. {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.style, DecorationStyle::Alphabetical);
        assert_eq!(marker.separator, Separator::Period);
    }

    #[test]
    fn list_marker_numerical_parenthesis(
        item1 in list_item_text_strategy(),
        item2 in list_item_text_strategy(),
    ) {
        let source = format!("\n1) {item1}\n2) {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.style, DecorationStyle::Numerical);
        assert_eq!(marker.separator, Separator::Parenthesis);
    }

    #[test]
    fn list_marker_double_parens(
        item1 in list_item_text_strategy(),
        item2 in list_item_text_strategy(),
    ) {
        let source = format!("\n(1) {item1}\n(2) {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.style, DecorationStyle::Numerical);
        assert_eq!(marker.separator, Separator::DoubleParens);
    }
}
