//! Property-based tests for annotation attachment to elements
//!
//! All previous annotation proptests were standalone (parse_annotation_without_attachment).
//! These tests verify annotations correctly attach to their target elements
//! (paragraphs, definitions, lists, sessions, verbatim blocks) through the
//! full parse_document pipeline.

use lex_core::lex::parsing::parse_document;
use lex_core::lex::testing::assert_ast;
use proptest::prelude::*;

// =============================================================================
// Strategies
// =============================================================================

fn label_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,8}"
}

fn subject_strategy() -> impl Strategy<Value = String> {
    "[A-Z][a-zA-Z0-9 ]{1,20}"
        .prop_map(|s| s.trim_end().to_string())
        .prop_filter("must not end with colon or be empty", |s| {
            !s.ends_with(':') && !s.is_empty()
        })
}

fn paragraph_line() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ [a-z]+ [a-z]+[.]"
}

fn list_text() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ [a-z]+"
}

// =============================================================================
// 1. Annotation attached to paragraph
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn annotation_attaches_to_following_paragraph(
        label in label_strategy(),
        para in paragraph_line(),
    ) {
        // Annotation followed by paragraph (inside a session to avoid root-level ambiguity)
        let source = format!(
            "Title:\n\n    :: {label} ::\n    {para}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .child(0, |child| {
                        child.assert_paragraph()
                            .annotation_count(1)
                            .annotation(0, |ann| {
                                ann.label(&label);
                            });
                    });
            });
    }

    #[test]
    fn annotation_attaches_to_definition(
        label in label_strategy(),
        def_subject in subject_strategy(),
        content in paragraph_line(),
    ) {
        let source = format!(
            "Title:\n\n    :: {label} ::\n    {def_subject}:\n        {content}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .child(0, |child| {
                        child.assert_definition()
                            .subject(&def_subject)
                            .annotation_count(1)
                            .annotation(0, |ann| {
                                ann.label(&label);
                            });
                    });
            });
    }

    #[test]
    fn annotation_attaches_to_list(
        label in label_strategy(),
        item1 in list_text(),
        item2 in list_text(),
    ) {
        let source = format!(
            "Title:\n\n    Intro.\n\n    :: {label} ::\n    - {item1}\n    - {item2}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .child(1, |child| {
                        child.assert_list()
                            .annotation_count(1)
                            .annotation(0, |ann| {
                                ann.label(&label);
                            });
                    });
            });
    }

    #[test]
    fn annotation_attaches_to_verbatim(
        ann_label in label_strategy(),
        verb_subject in subject_strategy(),
        verb_label in label_strategy(),
        code in "[a-zA-Z0-9]+",
    ) {
        let source = format!(
            "Title:\n\n    :: {ann_label} ::\n    {verb_subject}:\n        {code}\n    :: {verb_label} ::\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .child(0, |child| {
                        child.assert_verbatim_block()
                            .subject(&verb_subject)
                            .closing_label(&verb_label)
                            .annotation_count(1)
                            .annotation(0, |ann| {
                                ann.label(&ann_label);
                            });
                    });
            });
    }
}

// =============================================================================
// 2. Multiple annotations on same element
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn two_annotations_on_paragraph(
        label1 in label_strategy(),
        label2 in label_strategy(),
        para in paragraph_line(),
    ) {
        let source = format!(
            "Title:\n\n    :: {label1} ::\n    :: {label2} ::\n    {para}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .child(0, |child| {
                        child.assert_paragraph()
                            .annotation_count(2);
                    });
            });
    }
}

// =============================================================================
// 3. Annotation with parameters attached to element
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn annotation_with_params_attaches_to_paragraph(
        label in label_strategy(),
        key in "[a-z][a-z0-9_]{0,6}",
        value in "[a-z0-9]+",
        para in paragraph_line(),
    ) {
        let source = format!(
            "Title:\n\n    :: {label} {key}={value} ::\n    {para}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .child(0, |child| {
                        child.assert_paragraph()
                            .annotation_count(1)
                            .annotation(0, |ann| {
                                ann.label(&label)
                                    .has_parameter_with_value(&key, &value);
                            });
                    });
            });
    }
}

// =============================================================================
// 4. Block annotation with content attached to element
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn block_annotation_attaches_to_paragraph(
        label in label_strategy(),
        ann_content in paragraph_line(),
        para in paragraph_line(),
    ) {
        let source = format!(
            "Title:\n\n    :: {label} ::\n        {ann_content}\n    ::\n    {para}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .child(0, |child| {
                        child.assert_paragraph()
                            .annotation_count(1)
                            .annotation(0, |ann| {
                                ann.label(&label)
                                    .child_count(1)
                                    .child(0, |c| { c.assert_paragraph(); });
                            });
                    });
            });
    }
}
