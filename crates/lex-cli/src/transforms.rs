//! CLI-specific transforms
//!
//! This module defines all the transform combinations available in the CLI.
//! Each transform is a stage + format combination (e.g., "ast-tag", "token-core-json").
//!
//! ## Transform Pipeline
//!
//! The lex compiler has several processing stages:
//!
//! 1. **Tokenization** - Raw text → Token stream
//!    - `token-core-*`: Core tokens (no semantic indentation)
//!    - `token-line-*`: Line tokens (with semantic indentation)
//!
//! 2. **Parsing** - Tokens → Intermediate Representation (IR)
//!    - `ir-json`: Parse tree representation
//!
//! 3. **Assembly** - IR → Abstract Syntax Tree (AST)
//!    - `ast-tag`: XML-like tag format
//!    - `ast-treeviz`: Tree visualization with Unicode icons
//!    - `ast-json`: JSON representation
//!
//! ## Parameters
//!
//! Transforms accept parameters via CLI flags, config files, or env vars:
//!
//! - `--ast-full`: Shows complete AST including:
//!   * Document-level annotations
//!   * All node properties (labels, subjects, parameters, etc.)
//!   * Session titles, list item markers, definition subjects
//!
//! Example: `lex inspect file.lex ast-tag --ast-full`

use lex_analysis::semantic_tokens::collect_semantic_tokens;
use lex_babel::formats::{
    linetreeviz::to_linetreeviz_str_with_params, nodemap::to_nodemap_str_with_params,
    tag::serialize_document_with_params as serialize_ast_tag_with_params,
    treeviz::to_treeviz_str_with_params,
};
use lex_core::lex::lexing::transformations::line_token_grouping::GroupedTokens;
use lex_core::lex::lexing::transformations::LineTokenGroupingMapper;
use lex_core::lex::loader::DocumentLoader;
use lex_core::lex::token::{to_line_container, LineContainer, LineToken};
use lex_core::lex::transforms::standard::{CORE_TOKENIZATION, LEXING, TO_IR};
use std::collections::HashMap;

/// All available CLI transforms (stage + format combinations)
pub const AVAILABLE_TRANSFORMS: &[&str] = &[
    "token-core-json",
    "token-core-simple",
    "token-core-pprint",
    "token-simple", // alias for token-core-simple
    "token-pprint", // alias for token-core-pprint
    "token-line-json",
    "token-line-simple",
    "token-line-pprint",
    "ir-json",
    "ast-json",
    "ast-tag",
    "ast-treeviz",
    "ast-linetreeviz",
    "ast-nodemap",
    "semantic-tokens",
    "semantic-tokens-json",
    "parity",
];

/// Execute a named transform on a source file with optional extra parameters
///
/// # Arguments
///
/// * `source` - The source text to transform
/// * `transform_name` - The transform to apply (e.g., "ast-tag", "token-core-json")
/// * `extra_params` - Optional parameters for the transform
///
/// # Extra Parameters
///
/// - `ast-full`: "true" - Show complete AST including all node properties
///
/// # Returns
///
/// The transformed output as a string, or an error message
///
/// # Examples
///
/// ```ignore
/// let source = "# Session\n\nContent";
/// let params = HashMap::new();
///
/// // Get tree visualization (default view)
/// let output = execute_transform(source, "ast-treeviz", &params)?;
///
/// // Get complete AST with all properties
/// let mut full_params = HashMap::new();
/// full_params.insert("ast-full".to_string(), "true".to_string());
/// let output = execute_transform(source, "ast-tag", &full_params)?;
/// ```
pub fn execute_transform(
    source: &str,
    transform_name: &str,
    extra_params: &HashMap<String, String>,
) -> Result<String, String> {
    let loader = DocumentLoader::from_string(source);

    // Default show-linum to true for inspect command if not specified
    let mut params = extra_params.clone();
    if !params.contains_key("show-linum") {
        params.insert("show-linum".to_string(), "true".to_string());
    }

    match transform_name {
        "token-core-json" => {
            let tokens = loader
                .with(&CORE_TOKENIZATION)
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(serde_json::to_string_pretty(&tokens_to_json(&tokens))
                .map_err(|e| format!("JSON serialization failed: {e}"))?)
        }
        "token-core-simple" | "token-simple" => {
            let tokens = loader
                .with(&CORE_TOKENIZATION)
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(tokens_to_simple(&tokens))
        }
        "token-core-pprint" | "token-pprint" => {
            let tokens = loader
                .with(&CORE_TOKENIZATION)
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(tokens_to_pprint(&tokens))
        }
        "token-line-json" => {
            let tokens = loader
                .with(&LEXING)
                .map_err(|e| format!("Transform failed: {e}"))?;
            let mut mapper = LineTokenGroupingMapper::new();
            let grouped = mapper.map(tokens);
            let line_tokens: Vec<LineToken> = grouped
                .into_iter()
                .map(GroupedTokens::into_line_token)
                .collect();
            Ok(
                serde_json::to_string_pretty(&line_tokens_to_json(&line_tokens))
                    .map_err(|e| format!("JSON serialization failed: {e}"))?,
            )
        }
        "token-line-simple" => {
            let tokens = loader
                .with(&LEXING)
                .map_err(|e| format!("Transform failed: {e}"))?;
            let mut mapper = LineTokenGroupingMapper::new();
            let grouped = mapper.map(tokens);
            let line_tokens: Vec<LineToken> = grouped
                .into_iter()
                .map(GroupedTokens::into_line_token)
                .collect();
            Ok(line_tokens_to_simple(&line_tokens))
        }
        "token-line-pprint" => {
            let tokens = loader
                .with(&LEXING)
                .map_err(|e| format!("Transform failed: {e}"))?;
            let mut mapper = LineTokenGroupingMapper::new();
            let grouped = mapper.map(tokens);
            let line_tokens: Vec<LineToken> = grouped
                .into_iter()
                .map(GroupedTokens::into_line_token)
                .collect();
            Ok(line_tokens_to_pprint(&line_tokens))
        }
        "ir-json" => {
            let ir = loader
                .with(&TO_IR)
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(serde_json::to_string_pretty(&ir_to_json(&ir))
                .map_err(|e| format!("JSON serialization failed: {e}"))?)
        }
        "ast-json" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(serde_json::to_string_pretty(&ast_to_json(&doc))
                .map_err(|e| format!("JSON serialization failed: {e}"))?)
        }
        "ast-tag" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(serialize_ast_tag_with_params(&doc, &params))
        }
        "ast-treeviz" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            // Supports: --ast-full
            Ok(to_treeviz_str_with_params(&doc, &params))
        }
        "ast-linetreeviz" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            // linetreeviz collapses containers like Paragraph and List
            Ok(to_linetreeviz_str_with_params(&doc, &params))
        }
        "ast-nodemap" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(to_nodemap_str_with_params(&doc, source, &params))
        }
        "semantic-tokens" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(semantic_tokens_to_simple(&doc, source))
        }
        "semantic-tokens-json" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(
                serde_json::to_string_pretty(&semantic_tokens_to_json(&doc, source))
                    .map_err(|e| format!("JSON serialization failed: {e}"))?,
            )
        }
        "parity" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(ast_to_parity(&doc))
        }
        _ => Err(format!("Unknown transform: {transform_name}")),
    }
}

/// Produce a plain-text block skeleton for parity checking against tree-sitter.
///
/// The format is designed so that both the reference parser (lex-core) and the
/// tree-sitter parser can produce identical output with minimal transformation.
/// The reference side (this function) adapts to tree-sitter's natural structure:
/// - Individual BlankLine entries (not collapsed BlankLineGroup)
/// - Verbatim groups emitted as separate subject/content pairs
/// - No inline markup, locations, parameters, or closing labels
fn ast_to_parity(doc: &lex_core::lex::parsing::Document) -> String {
    let mut out = String::new();
    parity_line(&mut out, 0, "Document");
    if let Some(title) = &doc.title {
        parity_line(
            &mut out,
            1,
            &format!("DocumentTitle \"{}\"", title.as_str()),
        );
        if let Some(sub) = title.subtitle_str() {
            parity_line(&mut out, 2, &format!("DocumentSubtitle \"{sub}\""));
        }
    }
    // Document-level annotations (attached to document, not to children)
    for ann in &doc.annotations {
        parity_content_item(
            &mut out,
            1,
            &lex_core::lex::ast::ContentItem::Annotation(ann.clone()),
        );
    }
    for item in doc.root.children.iter() {
        parity_content_item(&mut out, 1, item);
    }
    out
}

fn parity_line(out: &mut String, depth: usize, text: &str) {
    for _ in 0..depth {
        out.push_str("  ");
    }
    out.push_str(text);
    out.push('\n');
}

fn parity_content_item(out: &mut String, depth: usize, item: &lex_core::lex::ast::ContentItem) {
    use lex_core::lex::ast::ContentItem;

    match item {
        ContentItem::Session(s) => {
            parity_line(out, depth, &format!("Session \"{}\"", s.title.as_string()));
            for child in s.children.iter() {
                parity_content_item(out, depth + 1, child);
            }
        }
        ContentItem::Paragraph(p) => {
            parity_line(out, depth, "Paragraph");
            for line in &p.lines {
                parity_content_item(out, depth + 1, line);
            }
        }
        ContentItem::TextLine(tl) => {
            let text = tl.text().trim_end();
            parity_line(out, depth, &format!("\"{text}\""));
        }
        ContentItem::Definition(d) => {
            parity_line(
                out,
                depth,
                &format!("Definition \"{}\"", d.subject.as_string()),
            );
            for child in d.children.iter() {
                parity_content_item(out, depth + 1, child);
            }
        }
        ContentItem::List(l) => {
            parity_line(out, depth, "List");
            for item in l.items.iter() {
                parity_content_item(out, depth + 1, item);
            }
        }
        ContentItem::ListItem(li) => {
            parity_line(
                out,
                depth,
                &format!("ListItem \"{}\"", li.marker.as_string()),
            );
            // First text line (inline with marker)
            if !li.text.is_empty() {
                let text = li.text[0].as_string().trim_end_matches('\n');
                parity_line(out, depth + 1, &format!("\"{text}\""));
            }
            for child in li.children.iter() {
                parity_content_item(out, depth + 1, child);
            }
        }
        ContentItem::VerbatimBlock(fb) => {
            // Emit each group as a separate subject/content sequence
            for group in fb.group() {
                parity_line(
                    out,
                    depth,
                    &format!("VerbatimBlock \"{}\"", group.subject.as_string()),
                );
                for child in group.children.iter() {
                    parity_content_item(out, depth + 1, child);
                }
            }
        }
        ContentItem::VerbatimLine(fl) => {
            parity_line(out, depth, &format!("\"{}\"", fl.content.as_string()));
        }
        ContentItem::Table(t) => {
            // Tree-sitter sees tables as VerbatimBlock — emit as VerbatimBlock for parity
            parity_line(
                out,
                depth,
                &format!("VerbatimBlock \"{}\"", t.subject.as_string()),
            );
            // Table rows become text lines from tree-sitter's perspective
            for row in t.header_rows.iter().chain(t.body_rows.iter()) {
                let cells: Vec<&str> = row.cells.iter().map(|c| c.text()).collect();
                let line = format!("| {} |", cells.join(" | "));
                parity_line(out, depth + 1, &format!("\"{line}\""));
            }
        }
        ContentItem::Annotation(a) => {
            parity_line(
                out,
                depth,
                &format!("Annotation \"{}\"", a.data.label.value),
            );
            for child in a.children.iter() {
                // Skip empty paragraphs (lex-core creates default empty paragraph
                // for marker-only annotations; tree-sitter doesn't)
                if let ContentItem::Paragraph(p) = child {
                    if p.lines.is_empty() {
                        continue;
                    }
                }
                parity_content_item(out, depth + 1, child);
            }
        }
        ContentItem::BlankLineGroup(blg) => {
            // Emit individual BlankLine entries to match tree-sitter's natural output
            for _ in 0..blg.count {
                parity_line(out, depth, "BlankLine");
            }
        }
    }
}

/// Convert tokens to JSON-serializable format
fn tokens_to_json(
    tokens: &[(lex_core::lex::token::Token, std::ops::Range<usize>)],
) -> serde_json::Value {
    use serde_json::json;

    json!(tokens
        .iter()
        .map(|(token, range)| {
            json!({
                "token": format!("{:?}", token),
                "start": range.start,
                "end": range.end,
            })
        })
        .collect::<Vec<_>>())
}

fn tokens_to_simple(tokens: &[(lex_core::lex::token::Token, std::ops::Range<usize>)]) -> String {
    tokens
        .iter()
        .map(|(token, _)| token.simple_name())
        .collect::<Vec<_>>()
        .join("\n")
}

fn tokens_to_pprint(tokens: &[(lex_core::lex::token::Token, std::ops::Range<usize>)]) -> String {
    use lex_core::lex::token::Token;

    let mut output = String::new();
    for (token, _) in tokens {
        output.push_str(token.simple_name());
        output.push('\n');
        if matches!(token, Token::BlankLine(_)) {
            output.push('\n');
        }
    }
    output
}

/// Convert line tokens into a JSON-friendly structure
fn line_tokens_to_json(line_tokens: &[LineToken]) -> serde_json::Value {
    use serde_json::json;

    json!(line_tokens
        .iter()
        .map(|line| {
            json!({
                "line_type": format!("{:?}", line.line_type),
                "tokens": line
                    .source_tokens
                    .iter()
                    .zip(line.token_spans.iter())
                    .map(|(token, span)| {
                        json!({
                            "token": format!("{:?}", token),
                            "start": span.start,
                            "end": span.end,
                        })
                    })
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>())
}

fn line_tokens_to_simple(line_tokens: &[LineToken]) -> String {
    line_tokens
        .iter()
        .map(|line| line.line_type.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_tokens_to_pprint(line_tokens: &[LineToken]) -> String {
    let container = to_line_container::build_line_container(line_tokens.to_vec());
    let mut output = String::new();
    render_line_tree(&container, 0, true, &mut output);
    output
}

fn render_line_tree(node: &LineContainer, depth: usize, is_root: bool, output: &mut String) {
    match node {
        LineContainer::Token(line) => {
            let indent = "  ".repeat(depth);
            output.push_str(&indent);
            output.push_str(&line.line_type.to_string());
            output.push('\n');
        }
        LineContainer::Container { children } => {
            let next_depth = if is_root { depth } else { depth + 1 };
            for child in children {
                render_line_tree(child, next_depth, false, output);
            }
        }
    }
}

/// Convert IR (ParseNode) to JSON-serializable format
fn ir_to_json(node: &lex_core::lex::parsing::ir::ParseNode) -> serde_json::Value {
    use serde_json::json;

    json!({
        "type": format!("{:?}", node.node_type),
        "tokens": tokens_to_json(&node.tokens),
        "children": node.children.iter().map(ir_to_json).collect::<Vec<_>>(),
        "has_payload": node.payload.is_some(),
    })
}

/// Convert AST (Document) to JSON-serializable format.
///
/// Produces a canonical JSON representation of the full AST tree, suitable for
/// parity testing between parsing engines (e.g., current Rust parser vs tree-sitter).
fn ast_to_json(doc: &lex_core::lex::parsing::Document) -> serde_json::Value {
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

/// Format semantic tokens as one line per token:
///   startLine:startCol-endLine:endCol  TokenKind  "text excerpt"
fn semantic_tokens_to_simple(doc: &lex_core::lex::parsing::Document, source: &str) -> String {
    let tokens = collect_semantic_tokens(doc);
    let lines: Vec<&str> = source.lines().collect();
    let mut output = String::new();

    for token in &tokens {
        let start = &token.range.start;
        let end = &token.range.end;

        // Extract text excerpt from source (single-line only for readability)
        let excerpt = if start.line == end.line {
            lines
                .get(start.line)
                .map(|l| {
                    let s = start.column.min(l.len());
                    let e = end.column.min(l.len());
                    &l[s..e]
                })
                .unwrap_or("")
        } else {
            lines
                .get(start.line)
                .map(|l| {
                    let s = start.column.min(l.len());
                    &l[s..]
                })
                .unwrap_or("")
        };

        // 1-based line and column numbers for display
        output.push_str(&format!(
            "{}:{}-{}:{}  {}  \"{}\"\n",
            start.line + 1,
            start.column + 1,
            end.line + 1,
            end.column + 1,
            token.kind.as_str(),
            excerpt.chars().take(60).collect::<String>(),
        ));
    }

    output
}

/// Format semantic tokens as JSON array
fn semantic_tokens_to_json(
    doc: &lex_core::lex::parsing::Document,
    source: &str,
) -> serde_json::Value {
    use serde_json::json;

    let tokens = collect_semantic_tokens(doc);
    let lines: Vec<&str> = source.lines().collect();

    json!(tokens
        .iter()
        .map(|token| {
            let start = &token.range.start;
            let end = &token.range.end;
            let excerpt = if start.line == end.line {
                lines
                    .get(start.line)
                    .map(|l| {
                        let s = start.column.min(l.len());
                        let e = end.column.min(l.len());
                        l[s..e].to_string()
                    })
                    .unwrap_or_default()
            } else {
                lines
                    .get(start.line)
                    .map(|l| {
                        let s = start.column.min(l.len());
                        l[s..].to_string()
                    })
                    .unwrap_or_default()
            };
            json!({
                "kind": token.kind.as_str(),
                "start_line": start.line + 1,
                "start_col": start.column + 1,
                "end_line": end.line + 1,
                "end_col": end.column + 1,
                "text": excerpt,
            })
        })
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_line_transform_emits_line_tokens() {
        let source = "Session:\n    Content\n";
        let extra_params = HashMap::new();
        let output =
            execute_transform(source, "token-line-json", &extra_params).expect("transform to run");

        assert!(output.contains("\"line_type\""));
        assert!(output.contains("SubjectLine"));
        assert!(output.contains("ParagraphLine"));
    }

    #[test]
    fn token_simple_outputs_names() {
        let source = "Session:\n    Content\n";
        let extra_params = HashMap::new();
        let output =
            execute_transform(source, "token-simple", &extra_params).expect("transform to run");

        assert!(output.contains("TEXT"));
        assert!(output.contains("BLANK_LINE"));
    }

    #[test]
    fn token_line_simple_outputs_names() {
        let source = "Session:\n    Content\n";
        let extra_params = HashMap::new();
        let output = execute_transform(source, "token-line-simple", &extra_params)
            .expect("transform to run");

        assert!(output.contains("SUBJECT_LINE"));
        assert!(output.contains("PARAGRAPH_LINE"));
    }

    #[test]
    fn token_pprint_inserts_blank_line() {
        let source = "Hello\n\nWorld\n";
        let extra_params = HashMap::new();
        let output =
            execute_transform(source, "token-pprint", &extra_params).expect("transform to run");

        assert!(output.contains("BLANK_LINE\n\n"));
    }

    #[test]
    fn token_line_pprint_indents_children() {
        let source = "Session:\n    Content\n";
        let extra_params = HashMap::new();
        let output = execute_transform(source, "token-line-pprint", &extra_params)
            .expect("transform to run");

        assert!(output.contains("SUBJECT_LINE"));
        assert!(output.contains("  PARAGRAPH_LINE"));
    }

    #[test]
    fn execute_transform_accepts_extra_params() {
        let source = "# Test\n";
        let mut extra_params = HashMap::new();
        extra_params.insert("all-nodes".to_string(), "true".to_string());
        extra_params.insert("max-depth".to_string(), "5".to_string());

        // Should not error with unknown params
        let result = execute_transform(source, "ast-treeviz", &extra_params);
        assert!(result.is_ok());
    }

    #[test]
    fn ast_full_param_includes_annotations() {
        use lex_babel::formats::treeviz::to_treeviz_str_with_params;
        use lex_core::lex::ast::elements::annotation::Annotation;
        use lex_core::lex::ast::elements::label::Label;
        use lex_core::lex::ast::elements::paragraph::Paragraph;
        use lex_core::lex::ast::elements::typed_content::ContentElement;
        use lex_core::lex::ast::{ContentItem, Document};

        // Create a document with document-level annotation programmatically
        let annotation = Annotation::new(
            Label::new("test-annotation".to_string()),
            vec![],
            Vec::<ContentElement>::new(),
        );
        let doc = Document::with_annotations_and_content(
            vec![annotation],
            vec![ContentItem::Paragraph(Paragraph::from_line(
                "Regular content".to_string(),
            ))],
        );

        let mut extra_params = HashMap::new();

        // Without ast-full, annotations should be excluded from output
        let output_normal = to_treeviz_str_with_params(&doc, &extra_params);
        assert!(
            !output_normal.contains("test-annotation"),
            "Annotation label should not be visible without ast-full"
        );

        // With ast-full=true, annotations should be included
        extra_params.insert("ast-full".to_string(), "true".to_string());
        let output_full = to_treeviz_str_with_params(&doc, &extra_params);
        // The annotation icon is " (double quote character)
        assert!(
            output_full.contains("\" test-annotation"),
            "With ast-full=true, annotation with icon should appear in output. Output was:\n{output_full}"
        );
        assert!(
            output_full.contains("test-annotation"),
            "Annotation label should be visible with ast-full"
        );
    }
}
