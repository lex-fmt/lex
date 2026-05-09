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
fn unsupported_variant_surfaces_as_error() {
    use super::error::FromWireError;
    use crate::lex::ast::elements::session::Session;
    // Session is intentionally not yet wired through the codec; the
    // forward direction emits a placeholder, the reverse surfaces it
    // as UnsupportedKind.
    let s = Session::with_title("title".into());
    let item = ContentItem::Session(s);
    let wire = to_wire_node(&item);
    let result = from_wire_node(&json_round_trip(&wire));
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
