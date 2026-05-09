//! Forward codec: lex-core internal AST → `lex_extension::WireNode`.
//!
//! Total over the AST shapes a parsed lex document can produce. Build-up
//! is incremental: this commit covers the simpler block shapes
//! (`Document`, `Paragraph`, `Annotation`, `BlankLineGroup`); follow-up
//! commits on the same PR extend coverage to `Session`, `Definition`,
//! `List`, `Table`, and `Verbatim`. Variants not yet wired return
//! [`unsupported_node`] which produces a structural placeholder rather
//! than panicking; the placeholder round-trips through the reverse
//! codec to surface the gap as a [`crate::lex::wire::FromWireError`].

use crate::lex::ast::elements::annotation::Annotation;
use crate::lex::ast::elements::blank_line_group::BlankLineGroup;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::paragraph::Paragraph;
use crate::lex::ast::Document;
use lex_extension::wire::WireNode;
use serde_json::{Map, Value};

use super::inline::text_content_to_wire;
use super::range::{origin_string, range_to_wire};

/// Convert a lex-core `Document` to a `WireNode::Document`.
///
/// The document's root session's children become the wire document's
/// children. Document-level title and document-level annotations are
/// **dropped** in this codec (lex.include's splice path discards both
/// for similar reasons — only the body content is interesting).
pub fn to_wire_document(doc: &Document) -> WireNode {
    let children = doc
        .root
        .children
        .iter()
        .map(to_wire_node)
        .collect::<Vec<_>>();
    WireNode::Document {
        range: range_to_wire(&doc.root.location),
        origin: origin_string(&doc.root.location),
        children,
    }
}

/// Convert a single `ContentItem` to a `WireNode`.
///
/// Variants not yet implemented produce an `Unknown`-shaped placeholder
/// (see [`unsupported_node`]); the reverse codec surfaces these as
/// [`super::FromWireError::UnsupportedKind`] so the gap is visible to
/// callers.
pub fn to_wire_node(item: &ContentItem) -> WireNode {
    match item {
        ContentItem::Paragraph(p) => paragraph_to_wire(p),
        ContentItem::BlankLineGroup(blg) => blank_to_wire(blg),
        ContentItem::Annotation(a) => annotation_to_wire(a),

        // Coverage built up in follow-up commits.
        ContentItem::Session(_)
        | ContentItem::Definition(_)
        | ContentItem::List(_)
        | ContentItem::ListItem(_)
        | ContentItem::TextLine(_)
        | ContentItem::VerbatimBlock(_)
        | ContentItem::Table(_)
        | ContentItem::VerbatimLine(_) => unsupported_node(item),
    }
}

fn paragraph_to_wire(p: &Paragraph) -> WireNode {
    // Walk each TextLine, separating lines with explicit newline
    // inlines so the reverse codec can reconstruct the original
    // multi-line shape. Without the separator, "Hello" + "World"
    // would round-trip as "HelloWorld".
    let mut inlines = Vec::new();
    let mut first_line = true;
    for line_item in &p.lines {
        if let ContentItem::TextLine(line) = line_item {
            if !first_line {
                inlines.push(lex_extension::wire::WireInline::Text { text: "\n".into() });
            }
            inlines.extend(text_content_to_wire(&line.content));
            first_line = false;
        }
    }
    WireNode::Paragraph {
        range: range_to_wire(&p.location),
        origin: origin_string(&p.location),
        inlines,
    }
}

fn blank_to_wire(blg: &BlankLineGroup) -> WireNode {
    WireNode::Blank {
        range: range_to_wire(&blg.location),
        origin: origin_string(&blg.location),
    }
}

fn annotation_to_wire(a: &Annotation) -> WireNode {
    let label = a.data.label.value.clone();
    let params = parameters_to_json(&a.data.parameters);
    let body = annotation_body_to_json(a);
    WireNode::Annotation {
        range: range_to_wire(&a.location),
        origin: origin_string(&a.location),
        label,
        params,
        body,
    }
}

fn parameters_to_json(params: &[crate::lex::ast::elements::parameter::Parameter]) -> Value {
    let mut obj = Map::with_capacity(params.len());
    for p in params {
        obj.insert(p.key.clone(), Value::String(p.value.clone()));
    }
    Value::Object(obj)
}

fn annotation_body_to_json(a: &Annotation) -> Value {
    let children = a.children.iter().collect::<Vec<_>>();
    if children.is_empty() {
        return Value::Null;
    }
    let wire_children: Vec<Value> = children
        .iter()
        .map(|c| serde_json::to_value(to_wire_node(c)).expect("wire node serialises"))
        .collect();
    let mut obj = Map::with_capacity(2);
    obj.insert("kind".into(), Value::String("block".into()));
    obj.insert("children".into(), Value::Array(wire_children));
    Value::Object(obj)
}

/// Placeholder for ContentItem variants not yet wired through the
/// codec. Emits a `Verbatim` carrying the lex-core node-type name and
/// no body — the reverse codec (which checks for `lex.internal`-prefix
/// labels) surfaces this as `FromWireError::UnsupportedKind`.
///
/// This keeps the forward codec total without panicking, so partial
/// coverage doesn't break callers that incidentally encounter an
/// unsupported shape during testing.
fn unsupported_node(item: &ContentItem) -> WireNode {
    let location = match item {
        ContentItem::Session(s) => &s.location,
        ContentItem::Definition(d) => &d.location,
        ContentItem::List(l) => &l.location,
        ContentItem::ListItem(li) => &li.location,
        ContentItem::TextLine(tl) => &tl.location,
        ContentItem::VerbatimBlock(v) => &v.location,
        ContentItem::Table(t) => &t.location,
        ContentItem::VerbatimLine(vl) => &vl.location,
        // The other arms are wired through the direct path; reaching
        // this branch via them would be a programmer error.
        _ => unreachable!("unsupported_node only used for unwired variants"),
    };
    let kind_name = item_node_type(item);
    WireNode::Verbatim {
        range: range_to_wire(location),
        origin: origin_string(location),
        label: format!("lex.internal.unsupported.{kind_name}"),
        params: Value::Object(Map::new()),
        body_text: String::new(),
    }
}

fn item_node_type(item: &ContentItem) -> &'static str {
    match item {
        ContentItem::Paragraph(_) => "paragraph",
        ContentItem::Session(_) => "session",
        ContentItem::List(_) => "list",
        ContentItem::ListItem(_) => "list_item",
        ContentItem::TextLine(_) => "text_line",
        ContentItem::Definition(_) => "definition",
        ContentItem::Annotation(_) => "annotation",
        ContentItem::VerbatimBlock(_) => "verbatim_block",
        ContentItem::Table(_) => "table",
        ContentItem::VerbatimLine(_) => "verbatim_line",
        ContentItem::BlankLineGroup(_) => "blank",
    }
}
