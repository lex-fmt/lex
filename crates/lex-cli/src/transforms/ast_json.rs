//! AST → JSON transform.
//!
//! Produces a canonical JSON representation of the full AST tree, suitable
//! for parity testing between parsing engines (e.g., current Rust parser vs
//! tree-sitter). Backs the `ast-json` transform.

/// Convert AST (Document) to JSON-serializable format.
///
/// Produces a canonical JSON representation of the full AST tree, suitable for
/// parity testing between parsing engines (e.g., current Rust parser vs tree-sitter).
pub(super) fn ast_to_json(doc: &lex_core::lex::parsing::Document) -> serde_json::Value {
    use serde_json::json;

    let children: Vec<serde_json::Value> =
        doc.root.children.iter().map(content_item_to_json).collect();

    // Include document-level annotations if present
    let annotations: Vec<serde_json::Value> =
        doc.annotations.iter().map(annotation_to_json).collect();

    let mut doc_json = json!({
        "type": "Document",
        "title": doc.title(),
        "children": children,
    });

    if !annotations.is_empty() {
        doc_json["annotations"] = json!(annotations);
    }

    doc_json
}

fn content_item_to_json(item: &lex_core::lex::ast::ContentItem) -> serde_json::Value {
    use lex_core::lex::ast::ContentItem;
    use serde_json::json;

    match item {
        ContentItem::Session(s) => {
            let mut node = json!({
                "type": "Session",
                "title": s.title.as_string(),
                "children": s.children.iter().map(content_item_to_json).collect::<Vec<_>>(),
            });
            if let Some(marker) = &s.marker {
                node["marker"] = sequence_marker_to_json(marker);
            }
            if !s.annotations.is_empty() {
                node["annotations"] = json!(s
                    .annotations
                    .iter()
                    .map(annotation_to_json)
                    .collect::<Vec<_>>());
            }
            node
        }
        ContentItem::Paragraph(p) => {
            json!({
                "type": "Paragraph",
                "lines": p.lines.iter().map(content_item_to_json).collect::<Vec<_>>(),
            })
        }
        ContentItem::TextLine(tl) => {
            json!({
                "type": "TextLine",
                "content": tl.text(),
            })
        }
        ContentItem::List(l) => {
            let mut node = json!({
                "type": "List",
                "items": l.items.iter().map(content_item_to_json).collect::<Vec<_>>(),
            });
            if let Some(marker) = &l.marker {
                node["marker"] = sequence_marker_to_json(marker);
            }
            if !l.annotations.is_empty() {
                node["annotations"] = json!(l
                    .annotations
                    .iter()
                    .map(annotation_to_json)
                    .collect::<Vec<_>>());
            }
            node
        }
        ContentItem::ListItem(li) => {
            let mut node = json!({
                "type": "ListItem",
                "marker": li.marker.as_string(),
                "text": li.text.iter().map(|t| t.as_string()).collect::<Vec<_>>(),
                "children": li.children.iter().map(content_item_to_json).collect::<Vec<_>>(),
            });
            if !li.annotations.is_empty() {
                node["annotations"] = json!(li
                    .annotations
                    .iter()
                    .map(annotation_to_json)
                    .collect::<Vec<_>>());
            }
            node
        }
        ContentItem::Definition(d) => {
            let mut node = json!({
                "type": "Definition",
                "subject": d.subject.as_string(),
                "children": d.children.iter().map(content_item_to_json).collect::<Vec<_>>(),
            });
            if !d.annotations.is_empty() {
                node["annotations"] = json!(d
                    .annotations
                    .iter()
                    .map(annotation_to_json)
                    .collect::<Vec<_>>());
            }
            node
        }
        ContentItem::Annotation(a) => annotation_to_json(a),
        ContentItem::VerbatimBlock(fb) => {
            let groups: Vec<serde_json::Value> = fb
                .group()
                .map(|g| {
                    json!({
                        "subject": g.subject.as_string(),
                        "lines": g.children.iter().map(content_item_to_json).collect::<Vec<_>>(),
                    })
                })
                .collect();

            let mut node = json!({
                "type": "VerbatimBlock",
                "mode": format!("{:?}", fb.mode),
                "closing_label": fb.closing_data.label.value,
                "groups": groups,
            });
            if !fb.closing_data.parameters.is_empty() {
                node["closing_parameters"] = json!(fb
                    .closing_data
                    .parameters
                    .iter()
                    .map(|p| json!({"key": p.key, "value": p.value}))
                    .collect::<Vec<_>>());
            }
            if !fb.annotations.is_empty() {
                node["annotations"] = json!(fb
                    .annotations
                    .iter()
                    .map(annotation_to_json)
                    .collect::<Vec<_>>());
            }
            node
        }
        ContentItem::VerbatimLine(fl) => {
            json!({
                "type": "VerbatimLine",
                "content": fl.content.as_string(),
            })
        }
        ContentItem::Table(t) => {
            let header_rows: Vec<serde_json::Value> = t
                .header_rows
                .iter()
                .map(|row| {
                    json!({
                        "cells": row.cells.iter().map(|cell| json!({
                            "content": cell.content.as_string(),
                            "header": cell.header,
                            "align": format!("{:?}", cell.align),
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let body_rows: Vec<serde_json::Value> = t
                .body_rows
                .iter()
                .map(|row| {
                    json!({
                        "cells": row.cells.iter().map(|cell| json!({
                            "content": cell.content.as_string(),
                            "header": cell.header,
                            "align": format!("{:?}", cell.align),
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let mut node = json!({
                "type": "Table",
                "subject": t.subject.as_string(),
                "mode": format!("{:?}", t.mode),
                "header_rows": header_rows,
                "body_rows": body_rows,
            });
            // Table config is in annotations, not closing_data
            if !t.annotations.is_empty() {
                node["annotations"] = json!(t
                    .annotations
                    .iter()
                    .map(annotation_to_json)
                    .collect::<Vec<_>>());
            }
            node
        }
        ContentItem::BlankLineGroup(blg) => {
            json!({
                "type": "BlankLineGroup",
                "count": blg.count,
            })
        }
    }
}

fn annotation_to_json(
    ann: &lex_core::lex::ast::elements::annotation::Annotation,
) -> serde_json::Value {
    use serde_json::json;

    let mut node = json!({
        "type": "Annotation",
        "label": ann.data.label.value,
        "children": ann.children.iter().map(content_item_to_json).collect::<Vec<_>>(),
    });
    if !ann.data.parameters.is_empty() {
        node["parameters"] = json!(ann
            .data
            .parameters
            .iter()
            .map(|p| json!({"key": p.key, "value": p.value}))
            .collect::<Vec<_>>());
    }
    node
}

fn sequence_marker_to_json(
    marker: &lex_core::lex::ast::elements::sequence_marker::SequenceMarker,
) -> serde_json::Value {
    use serde_json::json;

    json!({
        "raw": marker.raw_text.as_string(),
        "style": format!("{}", marker.style),
        "separator": format!("{}", marker.separator),
        "form": format!("{}", marker.form),
    })
}
