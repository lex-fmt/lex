//! Round-trip tests for the wire codec.
//!
//! Strategy: for each shape currently covered, build a synthetic
//! lex-core AST node, run through `to_wire_node`, JSON-serialise +
//! deserialise (proves the wire form actually round-trips through the
//! protocol envelope), then convert back via `from_wire_node`. The
//! recovered AST should be structurally equivalent to the input.
//!
//! Corpus-driven round-trip across every `.lex` fixture lands in a
//! follow-up commit on the same PR once Session/Definition/List/Table/
//! Verbatim coverage is complete in `to_wire.rs` / `from_wire.rs`.

use super::from_wire::{from_wire_node, from_wire_subtree};
use super::to_wire::to_wire_node;
use crate::lex::ast::elements::annotation::Annotation;
use crate::lex::ast::elements::blank_line_group::BlankLineGroup;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::label::Label;
use crate::lex::ast::elements::paragraph::{Paragraph, TextLine};
use crate::lex::ast::range::{Position, Range};
use crate::lex::ast::TextContent;
use lex_extension::wire::WireNode;

fn r(s: usize, e: usize) -> Range {
    Range::new(0..0, Position::new(s, 0), Position::new(e, 0))
}

fn json_round_trip(node: &WireNode) -> WireNode {
    let s = serde_json::to_string(node).expect("wire node serialises");
    serde_json::from_str(&s).expect("wire node deserialises")
}

#[test]
fn paragraph_round_trips() {
    let p = Paragraph::new(vec![ContentItem::TextLine(TextLine::new(
        TextContent::from_string("hello world".into(), None),
    ))]);
    let item = ContentItem::Paragraph(p);
    let wire = to_wire_node(&item);
    let wire_after = json_round_trip(&wire);
    let back = from_wire_node(&wire_after).expect("from_wire ok");
    assert_eq!(back.len(), 1);
    match &back[0] {
        ContentItem::Paragraph(p) => {
            // The single text line should carry the original raw string.
            assert_eq!(p.lines.len(), 1);
            if let ContentItem::TextLine(line) = &p.lines[0] {
                assert_eq!(line.content.as_string(), "hello world");
            } else {
                panic!("expected TextLine inside Paragraph");
            }
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn blank_round_trips() {
    let mut blg = BlankLineGroup::new(1, Vec::new());
    blg.location = r(0, 1);
    let item = ContentItem::BlankLineGroup(blg);
    let wire = to_wire_node(&item);
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    assert!(matches!(back[0], ContentItem::BlankLineGroup(_)));
}

#[test]
fn marker_annotation_round_trips() {
    let a = Annotation::marker(Label::new("note".into()));
    let item = ContentItem::Annotation(a);
    let wire = to_wire_node(&item);
    // Marker annotation has no body — wire carries `body: null`.
    if let WireNode::Annotation { body, .. } = &wire {
        assert!(body.is_null(), "marker annotation must serialise body=null");
    } else {
        panic!("expected Annotation");
    }
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::Annotation(a) => {
            assert_eq!(a.data.label.value, "note");
            assert_eq!(a.data.parameters.len(), 0);
            assert_eq!(a.children.iter().count(), 0);
        }
        other => panic!("expected Annotation, got {other:?}"),
    }
}

#[test]
fn parameterised_annotation_round_trips() {
    use crate::lex::ast::elements::parameter::Parameter;
    let mut a = Annotation::marker(Label::new("acme.task".into()));
    a.data.parameters.push(Parameter {
        key: "id".into(),
        value: "ACME-1234".into(),
        location: r(0, 0),
    });
    a.data.parameters.push(Parameter {
        key: "priority".into(),
        value: "high".into(),
        location: r(0, 0),
    });
    let item = ContentItem::Annotation(a);
    let wire = to_wire_node(&item);
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::Annotation(a) => {
            assert_eq!(a.data.label.value, "acme.task");
            // Parameter ordering may not be preserved (JSON object key
            // ordering is implementation-defined); compare as a sorted
            // key/value set instead.
            let mut got: Vec<(String, String)> = a
                .data
                .parameters
                .iter()
                .map(|p| (p.key.clone(), p.value.clone()))
                .collect();
            got.sort();
            assert_eq!(
                got,
                vec![
                    ("id".to_string(), "ACME-1234".to_string()),
                    ("priority".to_string(), "high".to_string()),
                ]
            );
        }
        other => panic!("expected Annotation, got {other:?}"),
    }
}

#[test]
fn unsupported_kind_in_internal_namespace_surfaces_as_error() {
    // The reverse codec must reject a `Verbatim` whose label uses the
    // reserved `lex.internal.unsupported.*` prefix — that prefix is
    // how the forward codec used to flag uncovered variants. Now that
    // every variant is wired, the prefix should never show up in
    // codec output, but the reverse-side guard remains so a malformed
    // wire input still surfaces an error instead of silently
    // round-tripping into an empty Verbatim.
    use super::error::FromWireError;
    let wire = lex_extension::wire::WireNode::Verbatim {
        range: lex_extension::wire::Range::new(
            lex_extension::wire::Position::new(0, 0),
            lex_extension::wire::Position::new(0, 0),
        ),
        origin: None,
        label: "lex.internal.unsupported.session".into(),
        params: serde_json::json!({}),
        body_text: String::new(),
        subject: String::new(),
        mode: "inflow".into(),
    };
    let result = from_wire_node(&wire);
    assert!(matches!(
        result,
        Err(FromWireError::UnsupportedKind { ref kind }) if kind == "session"
    ));
}

#[test]
fn from_wire_subtree_handles_empty() {
    let items = from_wire_subtree(&[]).expect("ok");
    assert!(items.is_empty());
}

#[test]
fn multi_line_paragraph_round_trips() {
    // A paragraph with two TextLines should preserve both lines
    // through the wire round-trip — without explicit `\n` separators
    // in the forward codec, "Hello" + "World" would become
    // "HelloWorld".
    let p = Paragraph::new(vec![
        ContentItem::TextLine(TextLine::new(TextContent::from_string(
            "Hello".into(),
            None,
        ))),
        ContentItem::TextLine(TextLine::new(TextContent::from_string(
            "World".into(),
            None,
        ))),
    ]);
    let item = ContentItem::Paragraph(p);
    let wire = to_wire_node(&item);
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::Paragraph(p) => {
            assert_eq!(p.lines.len(), 2, "expected two TextLines after round-trip");
            let texts: Vec<&str> = p
                .lines
                .iter()
                .filter_map(|li| match li {
                    ContentItem::TextLine(line) => Some(line.content.as_string()),
                    _ => None,
                })
                .collect();
            assert_eq!(texts, vec!["Hello", "World"]);
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn string_body_annotation_becomes_paragraph_child() {
    // A wire annotation with a string body (the single-line form,
    // e.g. `:: note :: hello`) should round-trip into an Annotation
    // whose children contain a single Paragraph carrying the text.
    let wire = lex_extension::wire::WireNode::Annotation {
        range: lex_extension::wire::Range::new(
            lex_extension::wire::Position::new(0, 0),
            lex_extension::wire::Position::new(0, 18),
        ),
        origin: None,
        label: "note".into(),
        params: serde_json::json!({}),
        body: serde_json::Value::String("hello".into()),
    };
    let back = from_wire_node(&wire).expect("ok");
    match &back[0] {
        ContentItem::Annotation(a) => {
            let children: Vec<&ContentItem> = a.children.iter().collect();
            assert_eq!(children.len(), 1, "string body should produce one child");
            match children[0] {
                ContentItem::Paragraph(p) => {
                    let line = match &p.lines[0] {
                        ContentItem::TextLine(line) => line,
                        _ => panic!("expected TextLine"),
                    };
                    assert_eq!(line.content.as_string(), "hello");
                }
                other => panic!("expected Paragraph child, got {other:?}"),
            }
        }
        other => panic!("expected Annotation, got {other:?}"),
    }
}

#[test]
fn blank_count_from_range_span() {
    // A wire blank node whose range spans 3 lines should reverse
    // into a BlankLineGroup with count=3, not the prior fixed
    // count=1.
    let wire = lex_extension::wire::WireNode::Blank {
        range: lex_extension::wire::Range::new(
            lex_extension::wire::Position::new(5, 0),
            lex_extension::wire::Position::new(8, 0),
        ),
        origin: None,
    };
    let back = from_wire_node(&wire).expect("ok");
    match &back[0] {
        ContentItem::BlankLineGroup(blg) => assert_eq!(blg.count, 3),
        other => panic!("expected BlankLineGroup, got {other:?}"),
    }
}

#[test]
fn blank_count_clamps_to_one_for_collapsed_range() {
    let wire = lex_extension::wire::WireNode::Blank {
        range: lex_extension::wire::Range::new(
            lex_extension::wire::Position::new(0, 0),
            lex_extension::wire::Position::new(0, 0),
        ),
        origin: None,
    };
    let back = from_wire_node(&wire).expect("ok");
    match &back[0] {
        ContentItem::BlankLineGroup(blg) => assert_eq!(blg.count, 1),
        _ => panic!("expected BlankLineGroup"),
    }
}

// ============================================================================
// Per-variant round-trip tests for the variants newly wired in PR 3c.
// ============================================================================

#[test]
fn session_round_trips() {
    use crate::lex::ast::elements::session::Session;
    let mut s = Session::with_title("Intro".into());
    s.children
        .as_mut_vec()
        .push(ContentItem::Paragraph(Paragraph::from_line(
            "First paragraph".into(),
        )));
    let item = ContentItem::Session(s);
    let wire = to_wire_node(&item);
    if let WireNode::Session { ref title, .. } = wire {
        assert_eq!(title, "Intro");
    } else {
        panic!("expected WireNode::Session, got {wire:?}");
    }
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::Session(s) => {
            assert_eq!(s.title.as_string(), "Intro");
            // The single paragraph child should have round-tripped.
            let mut found_para = false;
            for child in s.children.iter() {
                if let ContentItem::Paragraph(p) = child {
                    if let Some(ContentItem::TextLine(line)) = p.lines.first() {
                        if line.content.as_string() == "First paragraph" {
                            found_para = true;
                        }
                    }
                }
            }
            assert!(found_para, "session paragraph child must round-trip");
        }
        other => panic!("expected Session, got {other:?}"),
    }
}

#[test]
fn session_marker_round_trips_as_string() {
    use crate::lex::ast::elements::sequence_marker::SequenceMarker;
    use crate::lex::ast::elements::session::Session;
    let mut s = Session::with_title("Chapter Two".into());
    s.marker = SequenceMarker::parse("2.", None);
    let item = ContentItem::Session(s);
    let wire = to_wire_node(&item);
    if let WireNode::Session { ref marker, .. } = wire {
        assert_eq!(marker.as_deref(), Some("2."));
    } else {
        panic!("expected WireNode::Session");
    }
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::Session(s) => {
            let m = s.marker.as_ref().expect("marker reconstructed");
            assert_eq!(m.as_str(), "2.");
        }
        other => panic!("expected Session, got {other:?}"),
    }
}

#[test]
fn definition_round_trips() {
    use crate::lex::ast::elements::definition::Definition;
    let mut d = Definition::with_subject("Cache".into());
    d.children
        .as_mut_vec()
        .push(ContentItem::Paragraph(Paragraph::from_line(
            "Temporary storage.".into(),
        )));
    let item = ContentItem::Definition(d);
    let wire = to_wire_node(&item);
    if let WireNode::Definition { ref subject, .. } = wire {
        assert_eq!(subject, "Cache");
    } else {
        panic!("expected WireNode::Definition");
    }
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::Definition(d) => {
            assert_eq!(d.subject.as_string(), "Cache");
            assert!(!d.children.is_empty(), "child paragraph must survive");
        }
        other => panic!("expected Definition, got {other:?}"),
    }
}

#[test]
fn list_round_trips() {
    use crate::lex::ast::elements::list::{List, ListItem};
    use crate::lex::ast::elements::sequence_marker::SequenceMarker;
    let mut list = List::new(vec![
        ListItem::new("-".into(), "Bread".into()),
        ListItem::new("-".into(), "Milk".into()),
    ]);
    list.marker = SequenceMarker::parse("-", None);
    let item = ContentItem::List(list);
    let wire = to_wire_node(&item);
    if let WireNode::List {
        ref marker_style,
        ref items,
        ..
    } = wire
    {
        assert_eq!(marker_style, "dash");
        assert_eq!(items.len(), 2);
    } else {
        panic!("expected WireNode::List");
    }
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::List(l) => {
            assert_eq!(l.items.len(), 2);
            // Second item's text must round-trip.
            let texts: Vec<String> = l
                .items
                .iter()
                .filter_map(|item| match item {
                    ContentItem::ListItem(li) => {
                        li.text.first().map(|tc| tc.as_string().to_string())
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(texts, vec!["Bread".to_string(), "Milk".to_string()]);
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn nested_list_children_round_trip() {
    use crate::lex::ast::elements::list::{List, ListItem};
    use crate::lex::ast::elements::typed_content::ContentElement;
    let nested_list = List::new(vec![
        ListItem::new("-".into(), "child a".into()),
        ListItem::new("-".into(), "child b".into()),
    ]);
    let parent_with_children = ListItem::with_content(
        "-".into(),
        "parent".into(),
        vec![ContentElement::List(nested_list)],
    );
    let other = ListItem::new("-".into(), "sibling".into());
    let list = List::new(vec![parent_with_children, other]);
    let item = ContentItem::List(list);
    let wire = to_wire_node(&item);
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::List(l) => {
            // First item carries a nested list as a child.
            let parent = match l.items.iter().next() {
                Some(ContentItem::ListItem(li)) => li,
                _ => panic!("expected ListItem"),
            };
            let nested = parent.children.iter().find_map(|c| match c {
                ContentItem::List(inner) => Some(inner),
                _ => None,
            });
            assert!(
                nested.is_some(),
                "nested list inside list-item must round-trip"
            );
            assert_eq!(nested.unwrap().items.len(), 2);
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn verbatim_round_trips_label_and_body() {
    use crate::lex::ast::elements::data::Data;
    use crate::lex::ast::elements::label::Label;
    use crate::lex::ast::elements::typed_content::VerbatimContent;
    use crate::lex::ast::elements::verbatim::{Verbatim, VerbatimBlockMode};
    use crate::lex::ast::elements::verbatim_line::VerbatimLine;
    let body_lines = vec![
        VerbatimContent::VerbatimLine(VerbatimLine::new("fn main() {".into())),
        VerbatimContent::VerbatimLine(VerbatimLine::new("    println!(\"hi\");".into())),
        VerbatimContent::VerbatimLine(VerbatimLine::new("}".into())),
    ];
    let v = Verbatim::new(
        TextContent::from_string("Code:".into(), None),
        body_lines,
        Data::new(Label::new("rust".into()), Vec::new()),
        VerbatimBlockMode::Inflow,
    );
    let item = ContentItem::VerbatimBlock(Box::new(v));
    let wire = to_wire_node(&item);
    if let WireNode::Verbatim {
        ref label,
        ref body_text,
        ref subject,
        ref mode,
        ..
    } = wire
    {
        assert_eq!(label, "rust");
        assert_eq!(body_text, "fn main() {\n    println!(\"hi\");\n}");
        assert_eq!(subject, "Code:");
        assert_eq!(mode, "inflow");
    } else {
        panic!("expected WireNode::Verbatim");
    }
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::VerbatimBlock(v) => {
            assert_eq!(v.closing_data.label.value, "rust");
            assert_eq!(
                v.subject.as_string(),
                "Code:",
                "subject must round-trip verbatim"
            );
            assert_eq!(
                v.mode,
                VerbatimBlockMode::Inflow,
                "mode must round-trip verbatim"
            );
            let lines: Vec<String> = v
                .children
                .iter()
                .filter_map(|item| match item {
                    ContentItem::VerbatimLine(vl) => Some(vl.content.as_string().to_string()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                lines,
                vec![
                    "fn main() {".to_string(),
                    "    println!(\"hi\");".to_string(),
                    "}".to_string(),
                ]
            );
        }
        other => panic!("expected VerbatimBlock, got {other:?}"),
    }
}

#[test]
fn verbatim_fullwidth_mode_round_trips() {
    use crate::lex::ast::elements::data::Data;
    use crate::lex::ast::elements::label::Label;
    use crate::lex::ast::elements::verbatim::{Verbatim, VerbatimBlockMode};
    let v = Verbatim::new(
        TextContent::from_string("Wide:".into(), None),
        Vec::new(),
        Data::new(Label::new("text".into()), Vec::new()),
        VerbatimBlockMode::Fullwidth,
    );
    let item = ContentItem::VerbatimBlock(Box::new(v));
    let wire = to_wire_node(&item);
    if let WireNode::Verbatim { ref mode, .. } = wire {
        assert_eq!(mode, "fullwidth");
    } else {
        panic!("expected WireNode::Verbatim");
    }
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::VerbatimBlock(v) => {
            assert_eq!(v.mode, VerbatimBlockMode::Fullwidth);
            assert_eq!(v.subject.as_string(), "Wide:");
        }
        other => panic!("expected VerbatimBlock, got {other:?}"),
    }
}

#[test]
fn standalone_verbatim_line_round_trips_via_empty_label() {
    use crate::lex::ast::elements::verbatim_line::VerbatimLine;
    let item = ContentItem::VerbatimLine(VerbatimLine::new("loose verbatim line".into()));
    let wire = to_wire_node(&item);
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::VerbatimLine(vl) => {
            assert_eq!(vl.content.as_string(), "loose verbatim line");
        }
        other => panic!("expected VerbatimLine, got {other:?}"),
    }
}

#[test]
fn table_with_block_content_cells_surfaces_as_unsupported_kind() {
    // Tables with block-level cell children have no representation
    // in the wire format (`WireTableCell` only carries inlines), so
    // the forward codec must emit an unsupported-kind placeholder
    // rather than silently lose the cell children. The reverse codec
    // surfaces the placeholder as `FromWireError::UnsupportedKind`.
    use super::error::FromWireError;
    use crate::lex::ast::elements::list::{List, ListItem};
    use crate::lex::ast::elements::table::{Table, TableCell, TableRow};
    use crate::lex::ast::elements::typed_content::ContentElement;
    use crate::lex::ast::elements::verbatim::VerbatimBlockMode;

    // Build a table with one body cell whose children contain a
    // nested list.
    let nested_list = List::new(vec![
        ListItem::new("-".into(), "alpha".into()),
        ListItem::new("-".into(), "beta".into()),
    ]);
    let block_cell = TableCell::new(TextContent::from_string("see list".into(), None))
        .with_children(vec![ContentElement::List(nested_list)]);
    let plain_cell = TableCell::new(TextContent::from_string("inline".into(), None));
    let row = TableRow::new(vec![block_cell, plain_cell]);
    let t = Table::new(
        TextContent::from_string("Caption".into(), None),
        Vec::new(),
        vec![row],
        VerbatimBlockMode::Inflow,
    );

    let item = ContentItem::Table(Box::new(t));
    let wire = to_wire_node(&item);
    // Forward emits the placeholder, not a Table.
    if let WireNode::Verbatim { ref label, .. } = wire {
        assert_eq!(label, "lex.internal.unsupported.table_block_cells");
    } else {
        panic!("expected unsupported-kind placeholder, got {wire:?}");
    }
    // Reverse rejects the placeholder.
    let result = from_wire_node(&json_round_trip(&wire));
    assert!(
        matches!(
            result,
            Err(FromWireError::UnsupportedKind { ref kind }) if kind == "table_block_cells"
        ),
        "expected UnsupportedKind, got {result:?}"
    );
}

#[test]
fn table_round_trips_caption_and_rows() {
    use crate::lex::ast::elements::table::{Table, TableCell, TableRow};
    use crate::lex::ast::elements::verbatim::VerbatimBlockMode;
    let header = TableRow::new(vec![
        TableCell::new(TextContent::from_string("Name".into(), None)).with_header(true),
        TableCell::new(TextContent::from_string("Score".into(), None)).with_header(true),
    ]);
    let body = vec![
        TableRow::new(vec![
            TableCell::new(TextContent::from_string("Alice".into(), None)),
            TableCell::new(TextContent::from_string("42".into(), None)),
        ]),
        TableRow::new(vec![
            TableCell::new(TextContent::from_string("Bob".into(), None)),
            TableCell::new(TextContent::from_string("17".into(), None)),
        ]),
    ];
    let t = Table::new(
        TextContent::from_string("Scoreboard".into(), None),
        vec![header],
        body,
        VerbatimBlockMode::Inflow,
    );
    let item = ContentItem::Table(Box::new(t));
    let wire = to_wire_node(&item);
    if let WireNode::Table {
        ref caption,
        header_rows,
        ref rows,
        ..
    } = wire
    {
        assert_eq!(caption, "Scoreboard");
        assert_eq!(header_rows, 1);
        assert_eq!(rows.len(), 3);
    } else {
        panic!("expected WireNode::Table");
    }
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::Table(t) => {
            assert_eq!(t.subject.as_string(), "Scoreboard");
            assert_eq!(t.header_rows.len(), 1);
            assert_eq!(t.body_rows.len(), 2);
            let alice = &t.body_rows[0].cells[0];
            assert_eq!(alice.text(), "Alice");
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

#[test]
fn document_with_session_paragraph_blank_round_trips() {
    use crate::lex::ast::elements::session::Session;
    let mut s = Session::with_title("Intro".into());
    s.children
        .as_mut_vec()
        .push(ContentItem::Paragraph(Paragraph::from_line("Hello".into())));
    s.children
        .as_mut_vec()
        .push(ContentItem::BlankLineGroup(BlankLineGroup::new(
            1,
            Vec::new(),
        )));
    s.children
        .as_mut_vec()
        .push(ContentItem::Paragraph(Paragraph::from_line(
            "Second paragraph".into(),
        )));

    let item = ContentItem::Session(s);
    let wire = to_wire_node(&item);
    let back = from_wire_node(&json_round_trip(&wire)).expect("ok");
    match &back[0] {
        ContentItem::Session(s) => {
            // Session must contain three children in original order.
            let kinds: Vec<&'static str> = s
                .children
                .iter()
                .map(|c| match c {
                    ContentItem::Paragraph(_) => "paragraph",
                    ContentItem::BlankLineGroup(_) => "blank",
                    _ => "other",
                })
                .collect();
            assert_eq!(kinds, vec!["paragraph", "blank", "paragraph"]);
        }
        other => panic!("expected Session, got {other:?}"),
    }
}

/// Corpus-driven round-trip: parse a handful of representative
/// fixtures, run forward+reverse, and verify the codec doesn't surface
/// `UnsupportedKind`. Strict structural-equivalence isn't asserted
/// because the codec preserves block structure but normalises
/// representation-only details (see module docs); the bar here is
/// that real-world include payloads survive a full round trip without
/// content drops.
///
/// Fails loudly if a listed fixture is missing — silent skipping
/// would let the test pass while exercising zero coverage.
#[test]
fn corpus_round_trips_without_unsupported_kinds() {
    use super::to_wire::to_wire_document;
    use crate::lex::loader::DocumentLoader;
    use crate::lex::testing::workspace_path;

    let fixtures = [
        "comms/specs/elements/paragraph.docs/paragraph-01-flat-oneline.lex",
        "comms/specs/elements/list.docs/list-01-flat-simple-dash.lex",
        "comms/specs/elements/session.docs/session-01-flat-simple.lex",
        "comms/specs/elements/definition.docs/definition-01-flat-simple.lex",
    ];

    let mut exercised = 0usize;
    for fixture in fixtures {
        let path = workspace_path(fixture);
        assert!(
            path.exists(),
            "corpus fixture missing: {fixture} (resolved to {})",
            path.display()
        );
        let doc = DocumentLoader::from_path(&path)
            .unwrap_or_else(|e| panic!("could not load {fixture}: {e}"))
            .parse()
            .unwrap_or_else(|e| panic!("could not parse {fixture}: {e}"));
        let wire = to_wire_document(&doc);
        let WireNode::Document { children, .. } = wire else {
            panic!("expected WireNode::Document for {fixture}");
        };
        let back = from_wire_subtree(&children).unwrap_or_else(|e| {
            panic!("from_wire_subtree failed for {fixture}: {e}");
        });
        assert!(
            !back.is_empty() || children.is_empty(),
            "round-trip dropped content for {fixture}"
        );
        exercised += 1;
    }
    assert_eq!(
        exercised,
        fixtures.len(),
        "corpus test must exercise every listed fixture"
    );
}
