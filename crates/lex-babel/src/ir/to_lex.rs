//! Conversion from IR to Lex AST.
//!
//! This module provides functions to convert from the Intermediate Representation
//! back to Lex AST structures.

use lex_core::lex::ast::elements::{
    typed_content, verbatim::VerbatimBlockMode, Annotation as LexAnnotation, ContentElement,
    ContentItem as LexContentItem, Definition as LexDefinition, Label, List as LexList,
    ListItem as LexListItem, Paragraph as LexParagraph, Session as LexSession,
    Verbatim as LexVerbatim, VerbatimContent, VerbatimLine as LexVerbatimLine,
};
use lex_core::lex::ast::range::Position;
use lex_core::lex::ast::{Data, Document as LexDocument, Parameter, Range, TextContent};
use lex_core::lex::includes::{LoadError, LoadedFile, Loader, ResolveConfig};
use lex_extension::wire::{FormatCtx, LexAnnotationOut, WireNode};
use lex_extension::wire::{Position as WirePosition, Range as WireRange};
use lex_extension_host::registry::Registry;
use std::path::Path;
use std::sync::Arc;

use super::nodes::{
    Annotation, Definition, DocNode, Document, Heading, InlineContent, List, ListItem, Paragraph,
    Table, TableCell, TableRow, Verbatim,
};

/// A loader that always reports NotFound. The to_lex direction never
/// resolves includes — it only emits source — so we never actually
/// touch the loader. It exists because
/// `lex_core::lex::builtins::register_into` requires one.
struct NoopLoader;
impl Loader for NoopLoader {
    fn load(&self, path: &Path) -> Result<LoadedFile, LoadError> {
        Err(LoadError::NotFound {
            path: path.to_path_buf(),
        })
    }
}

/// Build a `lex_extension_host::Registry` with the built-in `lex.*`
/// schemas registered, suitable for the to_lex direction (no
/// filesystem access needed). Constructed lazily per call to
/// `to_lex_document` — cheap enough that callers don't need to pass
/// one in.
fn build_to_lex_registry() -> Registry {
    let registry = Registry::new();
    let _ = lex_core::lex::builtins::register_into(
        &registry,
        Arc::new(NoopLoader),
        ResolveConfig::with_root(std::path::PathBuf::from("/")),
    );
    registry
}

fn default_wire_range() -> WireRange {
    WireRange {
        start: WirePosition(0, 0),
        end: WirePosition(0, 0),
    }
}

/// Build a `WireNode::Verbatim` carrying `body` + `params` under the
/// given canonical label. Used to feed `dispatch_format` for the
/// built-in `lex.tabular.*` and `lex.media.*` handlers.
fn wire_verbatim(label: &str, body: String, params: Vec<(String, String)>) -> WireNode {
    let params_json = serde_json::Value::Object(
        params
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect(),
    );
    WireNode::Verbatim {
        range: default_wire_range(),
        origin: None,
        label: label.to_string(),
        params: params_json,
        body_text: body,
        subject: String::new(),
        mode: "inflow".to_string(),
    }
}

/// Build a `LexContentItem::VerbatimBlock` from a `LexAnnotationOut`
/// produced by `Registry::dispatch_format`.
fn lex_verbatim_from_annotation_out(out: LexAnnotationOut) -> LexContentItem {
    let label = Label::new(out.label.clone());
    let parameters: Vec<Parameter> = out
        .params
        .into_iter()
        .map(|(k, v)| Parameter {
            key: k,
            value: v,
            location: default_range(),
        })
        .collect();

    let subject = TextContent::from_string(String::new(), None);
    let lines: Vec<VerbatimContent> = out
        .body
        .lines()
        .map(|l| VerbatimContent::VerbatimLine(LexVerbatimLine::new(l.to_string())))
        .collect();

    let closing_data = Data::new(label, parameters);

    LexContentItem::VerbatimBlock(Box::new(LexVerbatim::new(
        subject,
        lines,
        closing_data,
        VerbatimBlockMode::Inflow,
    )))
}

/// Converts an IR document to a Lex document.
///
/// **`document_annotations` note (#570):** the new IR slot is *not*
/// emitted back into `lex_doc.annotations` on this path. The legacy
/// frontmatter promotion in `from_lex_document` still synthesizes a
/// `frontmatter` annotation into `children`, and downstream Lex
/// serializers expect to see it there — emitting both would
/// double-write. A follow-up to Phase 3b retires the promotion and
/// flips the source-of-truth to `document_annotations` atomically;
/// the [`to_lex_annotation_raw`] helper introduced in Phase 3a is
/// wired into production then. Until that flip,
/// `document_annotations` is a one-way slot: populated by
/// `from_lex_document`, consumed only by embedders that read it
/// directly.
pub fn to_lex_document(doc: &Document) -> LexDocument {
    let mut children = Vec::new();

    for node in &doc.children {
        children.extend(to_lex_content_items(node, 1));
    }

    let mut lex_doc = LexDocument::with_content(children);

    // Restore document title and subtitle from IR
    if let Some(title_inlines) = &doc.title {
        let title_text = inline_content_to_text(title_inlines);
        if !title_text.is_empty() {
            use lex_core::lex::ast::elements::document::DocumentTitle;
            let title_tc = TextContent::from_string(title_text, None);
            let subtitle_tc = doc.subtitle.as_ref().map(|sub_inlines| {
                TextContent::from_string(inline_content_to_text(sub_inlines), None)
            });
            lex_doc.title = Some(match subtitle_tc {
                Some(sub) => DocumentTitle::with_subtitle(title_tc, sub, Range::default()),
                None => DocumentTitle::new(title_tc, Range::default()),
            });
        }
    }

    lex_doc
}

/// Build a `LexAnnotation` directly from an IR `Annotation`, mirroring
/// what [`to_lex_annotation`] does but without wrapping the result in
/// [`LexContentItem::Annotation`].
///
/// Shipped in Phase 3a (#570) ready for Phase 3b to wire it into
/// [`to_lex_document`]: when that PR retires the legacy frontmatter
/// promotion, `to_lex_document` will iterate `document_annotations`
/// through this helper to populate `lex_doc.annotations`. Until then
/// it is dead code in production, exercised only by the Phase-3a
/// contract test that confirms the helper produces the expected
/// shape.
#[allow(dead_code)]
fn to_lex_annotation_raw(ann: &Annotation) -> LexAnnotation {
    let label = Label::new(ann.label.clone());
    let parameters: Vec<Parameter> = ann
        .parameters
        .iter()
        .map(|(k, v)| Parameter {
            key: k.clone(),
            value: v.clone(),
            location: default_range(),
        })
        .collect();

    let mut child_items = Vec::new();
    for child in &ann.content {
        child_items.extend(to_lex_content_items(child, 1));
    }

    let children = to_content_elements(child_items);
    LexAnnotation::new(label, parameters, children)
}

/// Converts an IR DocNode to one or more Lex ContentItems.
///
/// Some IR nodes may expand to multiple ContentItems (e.g., a Heading with children
/// becomes a Session with nested content).
fn to_lex_content_items(node: &DocNode, level: usize) -> Vec<LexContentItem> {
    match node {
        DocNode::Document(_) => {
            // Document should only appear at root, not recursively
            vec![]
        }
        DocNode::Heading(heading) => vec![to_lex_session(heading, level)],
        DocNode::Paragraph(para) => vec![to_lex_paragraph(para)],
        DocNode::List(list) => vec![to_lex_list(list)],
        DocNode::ListItem(item) => vec![to_lex_list_item(item)],
        DocNode::Definition(def) => vec![to_lex_definition(def)],
        DocNode::Verbatim(verb) => vec![to_lex_verbatim(verb)],
        DocNode::Annotation(ann) => vec![to_lex_annotation(ann, level)],
        DocNode::Table(table) => vec![to_lex_table(table, level)],
        DocNode::Image(_) | DocNode::Video(_) | DocNode::Audio(_) => vec![to_lex_media(node)],
        DocNode::Inline(_) => {
            // Inline content should not appear at block level
            vec![]
        }
    }
}

fn to_lex_session(heading: &Heading, level: usize) -> LexContentItem {
    let title_text = inline_content_to_text(&heading.content);
    let title = TextContent::from_string(title_text, None);

    let mut children = Vec::new();
    for child in &heading.children {
        children.extend(to_lex_content_items(child, level + 1));
    }

    // Convert ContentItem to SessionContent
    let session_children = typed_content::into_session_contents(children);

    LexContentItem::Session(LexSession::new(title, session_children))
}

/// Converts an IR Table to a Lex VerbatimBlock via the verbatim registry,
/// falling back to a nested Annotation structure if the registry has no handler.
fn to_lex_table(table: &Table, level: usize) -> LexContentItem {
    // Serialize the typed IR Table into pipe-table source. The
    // serialization logic still lives in
    // `crate::common::verbatim::table::serialize_pipe_table`, but
    // we now feed the result through `Registry::dispatch_format` so
    // the built-in `lex.tabular.table` handler (#570 Phase 4b) owns
    // the IR→Lex contract.
    let body = crate::common::verbatim::table::serialize_pipe_table(table);
    let registry = build_to_lex_registry();
    let ctx = FormatCtx {
        label: "lex.tabular.table".to_string(),
        params: Vec::new(),
        node: wire_verbatim("lex.tabular.table", body, Vec::new()),
        format_options: None,
    };
    if let Ok(Some(out)) = registry.dispatch_format(&ctx) {
        return lex_verbatim_from_annotation_out(out);
    }

    // Fallback to annotation if dispatch_format somehow fails (the
    // built-in handler must always return Some for the canonical
    // verbatim labels, so this path should be unreachable in
    // production — kept defensive against future handler changes).
    let label = Label::new("table".to_string());
    let parameters = Vec::new(); // Could add caption here if needed

    let mut children = Vec::new();

    // Header
    if !table.header.is_empty() {
        let thead_label = Label::new("thead".to_string());
        let mut thead_rows = Vec::new();
        for row in &table.header {
            thead_rows.push(to_lex_table_row(row, level + 1));
        }
        let thead = LexContentItem::Annotation(LexAnnotation::new(
            thead_label,
            Vec::new(),
            to_content_elements(thead_rows),
        ));
        children.push(thead);
    }

    // Body (rows)
    let tbody_label = Label::new("tbody".to_string());
    let mut tbody_rows = Vec::new();
    for row in &table.rows {
        tbody_rows.push(to_lex_table_row(row, level + 1));
    }
    let tbody = LexContentItem::Annotation(LexAnnotation::new(
        tbody_label,
        Vec::new(),
        to_content_elements(tbody_rows),
    ));
    children.push(tbody);

    LexContentItem::Annotation(LexAnnotation::new(
        label,
        parameters,
        to_content_elements(children),
    ))
}

fn to_lex_table_row(row: &TableRow, level: usize) -> LexContentItem {
    let label = Label::new("tr".to_string());
    let mut cells = Vec::new();
    for cell in &row.cells {
        cells.push(to_lex_table_cell(cell, level + 1));
    }
    LexContentItem::Annotation(LexAnnotation::new(
        label,
        Vec::new(),
        to_content_elements(cells),
    ))
}

fn to_lex_table_cell(cell: &TableCell, level: usize) -> LexContentItem {
    let label_str = if cell.header { "th" } else { "td" };
    let label = Label::new(label_str.to_string());

    let mut parameters = Vec::new();
    // Handle alignment
    let align_val = match cell.align {
        crate::ir::nodes::TableCellAlignment::Left => Some("left"),
        crate::ir::nodes::TableCellAlignment::Center => Some("center"),
        crate::ir::nodes::TableCellAlignment::Right => Some("right"),
        crate::ir::nodes::TableCellAlignment::None => None,
    };
    if let Some(align) = align_val {
        parameters.push(Parameter {
            key: "align".to_string(),
            value: align.to_string(),
            location: default_range(),
        });
    }

    let mut content = Vec::new();
    for child in &cell.content {
        content.extend(to_lex_content_items(child, level + 1));
    }

    LexContentItem::Annotation(LexAnnotation::new(
        label,
        parameters,
        to_content_elements(content),
    ))
}

/// Converts an IR Paragraph to a Lex Paragraph.
fn to_lex_paragraph(para: &Paragraph) -> LexContentItem {
    let text = inline_content_to_text(&para.content);
    LexContentItem::Paragraph(LexParagraph::from_line(text))
}

/// Converts an IR List to a Lex List.
///
/// Derives marker text for each item from the List's style and the item's position.
fn to_lex_list(list: &List) -> LexContentItem {
    let items: Vec<LexListItem> = list
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| to_lex_list_item_with_style(item, &list.style, i + 1))
        .collect();
    LexContentItem::List(LexList::new(items))
}

/// Converts an IR ListItem to a Lex ListItem with a marker derived from style and position.
fn to_lex_list_item_with_style(
    item: &ListItem,
    style: &super::nodes::ListStyle,
    index: usize,
) -> LexListItem {
    let marker = format_marker_for_style(style, index);
    let text = inline_content_to_text(&item.content);

    let mut child_items = Vec::new();
    for child in &item.children {
        child_items.extend(to_lex_content_items(child, 1));
    }

    let children = to_content_elements(child_items);
    LexListItem::with_content(marker, text, children)
}

/// Formats a marker string from a ListStyle and 1-based index.
fn format_marker_for_style(style: &super::nodes::ListStyle, index: usize) -> String {
    use super::nodes::ListStyle;
    match style {
        ListStyle::Bullet => "-".to_string(),
        ListStyle::Numeric => format!("{index}."),
        ListStyle::AlphaLower => {
            let c = if (1..=26).contains(&index) {
                char::from_u32((index as u32) + 96).unwrap()
            } else {
                return format!("{index}.");
            };
            format!("{c}.")
        }
        ListStyle::AlphaUpper => {
            let c = if (1..=26).contains(&index) {
                char::from_u32((index as u32) + 64).unwrap()
            } else {
                return format!("{index}.");
            };
            format!("{c}.")
        }
        ListStyle::RomanLower => {
            let roman = to_roman_lower(index);
            format!("{roman}.")
        }
        ListStyle::RomanUpper => {
            let roman = to_roman_upper(index);
            format!("{roman}.")
        }
    }
}

fn to_roman_lower(n: usize) -> String {
    to_roman_upper(n).to_lowercase()
}

fn to_roman_upper(n: usize) -> String {
    match n {
        1 => "I",
        2 => "II",
        3 => "III",
        4 => "IV",
        5 => "V",
        6 => "VI",
        7 => "VII",
        8 => "VIII",
        9 => "IX",
        10 => "X",
        11 => "XI",
        12 => "XII",
        13 => "XIII",
        14 => "XIV",
        15 => "XV",
        16 => "XVI",
        17 => "XVII",
        18 => "XVIII",
        19 => "XIX",
        20 => "XX",
        _ => return n.to_string(),
    }
    .to_string()
}

/// Converts an IR ListItem to a ContentItem::ListItem.
fn to_lex_list_item(item: &ListItem) -> LexContentItem {
    LexContentItem::ListItem(to_lex_list_item_struct(item))
}

/// Converts an IR ListItem to a Lex ListItem struct.
///
/// The marker is derived from the parent List's style/form and the item's
/// position, not from the item's inline content.
fn to_lex_list_item_struct(item: &ListItem) -> LexListItem {
    // Default marker — callers should use to_lex_list which provides proper markers
    let marker = "-".to_string();
    let text = inline_content_to_text(&item.content);

    let mut child_items = Vec::new();
    for child in &item.children {
        child_items.extend(to_lex_content_items(child, 1));
    }

    let children = to_content_elements(child_items);
    LexListItem::with_content(marker, text, children)
}

/// Converts an IR Definition to a Lex Definition.
fn to_lex_definition(def: &Definition) -> LexContentItem {
    let term_text = inline_content_to_text(&def.term);
    let term = TextContent::from_string(term_text, None);

    let mut child_items = Vec::new();
    for child in &def.description {
        child_items.extend(to_lex_content_items(child, 1));
    }

    let children = to_content_elements(child_items);
    LexContentItem::Definition(LexDefinition::new(term, children))
}

/// Converts an IR Verbatim to a Lex Verbatim block.
fn to_lex_verbatim(verb: &Verbatim) -> LexContentItem {
    let subject_text = verb.subject.clone().unwrap_or_default();
    let subject = TextContent::from_string(subject_text, None);

    // Split content into lines and create VerbatimLine items
    let lines: Vec<VerbatimContent> = verb
        .content
        .lines()
        .map(|line| VerbatimContent::VerbatimLine(LexVerbatimLine::new(line.to_string())))
        .collect();

    // Create closing data with language label
    let label_text = verb.language.clone().unwrap_or_default();
    let label = Label::new(label_text);
    let closing_data = Data::new(label, Vec::new());

    LexContentItem::VerbatimBlock(Box::new(LexVerbatim::new(
        subject,
        lines,
        closing_data,
        VerbatimBlockMode::Inflow,
    )))
}

/// Converts an IR Annotation to a Lex Annotation.
fn to_lex_annotation(ann: &Annotation, level: usize) -> LexContentItem {
    let label = Label::new(ann.label.clone());
    let parameters: Vec<Parameter> = ann
        .parameters
        .iter()
        .map(|(k, v)| Parameter {
            key: k.clone(),
            value: v.clone(),
            location: default_range(),
        })
        .collect();

    let mut child_items = Vec::new();
    for child in &ann.content {
        child_items.extend(to_lex_content_items(child, level));
    }

    let children = to_content_elements(child_items);
    LexContentItem::Annotation(LexAnnotation::new(label, parameters, children))
}

/// Converts IR inline content to plain text string.
///
/// This is a lossy conversion that flattens all inline formatting.
fn inline_content_to_text(content: &[InlineContent]) -> String {
    content
        .iter()
        .map(|inline| match inline {
            InlineContent::Text(text) => text.clone(),
            InlineContent::Bold(children) => {
                format!("*{}*", inline_content_to_text(children))
            }
            InlineContent::Italic(children) => {
                format!("_{}_", inline_content_to_text(children))
            }
            InlineContent::Code(code) => format!("`{code}`"),
            InlineContent::Math(math) => format!("#{math}#"),
            InlineContent::Reference(ref_text) => format!("[{ref_text}]"),
            InlineContent::Link { text, href } => format!("{text} [{href}]"),
            InlineContent::Image(image) => {
                let mut text = format!("![{}]({})", image.alt, image.src);
                if let Some(title) = &image.title {
                    text.push_str(&format!(" \"{title}\""));
                }
                text
            }
        })
        .collect()
}

/// Converts ContentItem to ContentElement, filtering out Sessions and ListItems
fn to_content_elements(items: Vec<LexContentItem>) -> Vec<ContentElement> {
    items
        .into_iter()
        .filter_map(|item| item.try_into().ok())
        .collect()
}

/// Helper to create a default Range
fn default_range() -> Range {
    Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
}

fn to_lex_media(node: &DocNode) -> LexContentItem {
    // Each media node serializes to a verbatim with the (src, alt|title|
    // poster, …) params canonical to its kind. Extract the params from
    // the typed IR node, then dispatch through the registry to let the
    // built-in `lex.media.*` handler (#570 Phase 4b) produce the
    // LexAnnotationOut.
    let (label, params) = match node {
        DocNode::Image(image) => {
            let mut params = Vec::new();
            params.push(("src".to_string(), image.src.clone()));
            if !image.alt.is_empty() {
                params.push(("alt".to_string(), image.alt.clone()));
            }
            if let Some(title) = &image.title {
                params.push(("title".to_string(), title.clone()));
            }
            ("lex.media.image", params)
        }
        DocNode::Video(video) => {
            let mut params = Vec::new();
            params.push(("src".to_string(), video.src.clone()));
            if let Some(title) = &video.title {
                params.push(("title".to_string(), title.clone()));
            }
            if let Some(poster) = &video.poster {
                params.push(("poster".to_string(), poster.clone()));
            }
            ("lex.media.video", params)
        }
        DocNode::Audio(audio) => {
            let mut params = Vec::new();
            params.push(("src".to_string(), audio.src.clone()));
            if let Some(title) = &audio.title {
                params.push(("title".to_string(), title.clone()));
            }
            ("lex.media.audio", params)
        }
        _ => return LexContentItem::Paragraph(LexParagraph::new(vec![])),
    };

    let registry = build_to_lex_registry();
    let ctx = FormatCtx {
        label: label.to_string(),
        params: params.clone(),
        node: wire_verbatim(label, String::new(), params),
        format_options: None,
    };
    if let Ok(Some(out)) = registry.dispatch_format(&ctx) {
        return lex_verbatim_from_annotation_out(out);
    }

    LexContentItem::Paragraph(LexParagraph::new(vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::nodes::*;

    #[test]
    fn test_paragraph_to_lex() {
        let ir_para = Paragraph {
            content: vec![InlineContent::Text("Hello world".to_string())],
        };

        let lex_item = to_lex_paragraph(&ir_para);

        match lex_item {
            LexContentItem::Paragraph(para) => {
                assert_eq!(para.text(), "Hello world");
            }
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_heading_to_session() {
        let ir_heading = Heading {
            level: 1,
            content: vec![InlineContent::Text("Test".to_string())],
            children: vec![],
        };

        let lex_item = to_lex_session(&ir_heading, 1);

        match lex_item {
            LexContentItem::Session(session) => {
                assert!(session.title.as_string().contains("Test"));
            }
            _ => panic!("Expected Session"),
        }
    }

    #[test]
    fn test_list_to_lex() {
        let ir_list = List {
            items: vec![
                ListItem {
                    content: vec![InlineContent::Text("Item 1".to_string())],
                    children: vec![],
                },
                ListItem {
                    content: vec![InlineContent::Text("Item 2".to_string())],
                    children: vec![],
                },
            ],
            ordered: false,
            style: ListStyle::Bullet,
            form: ListForm::Short,
        };

        let lex_item = to_lex_list(&ir_list);

        match lex_item {
            LexContentItem::List(list) => {
                // Lists contain ListItem children
                assert!(!list.items.is_empty());
            }
            _ => panic!("Expected List"),
        }
    }

    #[test]
    fn test_verbatim_with_language() {
        let ir_verb = Verbatim {
            subject: None,
            language: Some("rust".to_string()),
            content: "fn main() {}\nlet x = 1;".to_string(),
        };

        let lex_item = to_lex_verbatim(&ir_verb);

        match lex_item {
            LexContentItem::VerbatimBlock(verb) => {
                assert_eq!(verb.closing_data.label.value, "rust");
                // Should have 2 lines
                assert_eq!(verb.children.len(), 2);
            }
            _ => panic!("Expected VerbatimBlock"),
        }
    }

    #[test]
    fn test_inline_formatting_to_text() {
        let content = vec![
            InlineContent::Text("Plain ".to_string()),
            InlineContent::Bold(vec![InlineContent::Text("bold".to_string())]),
            InlineContent::Text(" ".to_string()),
            InlineContent::Italic(vec![InlineContent::Text("italic".to_string())]),
            InlineContent::Text(" ".to_string()),
            InlineContent::Code("code".to_string()),
        ];

        let text = inline_content_to_text(&content);

        assert!(text.contains("Plain"));
        assert!(text.contains("*bold*"));
        assert!(text.contains("_italic_"));
        assert!(text.contains("`code`"));
    }

    #[test]
    fn test_round_trip_paragraph() {
        use crate::{from_ir, to_ir};
        use lex_core::lex::ast::ContentItem;
        use lex_core::lex::ast::Document as LexDocument;

        // Create a Lex document with a paragraph
        let original_lex = LexDocument::with_content(vec![ContentItem::Paragraph(
            LexParagraph::from_line("Test content".to_string()),
        )]);

        // Convert to IR
        let ir_doc = to_ir(&original_lex);

        // Convert back to Lex
        let back_to_lex = from_ir(&ir_doc);

        // Check the content is preserved
        assert!(!back_to_lex.root.children.is_empty());
    }

    #[test]
    fn test_full_document_to_lex() {
        let ir_doc = Document {
            title: None,
            subtitle: None,
            children: vec![
                DocNode::Paragraph(Paragraph {
                    content: vec![InlineContent::Text("First paragraph".to_string())],
                }),
                DocNode::Paragraph(Paragraph {
                    content: vec![InlineContent::Text("Second paragraph".to_string())],
                }),
            ],
            document_annotations: vec![],
        };

        let lex_doc = to_lex_document(&ir_doc);

        // Document should have root session with our content
        assert!(!lex_doc.root.children.is_empty());
    }

    #[test]
    fn to_lex_document_drops_document_annotations_in_phase_3a() {
        // Phase 3a of #570 is *additive*. `document_annotations` is
        // populated on the lex → IR direction (via `from_lex_document`)
        // but consumed only by embedders that read the slot directly;
        // `to_lex_document` deliberately ignores it, because the
        // legacy frontmatter promotion that runs *before* this stage
        // already synthesizes a `frontmatter` annotation into
        // `children`, and downstream Lex serializers expect exactly
        // one copy. Phase 3b retires the legacy promotion and wires
        // `to_lex_annotation_raw` into `to_lex_document` atomically.
        // This test locks the Phase 3a behaviour so a premature flip
        // can't sneak in.
        use crate::ir::nodes::Annotation;

        let ir_doc = Document {
            title: None,
            subtitle: None,
            children: vec![],
            document_annotations: vec![Annotation {
                label: "lex.metadata.author".to_string(),
                parameters: vec![("name".to_string(), "Alice".to_string())],
                content: vec![],
            }],
        };

        let lex_doc = to_lex_document(&ir_doc);
        assert!(
            lex_doc.annotations.is_empty(),
            "Phase 3a contract: to_lex_document leaves lex_doc.annotations empty"
        );

        // Sanity: the helper exists and produces a non-empty result —
        // Phase 3b uses it.
        let raw = to_lex_annotation_raw(&ir_doc.document_annotations[0]);
        assert_eq!(raw.data.label.value, "lex.metadata.author");
    }
}
