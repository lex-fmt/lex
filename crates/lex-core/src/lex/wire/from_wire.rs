//! Reverse codec: `lex_extension::WireNode` → lex-core internal AST.
//!
//! Fallible — wire input may be malformed (handler returned an unknown
//! kind, missing required field, etc.). Recognised variants produce
//! lex-core [`ContentItem`]s; unrecognised shapes return
//! [`FromWireError::UnsupportedKind`].
//!
//! The reverse path mirrors [`super::to_wire`]: every `WireNode` variant
//! emitted by the forward codec has a defined inverse, so a forward+
//! reverse round trip on a parser-produced AST yields a structurally
//! equivalent tree (modulo the documented losses listed in the module-
//! level docs).

use crate::lex::ast::elements::annotation::Annotation as CoreAnnotation;
use crate::lex::ast::elements::blank_line_group::BlankLineGroup;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::data::Data;
use crate::lex::ast::elements::definition::Definition;
use crate::lex::ast::elements::label::Label;
use crate::lex::ast::elements::list::{List, ListItem};
use crate::lex::ast::elements::paragraph::{Paragraph, TextLine};
use crate::lex::ast::elements::parameter::Parameter;
use crate::lex::ast::elements::sequence_marker::SequenceMarker;
use crate::lex::ast::elements::session::Session;
use crate::lex::ast::elements::table::{Table, TableCell, TableCellAlignment, TableRow};
use crate::lex::ast::elements::typed_content::{ContentElement, SessionContent, VerbatimContent};
use crate::lex::ast::elements::verbatim::{Verbatim, VerbatimBlockMode};
use crate::lex::ast::elements::verbatim_line::VerbatimLine;
use crate::lex::ast::TextContent;
use lex_extension::wire::{WireFootnote, WireListItem, WireNode, WireRow, WireTableCell};
use serde_json::Value;

use super::error::FromWireError;
use super::inline::text_content_from_wire;
use super::range::{range_from_wire_with_origin, OriginInterner};

/// Convert a `WireNode::Document` into a list of lex-core
/// `ContentItem`s — one per child. The `WireNode::Document` wrapper
/// itself is unwrapped; callers wrap the resulting children in a host
/// container as needed.
///
/// Non-document roots are accepted: if `node` is not a `Document` (for
/// example a single `WireNode::Paragraph`), the result is a single-
/// element vector containing the converted item.
///
/// When `node` is `WireNode::Document`, its `origin` (if any) is
/// inherited by every child whose own `origin` slot is `None` —
/// matching how the parser's `stamp_doc` walks the tree, so handler
/// authors only need to stamp the document root and the codec
/// propagates the origin to every node downstream.
pub fn from_wire_node(node: &WireNode) -> Result<Vec<ContentItem>, FromWireError> {
    let mut interner = OriginInterner::new();
    match node {
        WireNode::Document {
            children, origin, ..
        } => from_wire_subtree_interned(children, origin.as_deref(), &mut interner),
        single => Ok(vec![convert_one(single, None, &mut interner)?]),
    }
}

/// Convert a slice of wire nodes into a vector of `ContentItem`s.
pub fn from_wire_subtree(nodes: &[WireNode]) -> Result<Vec<ContentItem>, FromWireError> {
    let mut interner = OriginInterner::new();
    from_wire_subtree_interned(nodes, None, &mut interner)
}

/// Internal worker shared by `from_wire_node` and `from_wire_subtree`
/// so a single [`OriginInterner`] is threaded through every nested
/// recursion of a single decode call. `inherited_origin` is the
/// effective origin from the closest ancestor with a stamped
/// `origin`; child nodes whose own `origin` slot is `None` inherit
/// it so handler authors don't have to manually stamp every node.
fn from_wire_subtree_interned(
    nodes: &[WireNode],
    inherited_origin: Option<&str>,
    interner: &mut OriginInterner,
) -> Result<Vec<ContentItem>, FromWireError> {
    nodes
        .iter()
        .map(|n| convert_one(n, inherited_origin, interner))
        .collect()
}

fn convert_one(
    node: &WireNode,
    inherited_origin: Option<&str>,
    interner: &mut OriginInterner,
) -> Result<ContentItem, FromWireError> {
    // Helper: a node's effective origin is its own `origin` if set,
    // otherwise the inherited one from the parent. Children are
    // walked with this effective origin as their inherited.
    fn effective<'a>(own: Option<&'a str>, inherited: Option<&'a str>) -> Option<&'a str> {
        own.or(inherited)
    }

    // `WireNode` is `#[non_exhaustive]` and its variants may gain
    // new optional fields over time; the struct-variant patterns
    // below use `..` so adding a field upstream doesn't break this
    // crate at the next bump. The fields we explicitly bind are
    // the only ones the codec needs; future additions land in `..`
    // and are silently ignored until the codec opts in.
    match node {
        WireNode::Paragraph {
            range,
            origin,
            inlines,
            ..
        } => {
            let eff = effective(origin.as_deref(), inherited_origin);
            Ok(ContentItem::Paragraph(paragraph_from_wire(
                range, eff, inlines, interner,
            )))
        }
        WireNode::Blank { range, origin, .. } => {
            // Reconstruct the blank-line count from the range when
            // possible: an N-line blank group spans N lines, so
            // `end.line - start.line` (clamped to >= 1) recovers the
            // count. Wire ranges that came from lex-core preserve
            // line boundaries; ranges with collapsed start/end fall
            // back to count = 1.
            let span_lines = range.end.line().saturating_sub(range.start.line());
            let count = (span_lines as usize).max(1);
            let mut blg = BlankLineGroup::new(count, Vec::new());
            let eff = effective(origin.as_deref(), inherited_origin);
            blg.location = range_from_wire_with_origin(range, eff, interner);
            Ok(ContentItem::BlankLineGroup(blg))
        }
        WireNode::Annotation {
            range,
            origin,
            label,
            params,
            body,
            ..
        } => {
            let eff = effective(origin.as_deref(), inherited_origin);
            Ok(ContentItem::Annotation(annotation_from_wire(
                range, eff, label, params, body, interner,
            )?))
        }
        WireNode::Session {
            range,
            origin,
            title,
            marker,
            children,
            ..
        } => {
            let eff = effective(origin.as_deref(), inherited_origin);
            Ok(ContentItem::Session(session_from_wire(
                range, eff, title, marker, children, interner,
            )?))
        }
        WireNode::Definition {
            range,
            origin,
            subject,
            children,
            ..
        } => {
            let eff = effective(origin.as_deref(), inherited_origin);
            Ok(ContentItem::Definition(definition_from_wire(
                range, eff, subject, children, interner,
            )?))
        }
        WireNode::List {
            range,
            origin,
            marker_style,
            items,
            ..
        } => {
            let eff = effective(origin.as_deref(), inherited_origin);
            Ok(ContentItem::List(list_from_wire(
                range,
                eff,
                marker_style,
                items,
                interner,
            )?))
        }
        WireNode::Table {
            range,
            origin,
            caption,
            header_rows,
            column_aligns,
            rows,
            footnotes,
            ..
        } => {
            let eff = effective(origin.as_deref(), inherited_origin);
            Ok(ContentItem::Table(Box::new(table_from_wire(
                range,
                eff,
                caption,
                *header_rows,
                column_aligns,
                rows,
                footnotes,
                interner,
            )?)))
        }
        WireNode::Verbatim {
            range,
            origin,
            label,
            params,
            body_text,
            subject,
            mode,
            ..
        } => {
            let eff = effective(origin.as_deref(), inherited_origin);
            Ok(verbatim_from_wire(
                range, eff, label, params, body_text, subject, mode, interner,
            )?)
        }
        // `Image` / `Video` / `Audio` are wire_version 2 media kinds.
        // lex-core has no typed `ContentItem` variant for them — the
        // information lives in `ContentItem::Verbatim` with the same
        // params lex-babel's media helpers consume. Round-trip them
        // back to a Verbatim with the canonical label + params, so
        // downstream `from_lex_verbatim` (or any consumer using the
        // free `image_from_params` / `video_from_params` /
        // `audio_from_params` helpers in lex-babel) sees the shape
        // it expects.
        WireNode::Image {
            range,
            origin,
            src,
            alt,
            title,
        } => {
            let eff = effective(origin.as_deref(), inherited_origin);
            let mut params = serde_json::Map::new();
            params.insert("src".into(), Value::String(src.clone()));
            if !alt.is_empty() {
                params.insert("alt".into(), Value::String(alt.clone()));
            }
            if let Some(title) = title {
                params.insert("title".into(), Value::String(title.clone()));
            }
            Ok(verbatim_from_wire(
                range,
                eff,
                "lex.media.image",
                &Value::Object(params),
                "",
                "",
                "inflow",
                interner,
            )?)
        }
        WireNode::Video {
            range,
            origin,
            src,
            title,
            poster,
        } => {
            let eff = effective(origin.as_deref(), inherited_origin);
            let mut params = serde_json::Map::new();
            params.insert("src".into(), Value::String(src.clone()));
            if let Some(title) = title {
                params.insert("title".into(), Value::String(title.clone()));
            }
            if let Some(poster) = poster {
                params.insert("poster".into(), Value::String(poster.clone()));
            }
            Ok(verbatim_from_wire(
                range,
                eff,
                "lex.media.video",
                &Value::Object(params),
                "",
                "",
                "inflow",
                interner,
            )?)
        }
        WireNode::Audio {
            range,
            origin,
            src,
            title,
        } => {
            let eff = effective(origin.as_deref(), inherited_origin);
            let mut params = serde_json::Map::new();
            params.insert("src".into(), Value::String(src.clone()));
            if let Some(title) = title {
                params.insert("title".into(), Value::String(title.clone()));
            }
            Ok(verbatim_from_wire(
                range,
                eff,
                "lex.media.audio",
                &Value::Object(params),
                "",
                "",
                "inflow",
                interner,
            )?)
        }
        // WireNode is `#[non_exhaustive]` — future kinds surface here.
        _ => Err(FromWireError::UnsupportedKind {
            kind: kind_name(node).into(),
        }),
    }
}

fn paragraph_from_wire(
    range: &lex_extension::wire::Range,
    origin: Option<&str>,
    inlines: &[lex_extension::wire::WireInline],
    interner: &mut OriginInterner,
) -> Paragraph {
    let location = range_from_wire_with_origin(range, origin, interner);
    // Round-trip the wire inlines back into a single source string,
    // then split on `\n` to reconstruct multiple TextLines. The
    // forward codec inserts an explicit `\n` Text inline between
    // each TextLine, so an N-line paragraph round-trips back to N
    // TextLines.
    let combined = text_content_from_wire(inlines);
    let raw = combined.as_string();
    let line_strings: Vec<&str> = if raw.is_empty() {
        Vec::new()
    } else {
        raw.split('\n').collect()
    };
    // Stamp each reconstructed line's `TextContent` with a location.
    // Without it `lex_analysis::inline::extract_references` early-returns
    // (it keys off `TextContent.location`), so reference-based
    // diagnostics (missing-footnote and the `check --references` family)
    // never fire inside an included/spliced fragment. The byte span is
    // synthetic — the wire `Range` carries no byte offsets — but it is
    // used only as an additive base by the inline walker, so a per-line
    // `0..len` span yields correct *relative* reference positions. The
    // `line`/`origin_path` carry the paragraph's, advanced one line per
    // split entry, so a reference finding is reported on the right line
    // and blamed on its origin file.
    let base_line = location.start.line;
    let base_column = location.start.column;
    let origin_path = location.origin_path.clone();
    let lines: Vec<ContentItem> = line_strings
        .into_iter()
        .enumerate()
        .map(|(idx, line_str)| {
            let line = base_line + idx;
            // The first line keeps the paragraph's start column;
            // continuation lines begin at column 0.
            let column = if idx == 0 { base_column } else { 0 };
            let len = line_str.len();
            let utf16_len: usize = line_str.chars().map(char::len_utf16).sum();
            let mut range = crate::lex::ast::range::Range::new(
                0..len,
                crate::lex::ast::range::Position::new(line, column),
                crate::lex::ast::range::Position::new(line, column + utf16_len),
            );
            range.origin_path = origin_path.clone();
            ContentItem::TextLine(TextLine::new(TextContent::from_string(
                line_str.to_string(),
                Some(range),
            )))
        })
        .collect();
    let mut p = if lines.is_empty() {
        Paragraph::new(vec![ContentItem::TextLine(TextLine::new(
            TextContent::empty(),
        ))])
    } else {
        Paragraph::new(lines)
    };
    p.location = location;
    p
}

fn annotation_from_wire(
    range: &lex_extension::wire::Range,
    origin: Option<&str>,
    label: &str,
    params: &Value,
    body: &Value,
    interner: &mut OriginInterner,
) -> Result<CoreAnnotation, FromWireError> {
    let parameters = parameters_from_json(params)?;
    let label = Label::new(label.to_string());
    let data = Data::new(label, parameters);

    let children = annotation_body_from_json(body, origin, interner)?;
    let mut a = CoreAnnotation::from_data(data, Vec::new());
    a.location = range_from_wire_with_origin(range, origin, interner);
    if !children.is_empty() {
        for child in children {
            a.children.as_mut_vec().push(child);
        }
    }
    Ok(a)
}

fn session_from_wire(
    range: &lex_extension::wire::Range,
    origin: Option<&str>,
    title: &str,
    marker: &Option<String>,
    children: &[WireNode],
    interner: &mut OriginInterner,
) -> Result<Session, FromWireError> {
    let title_tc = TextContent::from_string(title.to_string(), None);
    // Children inherit the session's effective origin if they don't
    // carry one of their own — same rule as the parser's stamp pass.
    let typed_children = wire_children_to_session_content(children, origin, interner)?;
    let mut s = Session::new(title_tc, typed_children);
    s.location = range_from_wire_with_origin(range, origin, interner);
    s.marker = marker
        .as_deref()
        .and_then(|m| SequenceMarker::parse(m, None));
    Ok(s)
}

fn definition_from_wire(
    range: &lex_extension::wire::Range,
    origin: Option<&str>,
    subject: &str,
    children: &[WireNode],
    interner: &mut OriginInterner,
) -> Result<Definition, FromWireError> {
    let subject_tc = TextContent::from_string(subject.to_string(), None);
    let typed_children = wire_children_to_content_elements(children, origin, interner)?;
    let mut d = Definition::new(subject_tc, typed_children);
    d.location = range_from_wire_with_origin(range, origin, interner);
    Ok(d)
}

fn list_from_wire(
    range: &lex_extension::wire::Range,
    origin: Option<&str>,
    marker_style: &str,
    items: &[WireListItem],
    interner: &mut OriginInterner,
) -> Result<List, FromWireError> {
    let marker = synthetic_marker_for_style(marker_style);
    // `WireListItem` has no `origin` slot of its own. List items in
    // a parsed lex source share the parent list's authoring file —
    // they can't span files within a single list — so we inherit the
    // parent's `origin` for each decoded item. Without this, spliced
    // list items would lose `origin_path` after a wire round-trip,
    // breaking origin-aware tooling (file-reference resolution,
    // scoped footnote lookup) that runs over the merged tree.
    let list_items = items
        .iter()
        .enumerate()
        .map(|(index, item)| list_item_from_wire(item, marker_style, index, origin, interner))
        .collect::<Result<Vec<_>, _>>()?;
    let mut l = List::new(list_items);
    l.location = range_from_wire_with_origin(range, origin, interner);
    l.marker = marker;
    Ok(l)
}

fn list_item_from_wire(
    item: &WireListItem,
    marker_style: &str,
    index: usize,
    parent_origin: Option<&str>,
    interner: &mut OriginInterner,
) -> Result<ListItem, FromWireError> {
    let combined = text_content_from_wire(&item.inlines);
    let raw = combined.as_string();
    let text_lines: Vec<TextContent> = if raw.is_empty() {
        vec![TextContent::empty()]
    } else {
        raw.split('\n')
            .map(|s| TextContent::from_string(s.to_string(), None))
            .collect()
    };
    let marker_text = synthetic_marker_text(marker_style, index);
    let typed_children =
        wire_children_to_content_elements(&item.children, parent_origin, interner)?;
    let mut li = ListItem::with_text_content(
        TextContent::from_string(marker_text, None),
        text_lines
            .first()
            .cloned()
            .unwrap_or_else(TextContent::empty),
        typed_children,
    );
    if text_lines.len() > 1 {
        li.text = text_lines;
    }
    li.location = range_from_wire_with_origin(&item.range, parent_origin, interner);
    Ok(li)
}

fn synthetic_marker_text(marker_style: &str, index: usize) -> String {
    match marker_style {
        "dash" => "-".into(),
        "numerical" => format!("{}.", index + 1),
        // Compute the modulo on `usize` first so lists with more than
        // 256 items don't wrap before the cast — `index as u8` would
        // truncate at index=256, mis-numbering everything past.
        "alphabetical" => format!("{}.", char::from(b'a' + ((index % 26) as u8))),
        "roman" => format!("{}.", roman_numeral(index + 1)),
        _ => "-".into(),
    }
}

fn synthetic_marker_for_style(marker_style: &str) -> Option<SequenceMarker> {
    let probe = synthetic_marker_text(marker_style, 0);
    SequenceMarker::parse(&probe, None)
}

fn roman_numeral(mut n: usize) -> String {
    const TABLE: &[(usize, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, sym) in TABLE {
        while n >= *value {
            out.push_str(sym);
            n -= *value;
        }
    }
    if out.is_empty() {
        "I".into()
    } else {
        out
    }
}

#[allow(clippy::too_many_arguments)]
fn table_from_wire(
    range: &lex_extension::wire::Range,
    origin: Option<&str>,
    caption: &str,
    header_rows: u32,
    column_aligns: &[String],
    rows: &[WireRow],
    footnotes: &[WireFootnote],
    interner: &mut OriginInterner,
) -> Result<Table, FromWireError> {
    // Per-column alignment in wire_version 2 — one entry per column.
    // Applied to every cell in that column on the reverse codec; rows
    // shorter than `column_aligns.length` get cells dropped on the
    // wire and aren't reconstructed here.
    let column_alignment: Vec<TableCellAlignment> = column_aligns
        .iter()
        .map(|s| match s.as_str() {
            "left" => TableCellAlignment::Left,
            "center" => TableCellAlignment::Center,
            "right" => TableCellAlignment::Right,
            _ => TableCellAlignment::None,
        })
        .collect();
    let header_count = header_rows as usize;
    let mut header_vec = Vec::with_capacity(header_count.min(rows.len()));
    let mut body_vec = Vec::with_capacity(rows.len().saturating_sub(header_count));
    for (i, row) in rows.iter().enumerate() {
        let is_header = i < header_count;
        // Track a running column cursor so cells with `colspan > 1`
        // advance past the columns they cover when picking
        // alignments. Naively using the cell index would mis-align
        // every cell after a spanning one.
        let mut col_cursor: usize = 0;
        let cells = row
            .cells
            .iter()
            .map(|c| {
                let align = column_alignment
                    .get(col_cursor)
                    .copied()
                    .unwrap_or(TableCellAlignment::None);
                col_cursor =
                    col_cursor.saturating_add(usize::try_from(c.colspan.max(1)).unwrap_or(1));
                table_cell_from_wire(c, align, is_header)
            })
            .collect();
        let table_row = TableRow::new(cells);
        if is_header {
            header_vec.push(table_row);
        } else {
            body_vec.push(table_row);
        }
    }
    let subject = TextContent::from_string(caption.to_string(), None);
    let mut t = Table::new(subject, header_vec, body_vec, VerbatimBlockMode::Inflow);
    t.location = range_from_wire_with_origin(range, origin, interner);
    if !footnotes.is_empty() {
        let footnote_items: Vec<ListItem> = footnotes
            .iter()
            .map(|f| {
                let combined = text_content_from_wire(&f.inlines);
                ListItem::with_text_content(
                    TextContent::from_string(f.marker.clone(), None),
                    combined,
                    Vec::new(),
                )
            })
            .collect();
        t.footnotes = Some(Box::new(List::new(footnote_items)));
    }
    Ok(t)
}

fn table_cell_from_wire(
    cell: &WireTableCell,
    align: TableCellAlignment,
    header: bool,
) -> TableCell {
    let content = text_content_from_wire(&cell.inlines);
    TableCell::new(content)
        .with_span(cell.colspan as usize, cell.rowspan as usize)
        .with_align(align)
        .with_header(header)
}

#[allow(clippy::too_many_arguments)]
fn verbatim_from_wire(
    range: &lex_extension::wire::Range,
    origin: Option<&str>,
    label: &str,
    params: &Value,
    body_text: &str,
    subject: &str,
    mode: &str,
    interner: &mut OriginInterner,
) -> Result<ContentItem, FromWireError> {
    if label == "lex.internal.unsupported.unknown" {
        return Err(FromWireError::UnsupportedKind {
            kind: "unknown".into(),
        });
    }
    if let Some(stripped) = label.strip_prefix("lex.internal.unsupported.") {
        // Forward codec emitted this for an as-yet-unwired variant;
        // surface the gap rather than silently dropping content.
        return Err(FromWireError::UnsupportedKind {
            kind: stripped.to_string(),
        });
    }

    if label.is_empty() {
        // Standalone VerbatimLine round-trip — see
        // `verbatim_line_standalone_to_wire` in the forward codec.
        let mut vl =
            VerbatimLine::from_text_content(TextContent::from_string(body_text.to_string(), None));
        vl.location = range_from_wire_with_origin(range, origin, interner);
        return Ok(ContentItem::VerbatimLine(vl));
    }

    let parameters = parameters_from_json(params)?;
    let closing_data = Data::new(Label::new(label.to_string()), parameters);
    let typed_lines: Vec<VerbatimContent> = if body_text.is_empty() {
        Vec::new()
    } else {
        body_text
            .split('\n')
            .map(|line| {
                VerbatimContent::VerbatimLine(VerbatimLine::from_text_content(
                    TextContent::from_string(line.to_string(), None),
                ))
            })
            .collect()
    };
    let subject_tc = if subject.is_empty() {
        TextContent::empty()
    } else {
        TextContent::from_string(subject.to_string(), None)
    };
    let block_mode = parse_verbatim_mode(mode);
    let mut v = Verbatim::new(subject_tc, typed_lines, closing_data, block_mode);
    v.location = range_from_wire_with_origin(range, origin, interner);
    Ok(ContentItem::VerbatimBlock(Box::new(v)))
}

/// Map the wire `mode` string back to a [`VerbatimBlockMode`].
/// Unknown values fall back to `Inflow` — the documented default
/// matching the parser's behaviour for ambiguous mode classification.
fn parse_verbatim_mode(mode: &str) -> VerbatimBlockMode {
    match mode {
        "fullwidth" => VerbatimBlockMode::Fullwidth,
        // "inflow" or anything unrecognised
        _ => VerbatimBlockMode::Inflow,
    }
}

fn parameters_from_json(params: &Value) -> Result<Vec<Parameter>, FromWireError> {
    let obj = params
        .as_object()
        .ok_or_else(|| FromWireError::MalformedField {
            field: "params",
            detail: "expected JSON object".into(),
        })?;
    let mut out = Vec::with_capacity(obj.len());
    for (k, v) in obj {
        let value_str = match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        out.push(Parameter {
            key: k.clone(),
            value: value_str,
            location: crate::lex::ast::range::Range::new(
                0..0,
                crate::lex::ast::range::Position::new(0, 0),
                crate::lex::ast::range::Position::new(0, 0),
            ),
        });
    }
    Ok(out)
}

fn annotation_body_from_json(
    body: &Value,
    inherited_origin: Option<&str>,
    interner: &mut OriginInterner,
) -> Result<Vec<ContentItem>, FromWireError> {
    match body {
        Value::Null => Ok(Vec::new()),
        Value::String(text) => {
            // Single-line / opaque-text annotation body. lex-core
            // represents this as a single Paragraph child (matching
            // `extract_annotation_single_content` in the parser). We
            // mirror that here so content isn't silently dropped.
            let line = TextLine::new(TextContent::from_string(text.clone(), None));
            let para = Paragraph::new(vec![ContentItem::TextLine(line)]);
            Ok(vec![ContentItem::Paragraph(para)])
        }
        Value::Object(obj) => {
            let kind = obj.get("kind").and_then(|v| v.as_str());
            if kind != Some("block") {
                return Err(FromWireError::MalformedField {
                    field: "body.kind",
                    detail: format!("expected \"block\", got {kind:?}"),
                });
            }
            let children: Vec<WireNode> = match obj.get("children") {
                Some(arr) => serde_json::from_value(arr.clone())
                    .map_err(|e| FromWireError::DeserialisationFailed(e.to_string()))?,
                None => Vec::new(),
            };
            from_wire_subtree_interned(&children, inherited_origin, interner)
        }
        _ => Err(FromWireError::MalformedField {
            field: "body",
            detail: "expected null, string, or object".into(),
        }),
    }
}

fn wire_children_to_session_content(
    children: &[WireNode],
    inherited_origin: Option<&str>,
    interner: &mut OriginInterner,
) -> Result<Vec<SessionContent>, FromWireError> {
    let items = from_wire_subtree_interned(children, inherited_origin, interner)?;
    Ok(items.into_iter().map(SessionContent::from).collect())
}

fn wire_children_to_content_elements(
    children: &[WireNode],
    inherited_origin: Option<&str>,
    interner: &mut OriginInterner,
) -> Result<Vec<ContentElement>, FromWireError> {
    let items = from_wire_subtree_interned(children, inherited_origin, interner)?;
    items
        .into_iter()
        .map(|item| {
            ContentElement::try_from(item).map_err(|e| FromWireError::MalformedField {
                field: "children",
                detail: e.to_string(),
            })
        })
        .collect()
}

fn kind_name(node: &WireNode) -> &'static str {
    match node {
        WireNode::Document { .. } => "document",
        WireNode::Session { .. } => "session",
        WireNode::Definition { .. } => "definition",
        WireNode::Paragraph { .. } => "paragraph",
        WireNode::List { .. } => "list",
        WireNode::Verbatim { .. } => "verbatim",
        WireNode::Table { .. } => "table",
        WireNode::Image { .. } => "image",
        WireNode::Video { .. } => "video",
        WireNode::Audio { .. } => "audio",
        WireNode::Annotation { .. } => "annotation",
        WireNode::Blank { .. } => "blank",
        _ => "unknown",
    }
}
