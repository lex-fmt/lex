//! Property-based tests for nested lists and complex list structures
//!
//! Covers: nested lists (list inside list item), lists with nested verbatim,
//! deeply nested structures, adjacent definitions, and mixed marker types.

use lex_core::lex::ast::elements::sequence_marker::{DecorationStyle, Form, Separator};
use lex_core::lex::ast::ContentItem;
use lex_core::lex::parsing::parse_document;
use lex_core::lex::testing::assert_ast;
use proptest::prelude::*;

// =============================================================================
// Strategies
// =============================================================================

fn subject_strategy() -> impl Strategy<Value = String> {
    "[A-Z][a-zA-Z0-9 ]{1,20}"
        .prop_map(|s| s.trim_end().to_string())
        .prop_filter("must not end with colon or be empty", |s| {
            !s.ends_with(':') && !s.is_empty()
        })
}

fn list_text() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ [a-z]+"
}

fn paragraph_line() -> impl Strategy<Value = String> {
    "[A-Z][a-z]+ [a-z]+ [a-z]+[.]"
}

fn label_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,8}"
}

// =============================================================================
// 1. Nested Lists (list inside list item)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn nested_dash_list_via_definition(
        def_subject in subject_strategy(),
        item1 in list_text(),
        item2 in list_text(),
        sub1 in list_text(),
        sub2 in list_text(),
    ) {
        // Nested lists inside a definition (avoids root-level session detection).
        // Outer list with blank line before it, inner list with blank line before it.
        let source = format!(
            "{def_subject}:\n    Intro text.\n\n    - {item1}\n        - {sub1}\n        - {sub2}\n    - {item2}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_definition()
                    .subject(&def_subject)
                    .child(1, |child| {
                        child.assert_list()
                            .item_count(2)
                            .item(0, |li| {
                                li.text_contains(&item1)
                                    .child(0, |nested| {
                                        nested.assert_list()
                                            .item_count(2)
                                            .item(0, |sub_li| { sub_li.text_contains(&sub1); })
                                            .item(1, |sub_li| { sub_li.text_contains(&sub2); });
                                    });
                            })
                            .item(1, |li| { li.text_contains(&item2); });
                    });
            });
    }

    #[test]
    fn nested_numbered_inside_dash_via_session(
        title in subject_strategy(),
        item1 in list_text(),
        item2 in list_text(),
        sub1 in list_text(),
        sub2 in list_text(),
    ) {
        // Session containing a list with nested numbered sublist
        let source = format!(
            "{title}:\n\n    - {item1}\n        1. {sub1}\n        2. {sub2}\n    - {item2}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let session_label = format!("{title}:");
        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .label(&session_label)
                    .child(0, |child| {
                        child.assert_list()
                            .item_count(2)
                            .item(0, |li| {
                                li.text_contains(&item1)
                                    .child(0, |nested| {
                                        nested.assert_list()
                                            .item_count(2)
                                            .item(0, |sub_li| { sub_li.text_contains(&sub1); })
                                            .item(1, |sub_li| { sub_li.text_contains(&sub2); });
                                    });
                            });
                    });
            });

        // Also verify the sublist uses numerical markers via direct AST access
        let session = doc.root.children.iter()
            .find_map(|i| i.as_session())
            .expect("Expected session");
        let outer = session.children.iter()
            .find_map(|i| i.as_list())
            .expect("Expected outer list");
        let first_item = outer.items[0]
            .as_list_item()
            .expect("Expected ListItem");
        let inner = first_item
            .children
            .iter()
            .find_map(|c| c.as_list())
            .expect("Expected inner list");
        let marker = inner.marker.as_ref().expect("sublist should have marker");
        assert_eq!(marker.style, DecorationStyle::Numerical);
    }

    #[test]
    fn three_level_nested_list_via_session(
        title in subject_strategy(),
        item1 in list_text(),
        item2 in list_text(),
        sub1 in list_text(),
        sub2 in list_text(),
        subsub1 in list_text(),
        subsub2 in list_text(),
    ) {
        // Three levels of list nesting inside a session.
        // Each level needs 2+ items to be recognized as a list.
        // The list needs a blank-line-separated intro before it.
        let source = format!(
            "{title}:\n\n\
             {0}Intro.\n\
             \n\
             {0}- {item1}\n\
             {0}{0}- {sub1}\n\
             {0}{0}{0}- {subsub1}\n\
             {0}{0}{0}- {subsub2}\n\
             {0}{0}- {sub2}\n\
             {0}- {item2}\n",
            "    "
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let session_label = format!("{title}:");
        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .label(&session_label)
                    .child(1, |child| {
                        child.assert_list()
                            .item_count(2)
                            .item(0, |li| {
                                li.text_contains(&item1)
                                    .child(0, |nested| {
                                        nested.assert_list()
                                            .item_count(2)
                                            .item(0, |sub_li| {
                                                sub_li.text_contains(&sub1)
                                                    .child(0, |deep| {
                                                        deep.assert_list()
                                                            .item_count(2)
                                                            .item(0, |ss| {
                                                                ss.text_contains(&subsub1);
                                                            })
                                                            .item(1, |ss| {
                                                                ss.text_contains(&subsub2);
                                                            });
                                                    });
                                            })
                                            .item(1, |sub_li| {
                                                sub_li.text_contains(&sub2);
                                            });
                                    });
                            })
                            .item(1, |li| { li.text_contains(&item2); });
                    });
            });
    }
}

// =============================================================================
// 2. List Items with Verbatim Blocks
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn list_item_with_verbatim(
        item1 in list_text(),
        item2 in list_text(),
        verbatim_subject in subject_strategy(),
        label in label_strategy(),
        code in "[a-zA-Z0-9 ]+",
    ) {
        let source = format!(
            "\n- {item1}\n    {verbatim_subject}:\n        {code}\n    :: {label} ::\n- {item2}\n"
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
                                child.assert_verbatim_block()
                                    .subject(&verbatim_subject)
                                    .closing_label(&label);
                            });
                    });
            });
    }
}

// =============================================================================
// 3. Definition with Verbatim Block
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn definition_with_verbatim(
        def_subject in subject_strategy(),
        verbatim_subject in subject_strategy(),
        label in label_strategy(),
        code in "[a-zA-Z0-9 ]+",
    ) {
        let source = format!(
            "{def_subject}:\n    {verbatim_subject}:\n        {code}\n    :: {label} ::\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_definition()
                    .subject(&def_subject)
                    .child(0, |child| {
                        child.assert_verbatim_block()
                            .subject(&verbatim_subject)
                            .closing_label(&label);
                    });
            });
    }
}

// =============================================================================
// 4. Adjacent Definitions
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn adjacent_definitions(
        subj1 in subject_strategy(),
        subj2 in subject_strategy(),
        content1 in paragraph_line(),
        content2 in paragraph_line(),
    ) {
        let source = format!(
            "{subj1}:\n    {content1}\n{subj2}:\n    {content2}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item_count(2)
            .item(0, |item| {
                item.assert_definition().subject(&subj1);
            })
            .item(1, |item| {
                item.assert_definition().subject(&subj2);
            });
    }

    #[test]
    fn three_adjacent_definitions(
        subj1 in subject_strategy(),
        subj2 in subject_strategy(),
        subj3 in subject_strategy(),
        c1 in paragraph_line(),
        c2 in paragraph_line(),
        c3 in paragraph_line(),
    ) {
        let source = format!(
            "{subj1}:\n    {c1}\n{subj2}:\n    {c2}\n{subj3}:\n    {c3}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item_count(3)
            .item(0, |item| { item.assert_definition().subject(&subj1); })
            .item(1, |item| { item.assert_definition().subject(&subj2); })
            .item(2, |item| { item.assert_definition().subject(&subj3); });
    }
}

// =============================================================================
// 5. Deep Nesting (4+ levels)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn four_level_session_nesting(
        t1 in subject_strategy(),
        t2 in subject_strategy(),
        t3 in subject_strategy(),
        t4 in subject_strategy(),
        content in paragraph_line(),
    ) {
        let source = format!(
            "{t1}:\n\n    {t2}:\n\n        {t3}:\n\n            {t4}:\n\n                {content}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let l1 = format!("{t1}:");
        let l2 = format!("{t2}:");
        let l3 = format!("{t3}:");
        let l4 = format!("{t4}:");
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
                                    .child(0, |c3| {
                                        c3.assert_session()
                                            .label(&l4)
                                            .child_count(1)
                                            .child(0, |c4| { c4.assert_paragraph(); });
                                    });
                            });
                    });
            });
    }

    #[test]
    fn four_level_definition_nesting(
        s1 in subject_strategy(),
        s2 in subject_strategy(),
        s3 in subject_strategy(),
        s4 in subject_strategy(),
        content in paragraph_line(),
    ) {
        let source = format!(
            "{s1}:\n    {s2}:\n        {s3}:\n            {s4}:\n                {content}\n"
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        assert_ast(&doc)
            .item(0, |item| {
                item.assert_definition()
                    .subject(&s1)
                    .child(0, |c1| {
                        c1.assert_definition()
                            .subject(&s2)
                            .child(0, |c2| {
                                c2.assert_definition()
                                    .subject(&s3)
                                    .child(0, |c3| {
                                        c3.assert_definition()
                                            .subject(&s4)
                                            .child_count(1);
                                    });
                            });
                    });
            });
    }
}

// =============================================================================
// 6. Extended Form Markers (1.2.3, I.a.2, etc.)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn list_marker_extended_numerical(
        item1 in list_text(),
        item2 in list_text(),
    ) {
        let source = format!("\n1.2.3. {item1}\n1.2.4. {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.form, Form::Extended);
        assert_eq!(marker.separator, Separator::Period);
    }

    #[test]
    fn list_marker_extended_mixed(
        item1 in list_text(),
        item2 in list_text(),
    ) {
        // Mixed extended form: 1.a) and 1.b)
        let source = format!("\n1.a) {item1}\n1.b) {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.form, Form::Extended);
        assert_eq!(marker.separator, Separator::Parenthesis);
    }

    #[test]
    fn list_marker_extended_two_level(
        item1 in list_text(),
        item2 in list_text(),
    ) {
        let source = format!("\n1.1. {item1}\n1.2. {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.form, Form::Extended);
        assert_eq!(marker.style, DecorationStyle::Numerical);
    }
}

// =============================================================================
// 7. Roman Numerals and Alphabetical Variants
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn list_marker_roman_period(
        item1 in list_text(),
        item2 in list_text(),
        item3 in list_text(),
    ) {
        let source = format!("\nI. {item1}\nII. {item2}\nIII. {item3}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.style, DecorationStyle::Roman);
        assert_eq!(marker.separator, Separator::Period);
        assert_eq!(marker.form, Form::Short);
    }

    #[test]
    fn list_marker_roman_double_parens(
        item1 in list_text(),
        item2 in list_text(),
    ) {
        let source = format!("\n(I) {item1}\n(II) {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.style, DecorationStyle::Roman);
        assert_eq!(marker.separator, Separator::DoubleParens);
    }

    #[test]
    fn list_marker_alpha_parenthesis(
        item1 in list_text(),
        item2 in list_text(),
    ) {
        let source = format!("\na) {item1}\nb) {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.style, DecorationStyle::Alphabetical);
        assert_eq!(marker.separator, Separator::Parenthesis);
    }

    #[test]
    fn list_marker_alpha_double_parens(
        item1 in list_text(),
        item2 in list_text(),
    ) {
        let source = format!("\n(a) {item1}\n(b) {item2}\n");
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let list = items
            .iter()
            .find_map(|item| if let ContentItem::List(l) = item { Some(l) } else { None })
            .expect("Expected a List");

        let marker = list.marker.as_ref().expect("List should have a marker");
        assert_eq!(marker.style, DecorationStyle::Alphabetical);
        assert_eq!(marker.separator, Separator::DoubleParens);
    }
}

// =============================================================================
// 7. Session with Mixed Content Types
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn session_with_all_element_types(
        title in subject_strategy(),
        para in paragraph_line(),
        def_subject in subject_strategy(),
        def_content in paragraph_line(),
        item1 in list_text(),
        item2 in list_text(),
        verbatim_subject in subject_strategy(),
        label in label_strategy(),
        code in "[a-zA-Z0-9 ]+",
    ) {
        // Session containing: paragraph, definition, list, verbatim
        let source = format!(
            "{title}:\n\n\
             {0}{para}\n\
             \n\
             {0}{def_subject}:\n\
             {0}{0}{def_content}\n\
             \n\
             {0}- {item1}\n\
             {0}- {item2}\n\
             \n\
             {0}{verbatim_subject}:\n\
             {0}{0}{code}\n\
             {0}:: {label} ::\n",
            "    "
        );
        let doc = parse_document(&source)
            .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));

        let session_label = format!("{title}:");
        assert_ast(&doc)
            .item(0, |item| {
                item.assert_session()
                    .label(&session_label)
                    .child_count(4)
                    .child(0, |c| { c.assert_paragraph(); })
                    .child(1, |c| {
                        c.assert_definition().subject(&def_subject);
                    })
                    .child(2, |c| {
                        c.assert_list().item_count(2);
                    })
                    .child(3, |c| {
                        c.assert_verbatim_block()
                            .subject(&verbatim_subject)
                            .closing_label(&label);
                    });
            });
    }
}
