use lex_babel::formats::lex::export;
use lex_babel::transforms::serialize_to_lex;
use lex_core::lex::ast::elements::container::{GeneralContainer, ListContainer, SessionContainer};
use lex_core::lex::ast::elements::sequence_marker::{
    DecorationStyle, Form, Separator, SequenceMarker,
};
use lex_core::lex::ast::elements::typed_content::{
    ContentElement, SessionContent, VerbatimContent,
};
use lex_core::lex::ast::elements::verbatim::VerbatimBlockMode;
use lex_core::lex::ast::elements::verbatim_line::VerbatimLine;
use lex_core::lex::ast::elements::BlankLineGroup;
use lex_core::lex::ast::*;
use lex_core::lex::parsing::parse_document;
use proptest::prelude::*;

use crate::skeleton::canon;

// -----------------------------------------------------------------------------
// AST Node Generators
// -----------------------------------------------------------------------------

fn paragraph_strategy() -> impl Strategy<Value = Paragraph> {
    prop::collection::vec("[a-zA-Z0-9]+( [a-zA-Z0-9]+)*", 1..5).prop_map(|lines| Paragraph {
        lines: lines
            .into_iter()
            .map(|line| ContentItem::TextLine(TextLine::new(TextContent::from_string(line, None))))
            .collect(),
        annotations: vec![],
        location: Default::default(),
    })
}

fn list_item_strategy() -> impl Strategy<Value = ListItem> {
    ("[a-zA-Z0-9]+( [a-zA-Z0-9]+)*").prop_map(|text| ListItem {
        marker: TextContent::from_string("-".to_string(), None),
        text: vec![TextContent::from_string(format!("{text}\n"), None)],
        children: GeneralContainer::empty(),
        annotations: vec![],
        location: Default::default(),
    })
}

fn list_item_with_children_strategy() -> impl Strategy<Value = ListItem> {
    (
        "[a-zA-Z0-9]+( [a-zA-Z0-9]+)*",
        paragraph_strategy(),
        prop::option::of(list_strategy()),
        any::<bool>(),
    )
        .prop_map(|(text, para, maybe_list, insert_blank)| {
            let mut children = GeneralContainer::empty();
            children.push(ContentItem::Paragraph(para));
            if let Some(list) = maybe_list {
                if insert_blank {
                    children.push(ContentItem::BlankLineGroup(BlankLineGroup {
                        count: 1,
                        source_tokens: vec![],
                        location: Default::default(),
                    }));
                }
                children.push(ContentItem::List(list));
            }
            ListItem {
                marker: TextContent::from_string("-".to_string(), None),
                text: vec![TextContent::from_string(format!("{text}\n"), None)],
                children,
                annotations: vec![],
                location: Default::default(),
            }
        })
}

fn list_strategy() -> impl Strategy<Value = List> {
    prop::collection::vec(list_item_strategy(), 2..5).prop_map(|items| {
        let mut list_container = ListContainer::empty();
        for item in items {
            list_container.push(ContentItem::ListItem(item));
        }
        let mut list = List::new(vec![]);
        list.items = list_container;
        list.marker = Some(SequenceMarker {
            raw_text: TextContent::from_string("-".to_string(), None),
            style: DecorationStyle::Plain,
            separator: Separator::Period,
            form: Form::Short,
            location: Default::default(),
        });
        list
    })
}

fn definition_child_strategy() -> impl Strategy<Value = ContentElement> {
    prop_oneof![
        paragraph_strategy().prop_map(ContentElement::Paragraph),
        list_strategy().prop_map(ContentElement::List),
    ]
}

fn definition_strategy() -> impl Strategy<Value = Definition> {
    (
        "[A-Z][a-zA-Z0-9 ]*".prop_map(|s| s.trim_end().to_string()),
        prop::collection::vec(definition_child_strategy(), 1..3).prop_filter(
            "No consecutive lists",
            |items| {
                let mut prev_was_list = false;
                for item in items {
                    let is_list = matches!(item, ContentElement::List(_));
                    if is_list && prev_was_list {
                        return false;
                    }
                    prev_was_list = is_list;
                }
                true
            },
        ),
    )
        // Reader-shaped: no pre-inserted BlankLineGroup separators between body
        // children. The serializer's separation matrix (lex#782/#783) now supplies
        // every structural blank, so the old "always separate paragraphs" workaround
        // is obsolete — the children go in exactly as a foreign reader would build
        // them. `matrix.rs::faithful_definition_followed_by_each_non_hijack_block`
        // is the proof this shape round-trips.
        .prop_map(|(subject, children)| {
            Definition::new(TextContent::from_string(subject, None), children)
        })
}

fn label_strategy() -> impl Strategy<Value = Label> {
    "[a-z][a-z0-9_-]{0,8}".prop_map(Label::new)
}

/// Generates verbatim content lines that may include internal indentation,
/// subject-like patterns, annotation-like patterns, and other tricky content
/// that should be preserved verbatim (not parsed as Lex structure).
fn verbatim_line_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Plain text
        "[a-zA-Z][a-zA-Z0-9 ]*",
        // Indented content (code-like)
        "    [a-zA-Z][a-zA-Z0-9 ]*",
        // Double-indented content
        "        [a-zA-Z][a-zA-Z0-9]*",
        // Subject-like line (ends with colon) — should NOT be parsed as definition
        "[A-Z][a-zA-Z0-9 ]*:",
        // List-like line — should NOT be parsed as list item
        "- [a-zA-Z0-9 ]+",
        // Mixed indentation (code with nested blocks)
        "  if [a-z]+ \\{",
        "    return [a-z]+;",
        "  \\}",
    ]
}

fn verbatim_strategy() -> impl Strategy<Value = Verbatim> {
    (
        "[A-Z][a-zA-Z0-9 ]*".prop_map(|s| s.trim_end().to_string()),
        label_strategy(),
        prop::collection::vec(verbatim_line_strategy(), 1..6),
    )
        .prop_map(|(subject, label, lines)| {
            let verbatim_lines: Vec<VerbatimContent> = lines
                .into_iter()
                .map(|l| VerbatimContent::VerbatimLine(VerbatimLine::new(l)))
                .collect();
            let closing_data = Data::new(label, vec![]);
            Verbatim::new(
                TextContent::from_string(subject, None),
                verbatim_lines,
                closing_data,
                VerbatimBlockMode::Inflow,
            )
        })
}

/// Session content: paragraphs and lists only (the proven baseline).
/// Definitions and verbatim blocks are tested separately via dedicated round-trip
/// tests because their nesting/indentation semantics require careful spacing.
fn session_content_strategy() -> impl Strategy<Value = SessionContent> {
    prop_oneof![
        paragraph_strategy().prop_map(|p| SessionContent::Element(ContentElement::Paragraph(p))),
        list_strategy().prop_map(|l| SessionContent::Element(ContentElement::List(l))),
    ]
}

fn session_strategy() -> impl Strategy<Value = Session> {
    (
        "[a-zA-Z0-9]+",
        prop::collection::vec(session_content_strategy(), 1..3).prop_filter(
            "No consecutive lists",
            |items| {
                let mut prev_was_list = false;
                for item in items {
                    let is_list = matches!(item, SessionContent::Element(ContentElement::List(_)));
                    if is_list && prev_was_list {
                        return false;
                    }
                    prev_was_list = is_list;
                }
                true
            },
        ),
    )
        // Reader-shaped: no pre-inserted BlankLineGroups between body elements and
        // no trailing separator — the shape a foreign reader produces. The
        // separation matrix (lex#782/#783) supplies the structural blanks on
        // serialize, so the old "always separate paragraphs" + trailing-boundary
        // workarounds are obsolete. `matrix.rs::faithful_blocks_nested_in_a_session_body`
        // proves a mixed reader-shaped session body round-trips.
        .prop_map(|(title, content)| Session {
            title: TextContent::from_string(title, None),
            marker: None,
            children: SessionContainer::from_typed(content),
            annotations: vec![],
            location: Default::default(),
        })
}

fn nested_session_strategy() -> impl Strategy<Value = Session> {
    (
        "[a-zA-Z0-9]+",
        prop::collection::vec(
            paragraph_strategy()
                .prop_map(|p| SessionContent::Element(ContentElement::Paragraph(p))),
            1..2,
        ),
        session_strategy(),
    )
        // Reader-shaped: the leading paragraph(s) and the nested child session are
        // adjacent siblings with no BlankLineGroup between them; the matrix supplies
        // the Paragraph→Session structural blank on serialize.
        .prop_map(|(title, content, child_session)| {
            let mut children: Vec<SessionContent> = content;
            children.push(SessionContent::Session(child_session));

            Session {
                title: TextContent::from_string(title, None),
                marker: None,
                children: SessionContainer::from_typed(children),
                annotations: vec![],
                location: Default::default(),
            }
        })
}

fn document_strategy() -> impl Strategy<Value = Document> {
    prop::collection::vec(session_strategy(), 1..3).prop_map(|sessions| {
        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(
            sessions.into_iter().map(SessionContent::Session).collect(),
        );
        doc
    })
}

fn document_with_nested_sessions_strategy() -> impl Strategy<Value = Document> {
    prop::collection::vec(nested_session_strategy(), 1..2).prop_map(|sessions| {
        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(
            sessions.into_iter().map(SessionContent::Session).collect(),
        );
        doc
    })
}

// -----------------------------------------------------------------------------
// The Round-Trip Tests
// -----------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn test_round_trip_holy_grail(ast in document_strategy()) {
        let serialized = export(&ast).expect("Serialization should not fail");
        let parsed = parse_document(&serialized).expect("Parsing should not fail");

        assert!(!parsed.root.children.is_empty());

        let e_items: Vec<&ContentItem> = ast.root.children.iter().collect();
        let a_items: Vec<&ContentItem> = parsed.root.children.iter().collect();
        assert_ast_equiv(&e_items, &a_items, &serialized);
    }

    #[test]
    fn test_round_trip_nested_sessions(ast in document_with_nested_sessions_strategy()) {
        let serialized = export(&ast).expect("Serialization should not fail");
        let parsed = parse_document(&serialized).expect("Parsing should not fail");

        assert!(!parsed.root.children.is_empty());

        let e_items: Vec<&ContentItem> = ast.root.children.iter().collect();
        let a_items: Vec<&ContentItem> = parsed.root.children.iter().collect();
        assert_ast_equiv(&e_items, &a_items, &serialized);
    }
}

// Standalone tests for individual element round-trips

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn test_round_trip_definition(def in definition_strategy()) {
        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Element(ContentElement::Definition(def)),
        ]);

        let serialized = export(&doc).expect("Serialization should not fail");
        let parsed = parse_document(&serialized).expect("Parsing should not fail");

        let e_items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let a_items: Vec<&ContentItem> = parsed.root.children.iter().collect();
        assert_ast_equiv(&e_items, &a_items, &serialized);
    }

    #[test]
    fn test_round_trip_verbatim(verb in verbatim_strategy()) {
        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Element(ContentElement::VerbatimBlock(Box::new(verb))),
        ]);

        let serialized = export(&doc).expect("Serialization should not fail");
        let parsed = parse_document(&serialized).expect("Parsing should not fail");

        let e_items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let a_items: Vec<&ContentItem> = parsed.root.children.iter().collect();
        assert_ast_equiv(&e_items, &a_items, &serialized);
    }

    #[test]
    fn test_round_trip_list_with_children(
        item1 in list_item_with_children_strategy(),
        item2 in list_item_strategy(),
    ) {
        let mut list_container = ListContainer::empty();
        list_container.push(ContentItem::ListItem(item1));
        list_container.push(ContentItem::ListItem(item2));
        let mut list = List::new(vec![]);
        list.items = list_container;
        list.marker = Some(SequenceMarker {
            raw_text: TextContent::from_string("-".to_string(), None),
            style: DecorationStyle::Plain,
            separator: Separator::Period,
            form: Form::Short,
            location: Default::default(),
        });

        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Element(ContentElement::List(list)),
        ]);

        let serialized = export(&doc).expect("Serialization should not fail");
        let parsed = parse_document(&serialized).expect("Parsing should not fail");

        let e_items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let a_items: Vec<&ContentItem> = parsed.root.children.iter().collect();
        assert_ast_equiv(&e_items, &a_items, &serialized);
    }
}

// Definitions and verbatim inside sessions

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn test_round_trip_session_with_definition(
        title in "[a-zA-Z0-9]+",
        def in definition_strategy(),
    ) {
        let content = vec![
            SessionContent::Element(ContentElement::Definition(def)),
            SessionContent::Element(ContentElement::BlankLineGroup(BlankLineGroup {
                count: 1,
                source_tokens: vec![],
                location: Default::default(),
            })),
        ];
        let session = Session {
            title: TextContent::from_string(title, None),
            marker: None,
            children: SessionContainer::from_typed(content),
            annotations: vec![],
            location: Default::default(),
        };

        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Session(session),
        ]);

        let serialized = export(&doc).expect("Serialization should not fail");
        let parsed = parse_document(&serialized).expect("Parsing should not fail");

        let e_items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let a_items: Vec<&ContentItem> = parsed.root.children.iter().collect();
        assert_ast_equiv(&e_items, &a_items, &serialized);
    }

    #[test]
    fn test_round_trip_session_with_verbatim(
        title in "[a-zA-Z0-9]+",
        verb in verbatim_strategy(),
    ) {
        let content = vec![
            SessionContent::Element(ContentElement::VerbatimBlock(Box::new(verb))),
            SessionContent::Element(ContentElement::BlankLineGroup(BlankLineGroup {
                count: 1,
                source_tokens: vec![],
                location: Default::default(),
            })),
        ];
        let session = Session {
            title: TextContent::from_string(title, None),
            marker: None,
            children: SessionContainer::from_typed(content),
            annotations: vec![],
            location: Default::default(),
        };

        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Session(session),
        ]);

        let serialized = export(&doc).expect("Serialization should not fail");
        let parsed = parse_document(&serialized).expect("Parsing should not fail");

        let e_items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let a_items: Vec<&ContentItem> = parsed.root.children.iter().collect();
        assert_ast_equiv(&e_items, &a_items, &serialized);
    }
}

// Deep nesting: session > definition > verbatim

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn test_round_trip_session_definition_verbatim(
        session_title in "[a-zA-Z0-9]+",
        def_subject in "[A-Z][a-zA-Z0-9 ]*".prop_map(|s| s.trim_end().to_string()),
        verb in verbatim_strategy(),
    ) {
        let def = Definition::new(
            TextContent::from_string(def_subject, None),
            vec![ContentElement::VerbatimBlock(Box::new(verb))],
        );
        let content = vec![
            SessionContent::Element(ContentElement::Definition(def)),
            SessionContent::Element(ContentElement::BlankLineGroup(BlankLineGroup {
                count: 1,
                source_tokens: vec![],
                location: Default::default(),
            })),
        ];
        let session = Session {
            title: TextContent::from_string(session_title, None),
            marker: None,
            children: SessionContainer::from_typed(content),
            annotations: vec![],
            location: Default::default(),
        };

        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Session(session),
        ]);

        let serialized = export(&doc).expect("Serialization should not fail");
        let parsed = parse_document(&serialized).expect("Parsing should not fail");

        let e_items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let a_items: Vec<&ContentItem> = parsed.root.children.iter().collect();
        assert_ast_equiv(&e_items, &a_items, &serialized);
    }

    #[test]
    fn test_round_trip_nested_session_verbatim(
        outer_title in "[a-zA-Z0-9]+",
        inner_title in "[a-zA-Z0-9]+",
        verb in verbatim_strategy(),
    ) {
        let inner_content = vec![
            SessionContent::Element(ContentElement::VerbatimBlock(Box::new(verb))),
            SessionContent::Element(ContentElement::BlankLineGroup(BlankLineGroup {
                count: 1,
                source_tokens: vec![],
                location: Default::default(),
            })),
        ];
        let inner_session = Session {
            title: TextContent::from_string(inner_title, None),
            marker: None,
            children: SessionContainer::from_typed(inner_content),
            annotations: vec![],
            location: Default::default(),
        };

        let outer_content = vec![
            SessionContent::Session(inner_session),
        ];
        let outer_session = Session {
            title: TextContent::from_string(outer_title, None),
            marker: None,
            children: SessionContainer::from_typed(outer_content),
            annotations: vec![],
            location: Default::default(),
        };

        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Session(outer_session),
        ]);

        let serialized = export(&doc).expect("Serialization should not fail");
        let parsed = parse_document(&serialized).expect("Parsing should not fail");

        let e_items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let a_items: Vec<&ContentItem> = parsed.root.children.iter().collect();
        assert_ast_equiv(&e_items, &a_items, &serialized);
    }
}

// -----------------------------------------------------------------------------
// Equivalence Checks
// -----------------------------------------------------------------------------

fn assert_ast_equiv(expected: &[&ContentItem], actual: &[&ContentItem], lex_string: &str) {
    // Filter out synthesized blank line groups
    let filtered_expected: Vec<&ContentItem> = expected
        .iter()
        .filter(|&&item| !matches!(item, ContentItem::BlankLineGroup(_)))
        .copied()
        .collect();

    let filtered_actual: Vec<&ContentItem> = actual
        .iter()
        .filter(|&&item| !matches!(item, ContentItem::BlankLineGroup(_)))
        .copied()
        .collect();

    if filtered_expected.len() != filtered_actual.len() {
        println!("EXPECTED ITEMS: {filtered_expected:#?}");
        println!("ACTUAL ITEMS: {filtered_actual:#?}");
        println!("FAILING LEX STRING:\n======\n{lex_string}\n======");
        assert_eq!(
            filtered_expected.len(),
            filtered_actual.len(),
            "AST item counts do not match"
        );
    }

    for (exp, act) in filtered_expected.iter().zip(filtered_actual.iter()) {
        match (*exp, *act) {
            (ContentItem::Paragraph(e_p), ContentItem::Paragraph(a_p)) => {
                assert_eq!(e_p.text(), a_p.text(), "Paragraph text mismatch");
            }
            (ContentItem::List(e_l), ContentItem::List(a_l)) => {
                let e_items: Vec<&ContentItem> = e_l.items.iter().collect();
                let a_items: Vec<&ContentItem> = a_l.items.iter().collect();
                assert_ast_equiv(&e_items, &a_items, lex_string);
            }
            (ContentItem::ListItem(e_li), ContentItem::ListItem(a_li)) => {
                let e_text = e_li.text.first().map(|t| t.as_string()).unwrap_or("");
                let a_text = a_li.text.first().map(|t| t.as_string()).unwrap_or("");
                assert_eq!(e_text, a_text, "ListItem text mismatch");
                let e_children: Vec<&ContentItem> = e_li.children.iter().collect();
                let a_children: Vec<&ContentItem> = a_li.children.iter().collect();
                assert_ast_equiv(&e_children, &a_children, lex_string);
            }
            (ContentItem::Session(e_s), ContentItem::Session(a_s)) => {
                assert_eq!(
                    e_s.title.as_string(),
                    a_s.title.as_string(),
                    "Session title mismatch"
                );
                let e_children: Vec<&ContentItem> = e_s.children.iter().collect();
                let a_children: Vec<&ContentItem> = a_s.children.iter().collect();
                assert_ast_equiv(&e_children, &a_children, lex_string);
            }
            (ContentItem::Definition(e_d), ContentItem::Definition(a_d)) => {
                assert_eq!(
                    e_d.subject.as_string(),
                    a_d.subject.as_string(),
                    "Definition subject mismatch"
                );
                let e_children: Vec<&ContentItem> = e_d.children.iter().collect();
                let a_children: Vec<&ContentItem> = a_d.children.iter().collect();
                assert_ast_equiv(&e_children, &a_children, lex_string);
            }
            (ContentItem::VerbatimBlock(e_v), ContentItem::VerbatimBlock(a_v)) => {
                assert_eq!(
                    e_v.subject.as_string(),
                    a_v.subject.as_string(),
                    "Verbatim subject mismatch"
                );
                assert_eq!(
                    e_v.closing_data.label.value,
                    a_v.closing_data.label.value,
                    "Verbatim closing label mismatch"
                );
                // Compare verbatim line content
                let e_lines: Vec<&str> = e_v
                    .children
                    .iter()
                    .filter_map(|c| {
                        if let ContentItem::VerbatimLine(vl) = c {
                            Some(vl.content.as_string())
                        } else {
                            None
                        }
                    })
                    .collect();
                let a_lines: Vec<&str> = a_v
                    .children
                    .iter()
                    .filter_map(|c| {
                        if let ContentItem::VerbatimLine(vl) = c {
                            Some(vl.content.as_string())
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(e_lines, a_lines, "Verbatim content mismatch");
            }
            (ContentItem::Annotation(e_a), ContentItem::Annotation(a_a)) => {
                assert_eq!(
                    e_a.data.label.value, a_a.data.label.value,
                    "Annotation label mismatch"
                );
                assert_eq!(
                    e_a.data.parameters.len(),
                    a_a.data.parameters.len(),
                    "Annotation parameter count mismatch"
                );
                for (ep, ap) in e_a
                    .data
                    .parameters
                    .iter()
                    .zip(a_a.data.parameters.iter())
                {
                    assert_eq!(ep.key, ap.key, "Annotation parameter key mismatch");
                    assert_eq!(ep.value, ap.value, "Annotation parameter value mismatch");
                }
                let e_children: Vec<&ContentItem> = e_a.children.iter().collect();
                let a_children: Vec<&ContentItem> = a_a.children.iter().collect();
                assert_ast_equiv(&e_children, &a_children, lex_string);
            }
            _ => panic!(
                "AST node types do not match or are not supported.\nExpected: {exp}\nActual: {act}\nLex:\n{lex_string}"
            ),
        }
    }
}

// -----------------------------------------------------------------------------
// Extended Marker Lists
// -----------------------------------------------------------------------------

fn numbered_list_strategy() -> impl Strategy<Value = List> {
    prop::collection::vec("[a-zA-Z0-9]+( [a-zA-Z0-9]+)*", 2..5).prop_map(|texts| {
        let mut list_container = ListContainer::empty();
        for (i, text) in texts.iter().enumerate() {
            list_container.push(ContentItem::ListItem(ListItem {
                marker: TextContent::from_string(format!("{}.", i + 1), None),
                text: vec![TextContent::from_string(format!("{text}\n"), None)],
                children: GeneralContainer::empty(),
                annotations: vec![],
                location: Default::default(),
            }));
        }
        let mut list = List::new(vec![]);
        list.items = list_container;
        list.marker = Some(SequenceMarker {
            raw_text: TextContent::from_string("1.".to_string(), None),
            style: DecorationStyle::Numerical,
            separator: Separator::Period,
            form: Form::Short,
            location: Default::default(),
        });
        list
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn test_round_trip_numbered_list(list in numbered_list_strategy()) {
        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Element(ContentElement::List(list)),
        ]);

        let serialized = export(&doc).expect("Serialization should not fail");
        let parsed = parse_document(&serialized).expect("Parsing should not fail");

        let e_items: Vec<&ContentItem> = doc.root.children.iter().collect();
        let a_items: Vec<&ContentItem> = parsed.root.children.iter().collect();
        assert_ast_equiv(&e_items, &a_items, &serialized);
    }

    #[test]
    fn test_format_idempotency_numbered_list(list in numbered_list_strategy()) {
        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Element(ContentElement::List(list)),
        ]);

        let formatted_1 = serialize_to_lex(&doc).expect("First serialization should not fail");
        let parsed = parse_document(&formatted_1).expect("Parsing formatted output should not fail");
        let formatted_2 = serialize_to_lex(&parsed).expect("Second serialization should not fail");
        assert_eq!(
            formatted_1, formatted_2,
            "Formatting is not idempotent!\nFirst:\n{formatted_1}\nSecond:\n{formatted_2}"
        );
    }
}

// IR round-trip tests: Lex source → IR → Lex AST → serialize → parse → compare

#[test]
fn test_ir_round_trip_simple_list() {
    let source = "- First item\n- Second item\n";
    let doc = parse_document(source).expect("parse");
    let ir = lex_babel::to_ir(&doc);
    let back = lex_babel::from_ir(&ir);
    let reserialized = export(&back).expect("export");
    let reparsed = parse_document(&reserialized).expect("reparse");
    let final_text = export(&reparsed).expect("final export");
    assert_eq!(reserialized, final_text, "IR round-trip not idempotent");
}

#[test]
fn test_ir_round_trip_numbered_list() {
    let source = "1. First item\n2. Second item\n3. Third item\n";
    let doc = parse_document(source).expect("parse");
    let ir = lex_babel::to_ir(&doc);
    let back = lex_babel::from_ir(&ir);
    let reserialized = export(&back).expect("export");
    let reparsed = parse_document(&reserialized).expect("reparse");
    let final_text = export(&reparsed).expect("final export");
    assert_eq!(reserialized, final_text, "IR round-trip not idempotent");
}

#[test]
fn test_ir_preserves_list_form() {
    use lex_babel::ir::nodes::ListForm;

    // Extended form flat list
    let source = "1.2.3 Item one\n1.2.4 Item two\n";
    let doc = parse_document(source).expect("parse");
    let ir = lex_babel::to_ir(&doc);

    // Check that the IR preserves the form
    fn find_list_form(node: &lex_babel::ir::nodes::DocNode) -> Option<ListForm> {
        match node {
            lex_babel::ir::nodes::DocNode::List(list) => Some(list.form),
            lex_babel::ir::nodes::DocNode::Heading(h) => h.children.iter().find_map(find_list_form),
            lex_babel::ir::nodes::DocNode::Document(d) => {
                d.children.iter().find_map(find_list_form)
            }
            _ => None,
        }
    }

    let doc_node = lex_babel::ir::nodes::DocNode::Document(ir);
    let form = find_list_form(&doc_node);
    assert_eq!(
        form,
        Some(ListForm::Extended),
        "IR should preserve Form::Extended"
    );
}

// -----------------------------------------------------------------------------
// Formatting Idempotency (Priority 4)
// -----------------------------------------------------------------------------
// Property: format(parse(format(ast))) == format(ast)
// If we serialize an AST to lex, then parse and re-serialize, the text should
// be identical. This ensures the formatter is idempotent.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn test_format_idempotency_sessions(ast in document_strategy()) {
        let formatted_1 = serialize_to_lex(&ast).expect("First serialization should not fail");
        let parsed = parse_document(&formatted_1).expect("Parsing formatted output should not fail");
        let formatted_2 = serialize_to_lex(&parsed).expect("Second serialization should not fail");
        assert_eq!(
            formatted_1, formatted_2,
            "Formatting is not idempotent!\nFirst:\n{formatted_1}\nSecond:\n{formatted_2}"
        );
    }

    #[test]
    fn test_format_idempotency_nested_sessions(ast in document_with_nested_sessions_strategy()) {
        let formatted_1 = serialize_to_lex(&ast).expect("First serialization should not fail");
        let parsed = parse_document(&formatted_1).expect("Parsing formatted output should not fail");
        let formatted_2 = serialize_to_lex(&parsed).expect("Second serialization should not fail");
        assert_eq!(
            formatted_1, formatted_2,
            "Formatting is not idempotent!\nFirst:\n{formatted_1}\nSecond:\n{formatted_2}"
        );
    }

    #[test]
    fn test_format_idempotency_definitions(def in definition_strategy()) {
        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Element(ContentElement::Definition(def)),
        ]);

        let formatted_1 = serialize_to_lex(&doc).expect("First serialization should not fail");
        let parsed = parse_document(&formatted_1).expect("Parsing formatted output should not fail");
        let formatted_2 = serialize_to_lex(&parsed).expect("Second serialization should not fail");
        assert_eq!(
            formatted_1, formatted_2,
            "Formatting is not idempotent!\nFirst:\n{formatted_1}\nSecond:\n{formatted_2}"
        );
    }

    #[test]
    fn test_format_idempotency_verbatim(verb in verbatim_strategy()) {
        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(vec![
            SessionContent::Element(ContentElement::VerbatimBlock(Box::new(verb))),
        ]);

        let formatted_1 = serialize_to_lex(&doc).expect("First serialization should not fail");
        let parsed = parse_document(&formatted_1).expect("Parsing formatted output should not fail");
        let formatted_2 = serialize_to_lex(&parsed).expect("Second serialization should not fail");
        assert_eq!(
            formatted_1, formatted_2,
            "Formatting is not idempotent!\nFirst:\n{formatted_1}\nSecond:\n{formatted_2}"
        );
    }
}

// -----------------------------------------------------------------------------
// Reader-shaped faithfulness (lex#784)
// -----------------------------------------------------------------------------
//
// The regression guard for the whole class of "reader's document falls apart on
// serialize" bugs. A *reader-shaped* document is one a foreign-format Reader
// (Markdown, RFC-XML, …) actually produces: sibling blocks carrying NO
// `BlankLineGroup` separators — blank lines are a Lex serialization concern the
// Reader never emits. With the separation matrix landed (#782/#783) the
// serializer now supplies every structural blank, so a reader-shaped AST is a
// valid input and must survive serialize→reparse *Skeleton-equal* (the
// Faithfulness invariant, `canon`; CONTEXT.md).
//
// The generator draws only the block kinds that round-trip faithfully as bare
// siblings and excludes the adjacencies the parser provably cannot separate yet
// — mirrored exactly from `tests/lex_separation/matrix.rs`:
//   - A Definition immediately before a closer-led block (Verbatim/Table/
//     Annotation) is hijacked: the verbatim/table matcher re-anchors the next
//     `:: label ::` closer onto the definition's `subject:` line, swallowing the
//     pair. No blank count fixes it (matrix.rs::is_known_hijack). We generate
//     Verbatim (not Table/Annotation) as a closer-led sibling, so the only such
//     adjacency reachable here is Definition→Verbatim, which the generator
//     rejects.
//   - Bare Annotation siblings do not round-trip as siblings (the parser
//     re-attaches or hoists a floating annotation), so Annotation is never
//     generated as a top-level block.
//   - Ragged tables normalize on serialize (lex#792) and top-level Tables would
//     otherwise stress an orthogonal axis; Table is out of the reader-content set
//     this slice targets (paragraphs, lists, sessions, definitions, verbatim).
//
// Each kind we DO generate is backed by a passing `matrix.rs::faithful_*` example
// proving that shape is faithful; this proptest generalizes them over random
// content and arbitrary sibling orderings.

/// The block kinds the reader-shaped generator emits, tagged so the proptest can
/// reject the documented Definition→Verbatim hijack adjacency by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReaderBlockKind {
    Paragraph,
    List,
    Session,
    Definition,
    Verbatim,
}

/// A reader-shaped nested (unordered) list: a parent item whose body is a nested
/// sub-list, plus a plain sibling item — NO `BlankLineGroup` anywhere. Mirrors
/// the shape `matrix.rs::faithful_nested_lists` proves faithful (item text
/// followed directly by a child list).
fn nested_list_strategy() -> impl Strategy<Value = List> {
    (
        "[a-zA-Z0-9]+( [a-zA-Z0-9]+)*",
        list_strategy(),
        "[a-zA-Z0-9]+( [a-zA-Z0-9]+)*",
    )
        .prop_map(|(parent_text, inner, sibling_text)| {
            let parent = ListItem::with_content(
                "-".to_string(),
                parent_text,
                vec![ContentElement::List(inner)],
            );
            let sibling = ListItem::new("-".to_string(), format!("{sibling_text}\n"));
            let mut list = List::new(vec![parent, sibling]);
            list.marker = Some(SequenceMarker {
                raw_text: TextContent::from_string("-".to_string(), None),
                style: DecorationStyle::Plain,
                separator: Separator::Period,
                form: Form::Short,
                location: Default::default(),
            });
            list
        })
}

/// A single reader-shaped top-level block, paired with its kind tag. Covers the
/// reader-content set: paragraphs, unordered/ordered/nested lists, sessions,
/// definitions, and verbatim blocks. No branch inserts a `BlankLineGroup`.
fn reader_block_strategy() -> impl Strategy<Value = (ReaderBlockKind, SessionContent)> {
    prop_oneof![
        paragraph_strategy().prop_map(|p| (
            ReaderBlockKind::Paragraph,
            SessionContent::Element(ContentElement::Paragraph(p)),
        )),
        list_strategy().prop_map(|l| (
            ReaderBlockKind::List,
            SessionContent::Element(ContentElement::List(l)),
        )),
        numbered_list_strategy().prop_map(|l| (
            ReaderBlockKind::List,
            SessionContent::Element(ContentElement::List(l)),
        )),
        nested_list_strategy().prop_map(|l| (
            ReaderBlockKind::List,
            SessionContent::Element(ContentElement::List(l)),
        )),
        session_strategy().prop_map(|s| (ReaderBlockKind::Session, SessionContent::Session(s),)),
        definition_strategy().prop_map(|d| (
            ReaderBlockKind::Definition,
            SessionContent::Element(ContentElement::Definition(d)),
        )),
        verbatim_strategy().prop_map(|v| (
            ReaderBlockKind::Verbatim,
            SessionContent::Element(ContentElement::VerbatimBlock(Box::new(v))),
        )),
    ]
}

/// A two-line lead paragraph. The document-title steal (ADR-0002 / #783) promotes
/// only a *single-line* first paragraph, so a two-line lead is never stolen and
/// neutralizes the title boundary — the generated blocks are then measured as
/// plain siblings. Mirrors `matrix.rs::lead`.
fn reader_lead() -> SessionContent {
    SessionContent::Element(ContentElement::Paragraph(Paragraph::new(vec![
        ContentItem::TextLine(TextLine::new(TextContent::from_string(
            "Lead line one".to_string(),
            None,
        ))),
        ContentItem::TextLine(TextLine::new(TextContent::from_string(
            "Lead line two".to_string(),
            None,
        ))),
    ])))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// FAITHFULNESS over reader-shaped documents (lex#784): an arbitrary sequence
    /// of sibling blocks built with NO pre-inserted `BlankLineGroup`s serializes to
    /// Lex and re-parses to the same Skeleton (`canon`). This is the property-level
    /// statement of the invariant the removed pre-insertion workaround used to hide.
    ///
    /// The only unfaithful adjacency the generator can produce — a Definition
    /// immediately before a Verbatim (matrix.rs::is_known_hijack) — is rejected, so
    /// the test exercises the real reader-shaped-separation failure mode without
    /// asserting an adjacency the parser provably cannot round-trip yet.
    #[test]
    fn reader_shaped_document_is_faithful(
        blocks in prop::collection::vec(reader_block_strategy(), 1..7),
    ) {
        // Reject the documented Definition → Verbatim hijack (the verbatim closer
        // re-anchors the definition subject). Mirrors matrix.rs::is_known_hijack;
        // the other two hijack partners (Table/Annotation) are never generated.
        // `prop_assume!` discards the sample so proptest regenerates and still runs
        // the full 256 accepted cases — rejected inputs are not counted as passes.
        for pair in blocks.windows(2) {
            prop_assume!(
                !(pair[0].0 == ReaderBlockKind::Definition
                    && pair[1].0 == ReaderBlockKind::Verbatim)
            );
        }

        // A title-neutralizing lead, then the generated blocks — all reader-shaped,
        // zero BlankLineGroups.
        let mut children = vec![reader_lead()];
        children.extend(blocks.into_iter().map(|(_, sc)| sc));

        let mut doc = Document::new();
        doc.root.children = SessionContainer::from_typed(children);

        let serialized = export(&doc).expect("Serialization should not fail");
        let reparsed = parse_document(&serialized)
            .unwrap_or_else(|e| panic!("serialized Lex did not re-parse: {e}\n{serialized}"));

        let want = canon(&doc);
        let got = canon(&reparsed);
        prop_assert_eq!(
            want, got,
            "reader-shaped document not faithful (canon mismatch)\n--- serialized Lex ---\n{}",
            serialized
        );
    }
}
