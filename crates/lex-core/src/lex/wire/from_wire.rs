//! Reverse codec: `lex_extension::WireNode` → lex-core internal AST.
//!
//! Fallible — wire input may be malformed (handler returned an unknown
//! kind, missing required field, etc.). Recognised variants produce
//! lex-core [`ContentItem`]s; unrecognised or partially-supported
//! shapes return [`FromWireError::UnsupportedKind`].
//!
//! Coverage matches `to_wire.rs`: this commit handles `Document`,
//! `Paragraph`, `Annotation`, and `Blank`. Follow-up commits on the
//! same PR extend to the remaining shapes.

use crate::lex::ast::elements::annotation::Annotation as CoreAnnotation;
use crate::lex::ast::elements::blank_line_group::BlankLineGroup;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::data::Data;
use crate::lex::ast::elements::label::Label;
use crate::lex::ast::elements::paragraph::{Paragraph, TextLine};
use crate::lex::ast::elements::parameter::Parameter;
use lex_extension::wire::WireNode;
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
        WireNode::Verbatim { label, .. } if label.starts_with("lex.internal.unsupported.") => {
            // Forward codec emitted this for an as-yet-unwired variant;
            // surface the gap rather than silently dropping content.
            Err(FromWireError::UnsupportedKind {
                kind: label
                    .strip_prefix("lex.internal.unsupported.")
                    .unwrap_or(label.as_str())
                    .to_string(),
            })
        }
        // Variants not yet wired in the reverse direction.
        WireNode::Document { .. }
        | WireNode::Session { .. }
        | WireNode::Definition { .. }
        | WireNode::List { .. }
        | WireNode::Verbatim { .. }
        | WireNode::Table { .. } => Err(FromWireError::UnsupportedKind {
            kind: kind_name(node).into(),
        }),
        // WireNode is `#[non_exhaustive]` — future kinds surface here.
        _ => Err(FromWireError::UnsupportedKind {
            kind: "unknown".into(),
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
            ContentItem::TextLine(TextLine::new(crate::lex::ast::TextContent::from_string(
                line_str.to_string(),
                None,
            )))
        })
        .collect();
    let mut p = if lines.is_empty() {
        Paragraph::new(vec![ContentItem::TextLine(TextLine::new(
            crate::lex::ast::TextContent::empty(),
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
    // Re-attach children via the container's typed setter.
    if !children.is_empty() {
        // GeneralContainer stores ContentItems; ContentElement is the
        // typed projection used by from_data. Splicing through the
        // raw container preserves whatever shapes the wire delivered.
        for child in children {
            a.children.as_mut_vec().push(child);
        }
    }
    Ok(a)
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
            let line = TextLine::new(crate::lex::ast::TextContent::from_string(
                text.clone(),
                None,
            ));
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
