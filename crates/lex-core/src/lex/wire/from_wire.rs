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
use super::range::range_from_wire;

/// Convert a `WireNode::Document` into a list of lex-core
/// `ContentItem`s — one per child. The `WireNode::Document` wrapper
/// itself is unwrapped; callers wrap the resulting children in a host
/// container as needed.
///
/// Non-document roots are accepted: if `node` is not a `Document` (for
/// example a single `WireNode::Paragraph`), the result is a single-
/// element vector containing the converted item.
pub fn from_wire_node(node: &WireNode) -> Result<Vec<ContentItem>, FromWireError> {
    match node {
        WireNode::Document { children, .. } => from_wire_subtree(children),
        single => Ok(vec![convert_one(single)?]),
    }
}

/// Convert a slice of wire nodes into a vector of `ContentItem`s.
pub fn from_wire_subtree(nodes: &[WireNode]) -> Result<Vec<ContentItem>, FromWireError> {
    nodes.iter().map(convert_one).collect()
}

fn convert_one(node: &WireNode) -> Result<ContentItem, FromWireError> {
    match node {
        WireNode::Paragraph { range, inlines, .. } => {
            Ok(ContentItem::Paragraph(paragraph_from_wire(range, inlines)))
        }
        WireNode::Blank { range, .. } => {
            // Reconstruct the blank-line count from the range when
            // possible: an N-line blank group spans N lines, so
            // `end.line - start.line` (clamped to >= 1) recovers the
            // count. Wire ranges that came from lex-core preserve
            // line boundaries; ranges with collapsed start/end fall
            // back to count = 1.
            let span_lines = range.end.line().saturating_sub(range.start.line());
            let count = (span_lines as usize).max(1);
            let mut blg = BlankLineGroup::new(count, Vec::new());
            blg.location = range_from_wire(range);
            Ok(ContentItem::BlankLineGroup(blg))
        }
        WireNode::Annotation {
            range,
            label,
            params,
            body,
            ..
        } => Ok(ContentItem::Annotation(annotation_from_wire(
            range, label, params, body,
        )?)),
        WireNode::Session {
            range,
            title,
            marker,
            children,
            ..
        } => Ok(ContentItem::Session(session_from_wire(
            range, title, marker, children,
        )?)),
        WireNode::Definition {
            range,
            subject,
            children,
            ..
        } => Ok(ContentItem::Definition(definition_from_wire(
            range, subject, children,
        )?)),
        WireNode::List {
            range,
            marker_style,
            items,
            ..
        } => Ok(ContentItem::List(list_from_wire(
            range,
            marker_style,
            items,
        )?)),
        WireNode::Table {
            range,
            caption,
            header_rows,
            align,
            rows,
            footnotes,
            ..
        } => Ok(ContentItem::Table(Box::new(table_from_wire(
            range,
            caption,
            *header_rows,
            align,
            rows,
            footnotes,
        )?))),
        WireNode::Verbatim {
            range,
            label,
            params,
            body_text,
            ..
        } => Ok(verbatim_from_wire(range, label, params, body_text)?),
        // WireNode is `#[non_exhaustive]` — future kinds surface here.
        _ => Err(FromWireError::UnsupportedKind {
            kind: kind_name(node).into(),
        }),
    }
}

fn paragraph_from_wire(
    range: &lex_extension::wire::Range,
    inlines: &[lex_extension::wire::WireInline],
) -> Paragraph {
    let location = range_from_wire(range);
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
    let lines: Vec<ContentItem> = line_strings
        .into_iter()
        .map(|line_str| {
            ContentItem::TextLine(TextLine::new(TextContent::from_string(
                line_str.to_string(),
                None,
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
    label: &str,
    params: &Value,
    body: &Value,
) -> Result<CoreAnnotation, FromWireError> {
    let parameters = parameters_from_json(params)?;
    let label = Label::new(label.to_string());
    let data = Data::new(label, parameters);

    let children = annotation_body_from_json(body)?;
    let mut a = CoreAnnotation::from_data(data, Vec::new());
    a.location = range_from_wire(range);
    if !children.is_empty() {
        for child in children {
            a.children.as_mut_vec().push(child);
        }
    }
    Ok(a)
}

fn session_from_wire(
    range: &lex_extension::wire::Range,
    title: &str,
    marker: &Option<String>,
    children: &[WireNode],
) -> Result<Session, FromWireError> {
    let title_tc = TextContent::from_string(title.to_string(), None);
    let typed_children = wire_children_to_session_content(children)?;
    let mut s = Session::new(title_tc, typed_children);
    s.location = range_from_wire(range);
    s.marker = marker
        .as_deref()
        .and_then(|m| SequenceMarker::parse(m, None));
    Ok(s)
}

fn definition_from_wire(
    range: &lex_extension::wire::Range,
    subject: &str,
    children: &[WireNode],
) -> Result<Definition, FromWireError> {
    let subject_tc = TextContent::from_string(subject.to_string(), None);
    let typed_children = wire_children_to_content_elements(children)?;
    let mut d = Definition::new(subject_tc, typed_children);
    d.location = range_from_wire(range);
    Ok(d)
}

fn list_from_wire(
    range: &lex_extension::wire::Range,
    marker_style: &str,
    items: &[WireListItem],
) -> Result<List, FromWireError> {
    let marker = synthetic_marker_for_style(marker_style);
    let list_items = items
        .iter()
        .enumerate()
        .map(|(index, item)| list_item_from_wire(item, marker_style, index))
        .collect::<Result<Vec<_>, _>>()?;
    let mut l = List::new(list_items);
    l.location = range_from_wire(range);
    l.marker = marker;
    Ok(l)
}

fn list_item_from_wire(
    item: &WireListItem,
    marker_style: &str,
    index: usize,
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
    let typed_children = wire_children_to_content_elements(&item.children)?;
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
    li.location = range_from_wire(&item.range);
    Ok(li)
}

fn synthetic_marker_text(marker_style: &str, index: usize) -> String {
    match marker_style {
        "dash" => "-".into(),
        "numerical" => format!("{}.", index + 1),
        "alphabetical" => format!("{}.", char::from(b'a' + (index as u8 % 26))),
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

fn table_from_wire(
    range: &lex_extension::wire::Range,
    caption: &str,
    header_rows: u32,
    align: &str,
    rows: &[WireRow],
    footnotes: &[WireFootnote],
) -> Result<Table, FromWireError> {
    let alignment = match align {
        "left" => TableCellAlignment::Left,
        "center" => TableCellAlignment::Center,
        "right" => TableCellAlignment::Right,
        _ => TableCellAlignment::None,
    };
    let header_count = header_rows as usize;
    let mut header_vec = Vec::with_capacity(header_count.min(rows.len()));
    let mut body_vec = Vec::with_capacity(rows.len().saturating_sub(header_count));
    for (i, row) in rows.iter().enumerate() {
        let is_header = i < header_count;
        let cells = row
            .cells
            .iter()
            .map(|c| table_cell_from_wire(c, alignment, is_header))
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
    t.location = range_from_wire(range);
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

fn verbatim_from_wire(
    range: &lex_extension::wire::Range,
    label: &str,
    params: &Value,
    body_text: &str,
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
        vl.location = range_from_wire(range);
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
    let subject = TextContent::empty();
    let mut v = Verbatim::new(
        subject,
        typed_lines,
        closing_data,
        VerbatimBlockMode::Inflow,
    );
    v.location = range_from_wire(range);
    Ok(ContentItem::VerbatimBlock(Box::new(v)))
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

fn annotation_body_from_json(body: &Value) -> Result<Vec<ContentItem>, FromWireError> {
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
            from_wire_subtree(&children)
        }
        _ => Err(FromWireError::MalformedField {
            field: "body",
            detail: "expected null, string, or object".into(),
        }),
    }
}

fn wire_children_to_session_content(
    children: &[WireNode],
) -> Result<Vec<SessionContent>, FromWireError> {
    let items = from_wire_subtree(children)?;
    Ok(items.into_iter().map(SessionContent::from).collect())
}

fn wire_children_to_content_elements(
    children: &[WireNode],
) -> Result<Vec<ContentElement>, FromWireError> {
    let items = from_wire_subtree(children)?;
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
        WireNode::Annotation { .. } => "annotation",
        WireNode::Blank { .. } => "blank",
        _ => "unknown",
    }
}
