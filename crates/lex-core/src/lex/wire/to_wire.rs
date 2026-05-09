//! Forward codec: lex-core internal AST → `lex_extension::WireNode`.
//!
//! Total over the AST shapes a parsed lex document can produce. Every
//! [`ContentItem`] variant is wired through to a structured [`WireNode`];
//! variants that have no direct slot in the wire form are mapped to the
//! closest equivalent (standalone `TextLine` becomes a single-line
//! [`WireNode::Paragraph`]; `VerbatimLine` outside a block becomes a
//! [`WireNode::Verbatim`] with an empty label) so that the reverse codec
//! can reconstruct a structurally equivalent AST.
//!
//! See the module-level docs in [`super`] for the documented losses
//! (annotation slots on inline nodes, document-level title/annotations,
//! verbatim multi-group bodies, byte-offset spans).

use crate::lex::ast::elements::annotation::Annotation;
use crate::lex::ast::elements::blank_line_group::BlankLineGroup;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::definition::Definition;
use crate::lex::ast::elements::list::{List, ListItem};
use crate::lex::ast::elements::paragraph::{Paragraph, TextLine};
use crate::lex::ast::elements::sequence_marker::DecorationStyle;
use crate::lex::ast::elements::session::Session;
use crate::lex::ast::elements::table::{Table, TableCell, TableCellAlignment};
use crate::lex::ast::elements::verbatim::Verbatim;
use crate::lex::ast::elements::verbatim_line::VerbatimLine;
use crate::lex::ast::Document;
use lex_extension::wire::{
    WireFootnote, WireInline, WireListItem, WireNode, WireRow, WireTableCell,
};
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
pub fn to_wire_node(item: &ContentItem) -> WireNode {
    match item {
        ContentItem::Paragraph(p) => paragraph_to_wire(p),
        ContentItem::BlankLineGroup(blg) => blank_to_wire(blg),
        ContentItem::Annotation(a) => annotation_to_wire(a),
        ContentItem::Session(s) => session_to_wire(s),
        ContentItem::Definition(d) => definition_to_wire(d),
        ContentItem::List(l) => list_to_wire(l),
        ContentItem::ListItem(li) => list_item_standalone_to_wire(li),
        ContentItem::Table(t) => table_to_wire(t),
        ContentItem::VerbatimBlock(v) => verbatim_to_wire(v),
        ContentItem::VerbatimLine(vl) => verbatim_line_standalone_to_wire(vl),
        ContentItem::TextLine(tl) => text_line_standalone_to_wire(tl),
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
                inlines.push(WireInline::Text { text: "\n".into() });
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

fn session_to_wire(s: &Session) -> WireNode {
    let children = s.children.iter().map(to_wire_node).collect::<Vec<_>>();
    WireNode::Session {
        range: range_to_wire(&s.location),
        origin: origin_string(&s.location),
        title: s.title_text().to_string(),
        marker: s.marker.as_ref().map(|m| m.as_str().to_string()),
        children,
    }
}

fn definition_to_wire(d: &Definition) -> WireNode {
    let children = d.children.iter().map(to_wire_node).collect::<Vec<_>>();
    WireNode::Definition {
        range: range_to_wire(&d.location),
        origin: origin_string(&d.location),
        subject: d.subject.as_string().to_string(),
        children,
    }
}

fn list_to_wire(l: &List) -> WireNode {
    let marker_style = l
        .marker
        .as_ref()
        .map(|m| decoration_style_name(m.style))
        .unwrap_or("dash")
        .to_string();
    let items = l
        .items
        .iter()
        .filter_map(|item| match item {
            ContentItem::ListItem(li) => Some(list_item_to_wire(li)),
            _ => None,
        })
        .collect();
    WireNode::List {
        range: range_to_wire(&l.location),
        origin: origin_string(&l.location),
        marker_style,
        items,
    }
}

fn list_item_to_wire(li: &ListItem) -> WireListItem {
    // Concatenate every TextContent in `text` into a single inline run,
    // interleaving `\n` separators between continuation lines so a
    // multi-line list-item body round-trips. The reverse codec splits
    // the joined run back into its components.
    let mut inlines = Vec::new();
    for (i, tc) in li.text.iter().enumerate() {
        if i > 0 {
            inlines.push(WireInline::Text { text: "\n".into() });
        }
        inlines.extend(text_content_to_wire(tc));
    }
    let children = li.children.iter().map(to_wire_node).collect();
    WireListItem {
        range: range_to_wire(&li.location),
        inlines,
        children,
    }
}

/// Wire form for a `ListItem` that appears outside a `List` (which the
/// parser does not produce, but the codec must still handle to stay
/// total). Wraps the item as a singleton list with a default marker so
/// the reverse codec produces a valid `List` rather than a malformed
/// node.
fn list_item_standalone_to_wire(li: &ListItem) -> WireNode {
    WireNode::List {
        range: range_to_wire(&li.location),
        origin: origin_string(&li.location),
        marker_style: "dash".into(),
        items: vec![list_item_to_wire(li)],
    }
}

fn table_to_wire(t: &Table) -> WireNode {
    let header_rows = u32::try_from(t.header_rows.len()).unwrap_or(u32::MAX);
    let align = table_align_summary(t);
    let rows = t
        .all_rows()
        .map(|row| WireRow {
            cells: row.cells.iter().map(table_cell_to_wire).collect(),
        })
        .collect();
    let footnotes = t
        .footnotes
        .as_deref()
        .map(table_footnotes_to_wire)
        .unwrap_or_default();
    WireNode::Table {
        range: range_to_wire(&t.location),
        origin: origin_string(&t.location),
        caption: t.subject.as_string().to_string(),
        header_rows,
        align,
        rows,
        footnotes,
    }
}

fn table_cell_to_wire(cell: &TableCell) -> WireTableCell {
    let inlines = if cell.has_block_content() {
        // Inlines of a cell that has block-level children are best
        // reconstructed from the cell's own text content.
        text_content_to_wire(&cell.content)
    } else {
        text_content_to_wire(&cell.content)
    };
    WireTableCell {
        inlines,
        colspan: u32::try_from(cell.colspan).unwrap_or(1),
        rowspan: u32::try_from(cell.rowspan).unwrap_or(1),
    }
}

/// Summarise a table's per-cell alignment into a single string. Wire
/// tables carry one alignment per table; lex-core tracks alignment per
/// cell. We pick the alignment of the first non-`None` body cell, or
/// the empty string when no alignment is set anywhere.
fn table_align_summary(t: &Table) -> String {
    for row in t.all_rows() {
        for cell in &row.cells {
            match cell.align {
                TableCellAlignment::Left => return "left".into(),
                TableCellAlignment::Center => return "center".into(),
                TableCellAlignment::Right => return "right".into(),
                TableCellAlignment::None => {}
            }
        }
    }
    String::new()
}

fn table_footnotes_to_wire(footnotes: &List) -> Vec<WireFootnote> {
    footnotes
        .items
        .iter()
        .filter_map(|item| match item {
            ContentItem::ListItem(li) => Some(WireFootnote {
                marker: li.marker.as_string().to_string(),
                inlines: li
                    .text
                    .first()
                    .map(text_content_to_wire)
                    .unwrap_or_default(),
            }),
            _ => None,
        })
        .collect()
}

fn verbatim_to_wire(v: &Verbatim) -> WireNode {
    let label = v.closing_data.label.value.clone();
    let params = parameters_to_json(&v.closing_data.parameters);
    // Body is the first group's content lines joined by `\n`. Multi-
    // group verbatims collapse to their first group in the wire form;
    // the documented loss is acceptable for the current codec
    // consumer (lex.include never returns multi-group verbatims).
    let body_text = v
        .children
        .iter()
        .filter_map(|item| match item {
            ContentItem::VerbatimLine(vl) => Some(vl.content.as_string().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    WireNode::Verbatim {
        range: range_to_wire(&v.location),
        origin: origin_string(&v.location),
        label,
        params,
        body_text,
    }
}

/// Wire form for a `VerbatimLine` that appears outside a verbatim
/// block. Wraps it as a single-line `Verbatim` carrying an empty label
/// — the reverse codec recognises the empty label and reconstructs a
/// `VerbatimLine`.
fn verbatim_line_standalone_to_wire(vl: &VerbatimLine) -> WireNode {
    WireNode::Verbatim {
        range: range_to_wire(&vl.location),
        origin: origin_string(&vl.location),
        label: String::new(),
        params: Value::Object(Map::new()),
        body_text: vl.content.as_string().to_string(),
    }
}

/// Wire form for a `TextLine` that appears outside a `Paragraph`. The
/// parser doesn't produce this directly, but `to_wire_node` must stay
/// total, so we wrap it as a single-line paragraph.
fn text_line_standalone_to_wire(tl: &TextLine) -> WireNode {
    WireNode::Paragraph {
        range: range_to_wire(&tl.location),
        origin: origin_string(&tl.location),
        inlines: text_content_to_wire(&tl.content),
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

fn decoration_style_name(style: DecorationStyle) -> &'static str {
    match style {
        DecorationStyle::Plain => "dash",
        DecorationStyle::Numerical => "numerical",
        DecorationStyle::Alphabetical => "alphabetical",
        DecorationStyle::Roman => "roman",
    }
}
