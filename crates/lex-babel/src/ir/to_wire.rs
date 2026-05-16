//! Convert IR nodes to `lex_extension::wire::WireNode` shapes.
//!
//! The IR doesn't carry source positions, so every node we build here
//! has a zero range. Used by `render_dispatch` to construct the
//! `LabelCtx.body` payload handed to render handlers.

use lex_extension::wire::{
    AnnotationBody, Position as WirePosition, Range as WireRange, RefKind, WireFootnote,
    WireInline, WireListItem, WireNode, WireRow, WireTableCell,
};
use serde_json::{Map, Value};

use crate::ir::nodes::{
    Annotation, Audio, Definition, DocNode, Heading, Image, InlineContent, List, ListItem,
    Paragraph, Table, TableCell, TableCellAlignment, Verbatim, Video,
};

fn zero_range() -> WireRange {
    WireRange {
        start: WirePosition(0, 0),
        end: WirePosition(0, 0),
    }
}

/// Convert an IR `DocNode` to a `WireNode`. Returns `None` for variants
/// that have no block-level wire representation (`Inline` only appears
/// inside containers and is unwrapped by the caller).
pub fn ir_to_wire_node(node: &DocNode) -> Option<WireNode> {
    match node {
        DocNode::Document(_) => None,
        DocNode::Heading(h) => Some(heading_to_wire(h)),
        DocNode::Paragraph(p) => Some(paragraph_to_wire(p)),
        DocNode::List(l) => Some(list_to_wire(l)),
        DocNode::ListItem(_) => None,
        DocNode::Definition(d) => Some(definition_to_wire(d)),
        DocNode::Verbatim(v) => Some(verbatim_to_wire(v)),
        DocNode::Annotation(a) => Some(annotation_to_wire(a)),
        DocNode::Table(t) => Some(table_to_wire(t)),
        DocNode::Image(i) => Some(image_to_wire(i)),
        DocNode::Video(v) => Some(video_to_wire(v)),
        DocNode::Audio(a) => Some(audio_to_wire(a)),
        DocNode::Inline(_) => None,
    }
}

fn heading_to_wire(h: &Heading) -> WireNode {
    let mut inlines = inlines_to_wire(&h.content);
    inlines.push(WireInline::Text { text: "\n".into() });
    let mut children = vec![WireNode::Paragraph {
        range: zero_range(),
        origin: None,
        inlines,
    }];
    children.extend(h.children.iter().filter_map(ir_to_wire_node));
    WireNode::Session {
        range: zero_range(),
        origin: None,
        title: inlines_to_text(&h.content),
        marker: None,
        children,
    }
}

fn paragraph_to_wire(p: &Paragraph) -> WireNode {
    WireNode::Paragraph {
        range: zero_range(),
        origin: None,
        inlines: inlines_to_wire(&p.content),
    }
}

fn list_to_wire(l: &List) -> WireNode {
    let marker_style = match l.style {
        crate::ir::nodes::ListStyle::Bullet => "dash",
        crate::ir::nodes::ListStyle::Numeric => "numerical",
        crate::ir::nodes::ListStyle::AlphaLower | crate::ir::nodes::ListStyle::AlphaUpper => {
            "alphabetical"
        }
        crate::ir::nodes::ListStyle::RomanLower | crate::ir::nodes::ListStyle::RomanUpper => {
            "roman"
        }
    }
    .to_string();
    WireNode::List {
        range: zero_range(),
        origin: None,
        marker_style,
        items: l.items.iter().map(list_item_to_wire).collect(),
    }
}

fn list_item_to_wire(li: &ListItem) -> WireListItem {
    WireListItem {
        range: zero_range(),
        inlines: inlines_to_wire(&li.content),
        children: li.children.iter().filter_map(ir_to_wire_node).collect(),
    }
}

fn definition_to_wire(d: &Definition) -> WireNode {
    let children = d.description.iter().filter_map(ir_to_wire_node).collect();
    WireNode::Definition {
        range: zero_range(),
        origin: None,
        subject: inlines_to_text(&d.term),
        children,
    }
}

fn verbatim_to_wire(v: &Verbatim) -> WireNode {
    WireNode::Verbatim {
        range: zero_range(),
        origin: None,
        label: String::new(),
        params: Value::Object(Map::new()),
        body_text: v.content.clone(),
        subject: v.subject.clone().unwrap_or_default(),
        mode: "inflow".into(),
    }
}

fn annotation_to_wire(a: &Annotation) -> WireNode {
    let body = ir_annotation_body_to_json(&a.content);
    WireNode::Annotation {
        range: zero_range(),
        origin: None,
        label: a.label.clone(),
        params: ir_params_to_json(&a.parameters),
        body,
    }
}

fn table_to_wire(t: &Table) -> WireNode {
    let header_rows = u32::try_from(t.header.len()).unwrap_or(u32::MAX);
    let column_aligns = table_column_aligns(t);
    let rows = t
        .header
        .iter()
        .chain(t.rows.iter())
        .map(|row| WireRow {
            cells: row.cells.iter().map(table_cell_to_wire).collect(),
        })
        .collect();
    let footnotes = t
        .footnotes
        .iter()
        .filter_map(|node| match node {
            DocNode::Annotation(a) => Some(WireFootnote {
                marker: a.label.clone(),
                inlines: ir_annotation_first_paragraph_inlines(&a.content),
            }),
            _ => None,
        })
        .collect();
    WireNode::Table {
        range: zero_range(),
        origin: None,
        caption: t
            .caption
            .as_ref()
            .map(|c| inlines_to_text(c))
            .unwrap_or_default(),
        header_rows,
        column_aligns,
        rows,
        footnotes,
    }
}

fn table_cell_to_wire(cell: &TableCell) -> WireTableCell {
    let mut inlines = Vec::new();
    for node in &cell.content {
        if let DocNode::Paragraph(p) = node {
            inlines.extend(inlines_to_wire(&p.content));
        }
    }
    WireTableCell {
        inlines,
        colspan: u32::try_from(cell.colspan).unwrap_or(1),
        rowspan: u32::try_from(cell.rowspan).unwrap_or(1),
    }
}

fn table_column_aligns(t: &Table) -> Vec<String> {
    let mut max_width = 0usize;
    for row in t.header.iter().chain(t.rows.iter()) {
        let width: usize = row.cells.iter().map(|c| c.colspan.max(1)).sum();
        max_width = max_width.max(width);
    }
    let mut aligns: Vec<String> = vec![String::new(); max_width];
    let mut fill = |rows: &[crate::ir::nodes::TableRow]| {
        for row in rows {
            let mut col = 0usize;
            for cell in &row.cells {
                if col >= aligns.len() {
                    break;
                }
                if aligns[col].is_empty() {
                    if let Some(s) = align_str(cell.align) {
                        aligns[col] = s.into();
                    }
                }
                col = col.saturating_add(cell.colspan.max(1));
            }
        }
    };
    fill(&t.rows);
    fill(&t.header);
    aligns
}

fn align_str(a: TableCellAlignment) -> Option<&'static str> {
    match a {
        TableCellAlignment::Left => Some("left"),
        TableCellAlignment::Center => Some("center"),
        TableCellAlignment::Right => Some("right"),
        TableCellAlignment::None => None,
    }
}

fn ir_annotation_first_paragraph_inlines(content: &[DocNode]) -> Vec<WireInline> {
    for node in content {
        if let DocNode::Paragraph(p) = node {
            return inlines_to_wire(&p.content);
        }
    }
    Vec::new()
}

fn image_to_wire(i: &Image) -> WireNode {
    WireNode::Image {
        range: zero_range(),
        origin: None,
        src: i.src.clone(),
        alt: i.alt.clone(),
        title: i.title.clone(),
    }
}

fn video_to_wire(v: &Video) -> WireNode {
    WireNode::Video {
        range: zero_range(),
        origin: None,
        src: v.src.clone(),
        title: v.title.clone(),
        poster: v.poster.clone(),
    }
}

fn audio_to_wire(a: &Audio) -> WireNode {
    WireNode::Audio {
        range: zero_range(),
        origin: None,
        src: a.src.clone(),
        title: a.title.clone(),
    }
}

fn inlines_to_wire(content: &[InlineContent]) -> Vec<WireInline> {
    content.iter().map(inline_to_wire).collect()
}

fn inline_to_wire(inline: &InlineContent) -> WireInline {
    match inline {
        InlineContent::Text(t) => WireInline::Text { text: t.clone() },
        InlineContent::Bold(children) => WireInline::Bold {
            children: inlines_to_wire(children),
        },
        InlineContent::Italic(children) => WireInline::Italic {
            children: inlines_to_wire(children),
        },
        InlineContent::Code(t) => WireInline::Code { text: t.clone() },
        InlineContent::Math(t) => WireInline::Math { text: t.clone() },
        InlineContent::Reference { raw, kind } => WireInline::Reference {
            ref_kind: ref_kind_to_wire(kind),
            target: raw.clone(),
            label: None,
        },
        InlineContent::Link { text, href } => WireInline::Reference {
            ref_kind: RefKind::Url,
            target: href.clone(),
            label: Some(text.clone()),
        },
        InlineContent::Image(_) => WireInline::Text {
            text: String::new(),
        },
    }
}

/// Flatten IR inline content into a plain text string.
///
/// Recurses through `Bold`/`Italic` containers, surfaces `Code`/`Math`
/// raw text, uses `Reference` targets and `Link` anchor text directly.
/// Used wherever a renderer needs a plain-text view of inline content
/// (title flattening, alt-text synthesis, etc.) without HTML markup.
pub fn inlines_to_text(content: &[InlineContent]) -> String {
    content
        .iter()
        .map(|i| match i {
            InlineContent::Text(t) => t.clone(),
            InlineContent::Bold(c) | InlineContent::Italic(c) => inlines_to_text(c),
            InlineContent::Code(t) | InlineContent::Math(t) => t.clone(),
            InlineContent::Reference { raw, .. } => raw.clone(),
            InlineContent::Link { text, .. } => text.clone(),
            InlineContent::Image(img) => img.alt.clone(),
        })
        .collect()
}

pub fn ir_params_to_json(params: &[(String, String)]) -> Value {
    let mut obj = Map::with_capacity(params.len());
    for (k, v) in params {
        obj.insert(k.clone(), Value::String(v.clone()));
    }
    Value::Object(obj)
}

/// Build an [`AnnotationBody`] directly from an IR annotation's content
/// vector. Skips the JSON serialise → deserialise round-trip the
/// `ir_annotation_body_to_json` path required.
///
/// - empty content → `AnnotationBody::None`
/// - non-empty IR content with no block-level wire mapping (e.g. only
///   bare `Inline` nodes) → `AnnotationBody::None`
/// - non-empty with block wire children → `AnnotationBody::Lex`
pub fn ir_annotation_body(content: &[DocNode]) -> AnnotationBody {
    if content.is_empty() {
        return AnnotationBody::None;
    }
    let children: Vec<WireNode> = content.iter().filter_map(ir_to_wire_node).collect();
    if children.is_empty() {
        AnnotationBody::None
    } else {
        AnnotationBody::Lex { children }
    }
}

/// JSON form of [`ir_annotation_body`]. Kept for callers (host-spec
/// codec, future wire-payload synthesis) that need the on-wire JSON
/// shape rather than the in-process enum.
pub fn ir_annotation_body_to_json(content: &[DocNode]) -> Value {
    if content.is_empty() {
        return Value::Null;
    }
    let children: Vec<Value> = content
        .iter()
        .filter_map(ir_to_wire_node)
        .map(|w| serde_json::to_value(w).expect("wire node serialises"))
        .collect();
    if children.is_empty() {
        return Value::Null;
    }
    let mut obj = Map::with_capacity(2);
    obj.insert("kind".into(), Value::String("block".into()));
    obj.insert("children".into(), Value::Array(children));
    Value::Object(obj)
}

/// Map the lex-core reference classification onto the wire-level
/// [`RefKind`] used by extension handlers. The wire enum is coarser
/// than lex-core's: `AnnotationReference` has no direct wire variant
/// and surfaces as `General`; `NotSure` surfaces as `Unsure`.
fn ref_kind_to_wire(kind: &crate::ir::nodes::ReferenceType) -> RefKind {
    use crate::ir::nodes::ReferenceType;
    match kind {
        ReferenceType::ToCome { .. } => RefKind::Placeholder,
        ReferenceType::Citation(_) => RefKind::Citation,
        ReferenceType::AnnotationReference { .. } => RefKind::General,
        ReferenceType::FootnoteNumber { .. } => RefKind::Footnote,
        ReferenceType::Session { .. } => RefKind::Session,
        ReferenceType::Url { .. } => RefKind::Url,
        ReferenceType::File { .. } => RefKind::File,
        ReferenceType::General { .. } => RefKind::General,
        ReferenceType::NotSure => RefKind::Unsure,
    }
}
