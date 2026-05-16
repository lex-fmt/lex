//! Conversion from Lex AST to the format-agnostic IR.
//!
//! Pipeline: Lex AST (Document) → IR (ir::nodes::Document)
//!
//! This is the entry point for all outbound format conversions. The IR strips
//! source-level details (positions, blank line groups, token info) to produce
//! a clean semantic representation that any format serializer can consume.
//!
//! Level mapping: root session children start at heading level 2 (the document
//! title occupies level 1). Each nested session increments the level.
//!
//! Verbatim blocks with built-in labels (`lex.tabular.table`,
//! `lex.media.{image,video,audio}`) are hydrated into first-class IR
//! nodes (Table, Image, Video, Audio) via the extension registry's
//! `on_resolve` dispatch (#583) — see [`from_lex_verbatim`].

use lex_core::lex::ast::elements::{
    inlines::InlineNode, Annotation as LexAnnotation, ContentItem as LexContentItem,
    Definition as LexDefinition, Document as LexDocument, List as LexList, ListItem as LexListItem,
    Paragraph as LexParagraph, Session as LexSession, TextLine as LexTextLine,
    Verbatim as LexVerbatim, VerbatimLine as LexVerbatimLine,
};
use lex_core::lex::ast::TextContent;
use lex_core::lex::wire::{origin_string, range_to_wire};
use lex_extension::wire::{AnnotationBody, HostNodeKind, LabelCtx, NodeRef, WireNode};
use lex_extension_host::registry::Registry;

use super::nodes::{
    Annotation, Audio, Definition, DocNode, Document, Heading, Image, InlineContent, List,
    ListForm, ListItem, ListStyle, Paragraph, Verbatim, Video,
};

/// Converts a lex document to the IR.
///
/// `registry` is the extension registry used to dispatch verbatim
/// labels through their `on_resolve` hooks (#583 — required for
/// `lex.tabular.table` to produce a typed `DocNode::Table` and for
/// `lex.media.{image,video,audio}` to participate in the IR
/// construction). Callers that only need the built-in `lex.*`
/// namespaces can pass [`default_registry()`]; CLI / LSP callers
/// that boot a registry with third-party namespaces pass theirs.
///
/// Post-refac/label cleanup: the legacy frontmatter promotion (which
/// scanned `children` for `lex.metadata.*` annotations and synthesized
/// a single `frontmatter` IR Annotation into `children[0]`) is retired.
/// Document-scope metadata now lives exclusively in
/// `Document::document_annotations`, populated from the lex-core
/// `doc.annotations` slot.
///
/// Phase 3b (#614): the IR slot is also the single source of truth
/// on the way back out. `to_lex_document` emits each entry into
/// `lex_doc.annotations` via `to_lex_annotation_raw`, and the
/// `frontmatter` event synthesis at the events-emission layer is
/// retired — format-specific serializers that need a packed YAML
/// preamble (currently just markdown) read `document_annotations`
/// from the IR directly.
///
/// **Behavioural break** (documented in CHANGELOG): an inline
/// `:: lex.metadata.title :: ...` in the document body is no longer
/// promoted to document metadata. Inline annotations stay inline;
/// document-scope metadata must be attached at the document level
/// (lex-core's `doc.annotations` slot).
pub fn from_lex_document(doc: &LexDocument, registry: &Registry) -> Document {
    // Extract document title and subtitle
    let title = doc
        .title
        .as_ref()
        .map(|t| convert_inline_content(&t.content));
    let subtitle = doc
        .title
        .as_ref()
        .and_then(|t| t.subtitle.as_ref())
        .map(convert_inline_content);

    let children = convert_children(&doc.root.children, 2, registry);

    let document_annotations = doc
        .annotations
        .iter()
        .map(|a| ir_annotation_from_lex(a, registry))
        .collect();

    Document {
        title,
        subtitle,
        children,
        document_annotations,
    }
}

/// Build an IR `Annotation` directly from a lex-core annotation, without
/// the `DocNode` enum wrapper that [`from_lex_annotation`] returns. Used
/// by [`from_lex_document`] to populate `Document::document_annotations`.
fn ir_annotation_from_lex(annotation: &LexAnnotation, registry: &Registry) -> Annotation {
    let label = annotation.data.label.value.clone();
    let form = annotation.data.label.form;
    let parameters = annotation
        .data
        .parameters
        .iter()
        .map(|p| (p.key.clone(), p.value.clone()))
        .collect();
    let content = convert_children(&annotation.children, 2, registry);
    Annotation {
        label,
        parameters,
        content,
        form,
    }
}

/// Helper: Converts a list of content items, filtering out blank lines
/// Also extracts annotations attached to each element
fn convert_children(items: &[LexContentItem], level: usize, registry: &Registry) -> Vec<DocNode> {
    items
        .iter()
        .filter(|item| !matches!(item, LexContentItem::BlankLineGroup(_)))
        .flat_map(|item| {
            let mut nodes = extract_attached_annotations(item, level, registry);
            nodes.push(from_lex_content_item_with_level(item, level, registry));
            nodes
        })
        .collect()
}

/// Extracts annotations attached to a content item and converts them to IR nodes
fn extract_attached_annotations(
    item: &LexContentItem,
    level: usize,
    registry: &Registry,
) -> Vec<DocNode> {
    let annotations = match item {
        LexContentItem::Session(session) => session.annotations(),
        LexContentItem::Paragraph(paragraph) => paragraph.annotations(),
        LexContentItem::List(list) => list.annotations(),
        LexContentItem::ListItem(list_item) => list_item.annotations(),
        LexContentItem::Definition(definition) => definition.annotations(),
        LexContentItem::VerbatimBlock(verbatim) => verbatim.annotations(),
        LexContentItem::Table(table) => table.annotations(),
        _ => &[],
    };

    annotations
        .iter()
        .map(|anno| from_lex_annotation(anno, level, registry))
        .collect()
}

/// Converts TextContent to IR InlineContent, resolving implicit anchors for linkable references.
fn convert_inline_content(text: &TextContent) -> Vec<InlineContent> {
    use crate::common::links::resolve_implicit_anchors;

    // Get inline items from TextContent
    let inline_items = text.inline_items();

    let content = if inline_items.is_empty() {
        // If no inline items, use raw string
        vec![InlineContent::Text(text.as_string().to_string())]
    } else {
        inline_items.iter().map(convert_inline_node).collect()
    };

    resolve_implicit_anchors(content)
}

/// Converts a single InlineNode to IR InlineContent
fn convert_inline_node(node: &InlineNode) -> InlineContent {
    match node {
        InlineNode::Plain { text, .. } => InlineContent::Text(text.clone()),
        InlineNode::Strong { content, .. } => {
            InlineContent::Bold(content.iter().map(convert_inline_node).collect())
        }
        InlineNode::Emphasis { content, .. } => {
            InlineContent::Italic(content.iter().map(convert_inline_node).collect())
        }
        InlineNode::Code { text, .. } => InlineContent::Code(text.clone()),
        InlineNode::Math { text, .. } => InlineContent::Math(text.clone()),
        InlineNode::Reference { data, .. } => InlineContent::Reference(data.raw.clone()),
    }
}

/// Converts a lex content item to an IR node with a given level.
fn from_lex_content_item_with_level(
    item: &LexContentItem,
    level: usize,
    registry: &Registry,
) -> DocNode {
    match item {
        LexContentItem::Session(session) => from_lex_session(session, level, registry),
        LexContentItem::Paragraph(paragraph) => from_lex_paragraph(paragraph),
        LexContentItem::List(list) => from_lex_list(list, level, registry),
        LexContentItem::ListItem(list_item) => from_lex_list_item(list_item, level, registry),
        LexContentItem::Definition(definition) => from_lex_definition(definition, level, registry),
        LexContentItem::VerbatimBlock(verbatim) => from_lex_verbatim(verbatim, registry),
        LexContentItem::Table(table) => from_lex_table(table, registry),
        LexContentItem::Annotation(annotation) => from_lex_annotation(annotation, level, registry),
        LexContentItem::TextLine(text_line) => from_lex_text_line(text_line),
        LexContentItem::VerbatimLine(verbatim_line) => from_lex_verbatim_line(verbatim_line),
        LexContentItem::BlankLineGroup(_) => {
            // Blank lines are filtered out by convert_children, but handle gracefully if encountered
            DocNode::Paragraph(Paragraph { content: vec![] })
        }
    }
}

/// Converts a lex session to an IR heading.
///
/// Session markers (e.g. "1." in "1. Introduction") are part of the author's
/// title text and are preserved as regular `InlineContent::Text` — not as a
/// separate structural variant. The full title text (including any numbering
/// prefix) is kept in `Heading.content`.
fn from_lex_session(session: &LexSession, level: usize, registry: &Registry) -> DocNode {
    let content = convert_inline_content(&session.title);

    let children = convert_children(&session.children, level + 1, registry);
    DocNode::Heading(Heading {
        level,
        content,
        children,
    })
}

/// Converts a lex paragraph to an IR paragraph.
fn from_lex_paragraph(paragraph: &LexParagraph) -> DocNode {
    // Paragraphs have multiple lines, each is a TextLine with TextContent
    let mut content = Vec::new();
    for line_item in &paragraph.lines {
        if let LexContentItem::TextLine(text_line) = line_item {
            content.extend(convert_inline_content(&text_line.content));
            // Add newline between lines except for the last line
            if line_item != paragraph.lines.last().unwrap() {
                content.push(InlineContent::Text("\n".to_string()));
            }
        }
    }
    DocNode::Paragraph(Paragraph { content })
}

/// Converts a lex list to an IR list.
fn from_lex_list(list: &LexList, level: usize, registry: &Registry) -> DocNode {
    let items: Vec<ListItem> = list
        .items
        .iter()
        .filter_map(|item| {
            if let LexContentItem::ListItem(li) = item {
                Some(convert_list_item(li, level, registry))
            } else {
                None
            }
        })
        .collect();

    // Detect list style from the first item's marker
    let style = if let Some(LexContentItem::ListItem(li)) = list.items.first() {
        detect_list_style(&li.marker)
    } else {
        ListStyle::Bullet
    };
    let ordered = style.is_ordered();

    // Detect form from the list's SequenceMarker
    let form = list
        .marker
        .as_ref()
        .map(|m| match m.form {
            lex_core::lex::ast::elements::sequence_marker::Form::Extended => ListForm::Extended,
            lex_core::lex::ast::elements::sequence_marker::Form::Short => ListForm::Short,
        })
        .unwrap_or(ListForm::Short);

    DocNode::List(List {
        items,
        ordered,
        style,
        form,
    })
}

/// Converts a lex list item to an IR list item node.
fn from_lex_list_item(list_item: &LexListItem, level: usize, registry: &Registry) -> DocNode {
    DocNode::ListItem(convert_list_item(list_item, level, registry))
}

/// Converts a lex list item to an IR list item struct.
///
/// List markers are structural (captured by `List.style` and `List.form` on the
/// parent) and are not included in the item's inline content.
fn convert_list_item(list_item: &LexListItem, level: usize, registry: &Registry) -> ListItem {
    let mut content = Vec::new();
    for text_content in &list_item.text {
        content.extend(convert_inline_content(text_content));
    }
    let children = convert_children(&list_item.children, level, registry);
    ListItem { content, children }
}

/// Converts a lex definition to an IR definition.
fn from_lex_definition(definition: &LexDefinition, level: usize, registry: &Registry) -> DocNode {
    let term = convert_inline_content(&definition.subject);
    let description = convert_children(&definition.children, level, registry);
    DocNode::Definition(Definition { term, description })
}

/// Converts a lex verbatim block to an IR verbatim block.
///
/// #615 unified registry surface: dispatches through
/// [`Registry::dispatch_ir_build`] (the IR-construction lifecycle hook)
/// rather than the pre-#615 `dispatch_resolve` path. The built-in
/// `lex.tabular.table` and `lex.media.{image,video,audio}` handlers
/// parse the verbatim into a typed `WireNode` (`Table` / `Image` /
/// `Video` / `Audio` per `wire_version: 2`); the typed wire output
/// converts to IR directly via [`from_wire_typed`], without a
/// wire-→-lex-core-AST round-trip and without dispatching on the
/// label string a second time. Third-party namespaces that register
/// a verbatim handler with `on_ir_build` participate the same way.
///
/// Falls back to a generic `DocNode::Verbatim` when no handler is
/// registered for the label, when the handler returns `Ok(None)`, or
/// when the returned wire kind isn't one this builder knows how to
/// type (third-party verbatim labels without an IR-build hook,
/// unrecognised labels, future wire variants).
fn from_lex_verbatim(verbatim: &LexVerbatim, registry: &Registry) -> DocNode {
    let subject_str = verbatim.subject.as_string();
    let subject = if subject_str.is_empty() {
        None
    } else {
        Some(subject_str.to_string())
    };
    let language = Some(verbatim.closing_data.label.value.clone());
    let content = verbatim
        .children
        .iter()
        .map(|item| {
            if let LexContentItem::VerbatimLine(vl) = item {
                vl.content.as_string().to_string()
            } else {
                "".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Build a LabelCtx from the parsed-verbatim payload (label + params
    // + body) and fire the IR-build hook. The handler returns a typed
    // WireNode directly; we convert to IR by switching on the wire
    // variant — no re-dispatch on the label string.
    let label = verbatim.closing_data.label.value.clone();
    let params_object = serde_json::Value::Object(
        verbatim
            .closing_data
            .parameters
            .iter()
            .map(|p| (p.key.clone(), serde_json::Value::String(p.value.clone())))
            .collect(),
    );
    let ctx = LabelCtx {
        label,
        params: params_object,
        body: AnnotationBody::Text(content.clone()),
        node: NodeRef {
            kind: HostNodeKind::Verbatim.as_str().into(),
            range: range_to_wire(&verbatim.location),
            origin: origin_string(&verbatim.location),
        },
    };
    if let Ok(Some(mut wire_node)) = registry.dispatch_ir_build(&ctx) {
        // The subject line on a verbatim is part of the host's context,
        // not the hydrated wire payload — built-in IR-build handlers
        // (and well-behaved third-party ones) read params + body only.
        // Restore it host-side so downstream renderers have a default
        // caption / title / alt when the source carried a subject but
        // the handler emitted an empty value.
        if let Some(s) = subject.as_deref() {
            if !s.is_empty() {
                inject_subject_into_wire_node(&mut wire_node, s);
            }
        }
        if let Some(node) = from_wire_typed(&wire_node, registry, &content) {
            return node;
        }
    }

    DocNode::Verbatim(Verbatim {
        subject,
        language,
        content,
    })
}

/// Restore the verbatim subject as a default caption / title / alt on
/// hydrated media + tabular wire nodes when the handler emitted an
/// empty field. Built-in IR-build handlers read `ctx.params` + body
/// only and have no visibility into the subject line — without this
/// injection a `.lex` source like
///
/// ```text
/// Sunset Photo:
///     ...
/// :: image src=sunset.jpg ::
/// ```
///
/// loses `"Sunset Photo"` in the resulting `WireNode::Image`. Issue #595.
fn inject_subject_into_wire_node(wire_node: &mut WireNode, subject: &str) {
    match wire_node {
        WireNode::Table { caption, .. } if caption.is_empty() => {
            *caption = subject.to_string();
        }
        WireNode::Image { title, alt, .. } => {
            if title.is_none() {
                *title = Some(subject.to_string());
            }
            if alt.is_empty() {
                *alt = subject.to_string();
            }
        }
        WireNode::Video { title, .. } if title.is_none() => {
            *title = Some(subject.to_string());
        }
        WireNode::Audio { title, .. } if title.is_none() => {
            *title = Some(subject.to_string());
        }
        _ => {}
    }
}

/// Convert a typed [`WireNode`] returned from `dispatch_ir_build`
/// directly into a typed IR `DocNode`, switching on the wire variant
/// (not on the label string).
///
/// Pre-#615 this path went wire → lex-core AST (via `from_wire_node`)
/// → IR (via `from_lex_table` / `from_lex_media_verbatim`), and the
/// media branch re-dispatched on the label string to pick the right
/// hydration helper. The unified registry surface (#615) eliminates
/// both detours: a `WireNode::Image` becomes a `DocNode::Image`
/// directly, no label-string switch needed.
///
/// Returns `None` when the wire variant isn't one this builder knows
/// how to type — the caller falls back to a generic `DocNode::Verbatim`.
fn from_wire_typed(
    wire_node: &WireNode,
    registry: &Registry,
    fallback_content: &str,
) -> Option<DocNode> {
    match wire_node {
        WireNode::Table { .. } => {
            // Tables hydrate via the lex-core AST round-trip — IR's
            // Table mirrors the AST shape exactly (cells, alignment,
            // headers), and the existing `from_lex_table` converter
            // is the canonical place that knows how to consume it.
            // Switching on `WireNode::Table` (not on the label string)
            // keeps third-party `Table`-shaped wire kinds aligned with
            // the built-in path automatically.
            let items = lex_core::lex::wire::from_wire_node(wire_node).ok()?;
            let LexContentItem::Table(table) = items.into_iter().next()? else {
                return None;
            };
            Some(from_lex_table(&table, registry))
        }
        WireNode::Image {
            src, alt, title, ..
        } => Some(DocNode::Image(Image {
            src: src.clone(),
            alt: if alt.is_empty() {
                fallback_content.trim().to_string()
            } else {
                alt.clone()
            },
            title: title.clone(),
        })),
        WireNode::Video {
            src, title, poster, ..
        } => Some(DocNode::Video(Video {
            src: src.clone(),
            title: title.clone(),
            poster: poster.clone(),
        })),
        WireNode::Audio { src, title, .. } => Some(DocNode::Audio(Audio {
            src: src.clone(),
            title: title.clone(),
        })),
        _ => None,
    }
}

/// Converts a lex annotation to an IR annotation.
fn from_lex_annotation(annotation: &LexAnnotation, level: usize, registry: &Registry) -> DocNode {
    let label = annotation.data.label.value.clone();
    let form = annotation.data.label.form;
    let parameters = annotation
        .data
        .parameters
        .iter()
        .map(|p| (p.key.clone(), p.value.clone()))
        .collect();
    let content = convert_children(&annotation.children, level, registry);
    DocNode::Annotation(Annotation {
        label,
        parameters,
        content,
        form,
    })
}

/// Converts a standalone TextLine to an IR paragraph.
/// TextLines are typically parts of paragraphs, but can appear standalone.
fn from_lex_text_line(text_line: &LexTextLine) -> DocNode {
    let content = convert_inline_content(&text_line.content);
    DocNode::Paragraph(Paragraph { content })
}

/// Converts a VerbatimLine to an IR verbatim block.
/// VerbatimLines are typically parts of VerbatimBlocks, but can appear standalone.
/// Converts a native lex Table AST node to an IR Table node.
fn from_lex_table(table: &lex_core::lex::ast::Table, registry: &Registry) -> DocNode {
    use crate::ir::nodes::{
        Table as IrTable, TableCell as IrTableCell, TableCellAlignment as IrAlign,
        TableRow as IrTableRow,
    };

    let convert_align = |a: lex_core::lex::ast::TableCellAlignment| -> IrAlign {
        match a {
            lex_core::lex::ast::TableCellAlignment::Left => IrAlign::Left,
            lex_core::lex::ast::TableCellAlignment::Center => IrAlign::Center,
            lex_core::lex::ast::TableCellAlignment::Right => IrAlign::Right,
            lex_core::lex::ast::TableCellAlignment::None => IrAlign::None,
        }
    };

    let convert_row = |row: &lex_core::lex::ast::TableRow| -> IrTableRow {
        IrTableRow {
            cells: row
                .cells
                .iter()
                .map(|cell| {
                    let content = if cell.has_block_content() {
                        convert_children(&cell.children, 2, registry)
                    } else {
                        vec![DocNode::Paragraph(Paragraph {
                            content: convert_inline_content(&cell.content),
                        })]
                    };
                    IrTableCell {
                        content,
                        header: cell.header,
                        align: convert_align(cell.align),
                        colspan: cell.colspan,
                        rowspan: cell.rowspan,
                    }
                })
                .collect(),
        }
    };

    let header: Vec<IrTableRow> = table.header_rows.iter().map(convert_row).collect();
    let rows: Vec<IrTableRow> = table.body_rows.iter().map(convert_row).collect();
    let caption = if table.subject.as_string().is_empty() {
        None
    } else {
        Some(convert_inline_content(&table.subject))
    };

    let footnotes = table
        .footnotes
        .as_ref()
        .map(|list| vec![from_lex_list(list, 2, registry)])
        .unwrap_or_default();

    let fullwidth = matches!(
        table.mode,
        lex_core::lex::ast::elements::verbatim::VerbatimBlockMode::Fullwidth
    );

    DocNode::Table(IrTable {
        rows,
        header,
        caption,
        footnotes,
        fullwidth,
    })
}

fn from_lex_verbatim_line(verbatim_line: &LexVerbatimLine) -> DocNode {
    let content = verbatim_line.content.as_string().to_string();
    DocNode::Verbatim(Verbatim {
        subject: None,
        language: None,
        content,
    })
}

/// Detects the list decoration style from a marker.
fn detect_list_style(marker: &TextContent) -> ListStyle {
    let marker_text = marker.as_string().trim();
    if marker_text.is_empty() {
        return ListStyle::Bullet;
    }

    // Strip trailing `.` or `)` to get the label part
    let label = marker_text.trim_end_matches(['.', ')']);

    if label.is_empty() {
        return ListStyle::Bullet;
    }

    // Check for bullet markers
    if matches!(label, "-" | "*" | "+" | "–" | "—") {
        return ListStyle::Bullet;
    }

    // Check for numeric: all digits
    if label.chars().all(|c| c.is_ascii_digit()) {
        return ListStyle::Numeric;
    }

    // Check for roman numerals (uppercase)
    if label
        .chars()
        .all(|c| matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'))
    {
        return ListStyle::RomanUpper;
    }

    // Check for roman numerals (lowercase)
    if label
        .chars()
        .all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'))
    {
        return ListStyle::RomanLower;
    }

    // Check for alpha (single or multi char)
    if label.chars().all(|c| c.is_ascii_uppercase()) {
        return ListStyle::AlphaUpper;
    }

    if label.chars().all(|c| c.is_ascii_lowercase()) {
        return ListStyle::AlphaLower;
    }

    // Fallback: if it has a period/paren, treat as numeric ordered
    if marker_text.contains('.') || marker_text.contains(')') {
        ListStyle::Numeric
    } else {
        ListStyle::Bullet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::ast::elements::{
        List as LexList, ListItem as LexListItem, Paragraph as LexParagraph, Session as LexSession,
        VerbatimContent,
    };
    use lex_core::lex::ast::{ContentItem, Document as LexDocument, TextContent};

    /// Test-scope shorthand for the lex-babel default registry —
    /// every test that calls `from_lex_document` directly needs one.
    fn test_registry() -> &'static Registry {
        crate::default_registry()
    }

    #[test]
    fn test_simple_paragraph_conversion() {
        let lex_para = LexParagraph::from_line("Hello world".to_string());
        let ir_node = from_lex_paragraph(&lex_para);

        match ir_node {
            DocNode::Paragraph(para) => {
                assert_eq!(para.content.len(), 1);
                assert!(
                    matches!(&para.content[0], InlineContent::Text(text) if text == "Hello world")
                );
            }
            _ => panic!("Expected Paragraph node"),
        }
    }

    #[test]
    fn test_session_to_heading() {
        let session = LexSession::with_title("Test Section".to_string());
        let ir_node = from_lex_session(&session, 1, test_registry());

        match ir_node {
            DocNode::Heading(heading) => {
                assert_eq!(heading.level, 1);
                assert_eq!(heading.content.len(), 1);
                assert!(heading.children.is_empty());
            }
            _ => panic!("Expected Heading node"),
        }
    }

    #[test]
    fn test_list_conversion() {
        let item1 = LexListItem::new("-".to_string(), "Item 1".to_string());
        let item2 = LexListItem::new("-".to_string(), "Item 2".to_string());
        let list = LexList::new(vec![item1, item2]);

        let ir_node = from_lex_list(&list, 1, test_registry());

        match ir_node {
            DocNode::List(list) => {
                assert_eq!(list.items.len(), 2);
            }
            _ => panic!("Expected List node"),
        }
    }

    #[test]
    fn test_verbatim_language_extraction() {
        let subject = TextContent::from_string("".to_string(), None);
        let content = vec![VerbatimContent::VerbatimLine(LexVerbatimLine::new(
            "code here".to_string(),
        ))];
        let closing_data = lex_core::lex::ast::Data::new(
            lex_core::lex::ast::elements::Label::new("rust".to_string()),
            Vec::new(),
        );
        let verb = LexVerbatim::new(
            subject,
            content,
            closing_data,
            lex_core::lex::ast::elements::verbatim::VerbatimBlockMode::Inflow,
        );

        let ir_node = from_lex_verbatim(&verb, test_registry());

        match ir_node {
            DocNode::Verbatim(verb) => {
                assert_eq!(verb.language, Some("rust".to_string()));
                assert_eq!(verb.content, "code here");
            }
            _ => panic!("Expected Verbatim node"),
        }
    }

    #[test]
    fn test_blank_lines_filtered() {
        let para = ContentItem::Paragraph(LexParagraph::from_line("Test".to_string()));
        let blank = ContentItem::BlankLineGroup(lex_core::lex::ast::elements::BlankLineGroup::new(
            1,
            Vec::new(),
        ));

        let children = convert_children(&[para, blank], 1, test_registry());

        assert_eq!(children.len(), 1);
    }

    #[test]
    fn test_full_document_conversion() {
        let doc = LexDocument::with_content(vec![ContentItem::Paragraph(LexParagraph::from_line(
            "Test paragraph".to_string(),
        ))]);

        let ir_doc = from_lex_document(&doc, test_registry());

        assert_eq!(ir_doc.children.len(), 1);
        assert!(matches!(ir_doc.children[0], DocNode::Paragraph(_)));
    }

    /// Build a lex-core document with one document-scope annotation
    /// attached. Used by Phase 3a tests for the new
    /// `Document::document_annotations` slot.
    fn doc_with_one_annotation(label: &str, value: &str) -> LexDocument {
        use lex_core::lex::ast::elements::annotation::Annotation as LexAnnotation;
        use lex_core::lex::ast::elements::label::Label;
        use lex_core::lex::ast::elements::paragraph::Paragraph;
        use lex_core::lex::ast::elements::typed_content::ContentElement;

        let body = ContentElement::Paragraph(Paragraph::from_line(value.to_string()));
        let ann = LexAnnotation::new(Label::from_string(label), Vec::new(), vec![body]);
        let mut doc = LexDocument::new();
        doc.annotations.push(ann);
        doc
    }

    #[test]
    fn document_annotations_field_is_populated_from_lex_document() {
        // Phase 3a contract: `from_lex_document` must populate the new
        // `document_annotations: Vec<Annotation>` slot from every
        // annotation in `doc.annotations` (the lex-core
        // document-scope slot). The synthetic `frontmatter`
        // annotation that the legacy promotion inserts into
        // `children` is *additive* in Phase 3a — Phase 3b removes it.
        let doc = doc_with_one_annotation("acme.custom", "Body text.");
        let ir_doc = from_lex_document(&doc, test_registry());

        assert_eq!(
            ir_doc.document_annotations.len(),
            1,
            "one document-scope annotation must populate document_annotations"
        );
        let ann = &ir_doc.document_annotations[0];
        assert_eq!(ann.label, "acme.custom");
        assert_eq!(ann.content.len(), 1);
    }

    #[test]
    fn document_annotations_empty_when_no_document_scope_annotations() {
        // No document-scope annotations → empty slot. Phase 3a must
        // not synthesize anything here on its own.
        let doc = LexDocument::with_content(vec![ContentItem::Paragraph(LexParagraph::from_line(
            "Body only.".to_string(),
        ))]);
        let ir_doc = from_lex_document(&doc, test_registry());
        assert!(
            ir_doc.document_annotations.is_empty(),
            "empty input must produce empty document_annotations"
        );
    }

    #[test]
    fn normalize_labels_pipeline_writes_canonical_labels_into_ast() {
        // Confirm the activated `NormalizeLabels` pass produces a
        // Document whose annotations carry canonical `lex.metadata.*`
        // labels by the time `from_lex_document` sees them.
        use lex_core::lex::transforms::standard::STRING_TO_AST;

        let lex_doc = STRING_TO_AST
            .run(":: title :: My Doc\n\nBody.\n".to_string())
            .expect("parse ok");
        let title_in_ast = lex_doc
            .annotations
            .first()
            .or_else(|| {
                lex_doc.root.children.iter().find_map(|item| match item {
                    lex_core::lex::ast::ContentItem::Annotation(a) => Some(a),
                    _ => None,
                })
            })
            .expect("title annotation parsed");
        assert_eq!(title_in_ast.data.label.value, "lex.metadata.title");

        // Post-refac/label cleanup: `from_lex_document` no longer
        // synthesizes a `frontmatter` annotation in children. The
        // canonical-labelled annotation lands in
        // `document_annotations` instead; the `frontmatter` event is
        // synthesized at the events-emission layer.
        let ir = from_lex_document(&lex_doc, test_registry());
        let frontmatter_in_children = ir
            .children
            .iter()
            .any(|c| matches!(c, DocNode::Annotation(a) if a.label == "frontmatter"));
        assert!(
            !frontmatter_in_children,
            "frontmatter must no longer be synthesized in children"
        );
    }

    /// Issue #595 regression: the subject line on a media / tabular
    /// verbatim must survive the `on_resolve` dispatch as a default
    /// caption / title / alt. Built-in resolve handlers don't read
    /// `ctx.subject` (it isn't on the ctx), so the host has to inject
    /// it after the wire roundtrip.
    #[test]
    fn verbatim_subject_becomes_image_title_when_handler_left_it_empty() {
        let subject = TextContent::from_string("Sunset Photo".to_string(), None);
        let label = lex_core::lex::ast::elements::Label::new("lex.media.image".to_string());
        let parameters = vec![lex_core::lex::ast::Parameter {
            key: "src".to_string(),
            value: "sunset.jpg".to_string(),
            location: lex_core::lex::ast::Range::default(),
        }];
        let closing_data = lex_core::lex::ast::Data::new(label, parameters);
        let verb = LexVerbatim::new(
            subject,
            Vec::new(),
            closing_data,
            lex_core::lex::ast::elements::verbatim::VerbatimBlockMode::Inflow,
        );
        let ir_node = from_lex_verbatim(&verb, test_registry());
        match ir_node {
            DocNode::Image(image) => {
                assert_eq!(image.src, "sunset.jpg");
                assert_eq!(
                    image.title.as_deref(),
                    Some("Sunset Photo"),
                    "subject must be restored as the image title"
                );
            }
            other => panic!("expected DocNode::Image, got {other:?}"),
        }
    }

    #[test]
    fn verbatim_subject_becomes_table_caption_when_handler_left_it_empty() {
        let subject = TextContent::from_string("Quarterly results".to_string(), None);
        let content = vec![
            VerbatimContent::VerbatimLine(LexVerbatimLine::new("| Q1 | Q2 |".to_string())),
            VerbatimContent::VerbatimLine(LexVerbatimLine::new("| -- | -- |".to_string())),
            VerbatimContent::VerbatimLine(LexVerbatimLine::new("| 10 | 20 |".to_string())),
        ];
        let closing_data = lex_core::lex::ast::Data::new(
            lex_core::lex::ast::elements::Label::new("lex.tabular.table".to_string()),
            Vec::new(),
        );
        let verb = LexVerbatim::new(
            subject,
            content,
            closing_data,
            lex_core::lex::ast::elements::verbatim::VerbatimBlockMode::Inflow,
        );
        let ir_node = from_lex_verbatim(&verb, test_registry());
        match ir_node {
            DocNode::Table(table) => {
                let caption = table
                    .caption
                    .as_ref()
                    .map(|inlines| {
                        inlines
                            .iter()
                            .filter_map(|inline| match inline {
                                crate::ir::nodes::InlineContent::Text(t) => Some(t.as_str()),
                                _ => None,
                            })
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                assert_eq!(
                    caption, "Quarterly results",
                    "subject must be restored as the table caption"
                );
            }
            other => panic!("expected DocNode::Table, got {other:?}"),
        }
    }

    #[test]
    fn verbatim_subject_does_not_overwrite_handler_set_image_title() {
        // When the user wrote both a subject AND an explicit `title=`
        // parameter, the param wins — the subject is only a fallback
        // for empty fields.
        let subject = TextContent::from_string("Subject Wins?".to_string(), None);
        let label = lex_core::lex::ast::elements::Label::new("lex.media.image".to_string());
        let parameters = vec![
            lex_core::lex::ast::Parameter {
                key: "src".to_string(),
                value: "x.jpg".to_string(),
                location: lex_core::lex::ast::Range::default(),
            },
            lex_core::lex::ast::Parameter {
                key: "title".to_string(),
                value: "Param Wins".to_string(),
                location: lex_core::lex::ast::Range::default(),
            },
        ];
        let closing_data = lex_core::lex::ast::Data::new(label, parameters);
        let verb = LexVerbatim::new(
            subject,
            Vec::new(),
            closing_data,
            lex_core::lex::ast::elements::verbatim::VerbatimBlockMode::Inflow,
        );
        let ir_node = from_lex_verbatim(&verb, test_registry());
        match ir_node {
            DocNode::Image(image) => {
                assert_eq!(image.title.as_deref(), Some("Param Wins"));
            }
            other => panic!("expected DocNode::Image, got {other:?}"),
        }
    }

    #[test]
    fn document_annotations_is_source_of_truth_for_doc_metadata() {
        // Post-refac/label cleanup: document-scope metadata flows
        // through `document_annotations` only — the legacy
        // `frontmatter` synthesis in children is gone.
        let doc = doc_with_one_annotation("author", "Alice");
        let ir_doc = from_lex_document(&doc, test_registry());

        // The new slot carries the annotation.
        assert_eq!(ir_doc.document_annotations.len(), 1);
        assert_eq!(ir_doc.document_annotations[0].label, "author");

        // Children no longer carry a synthetic frontmatter annotation.
        let frontmatter_in_children = ir_doc
            .children
            .iter()
            .any(|c| matches!(c, DocNode::Annotation(a) if a.label == "frontmatter"));
        assert!(
            !frontmatter_in_children,
            "synthetic frontmatter must not appear in children after cleanup"
        );
    }

    /// #615: `from_lex_verbatim` migrated from `dispatch_resolve` to
    /// `dispatch_ir_build`. The end-to-end behaviour for built-in
    /// verbatim labels (table, image, video, audio) must be unchanged
    /// — same typed IR nodes produced from the same Lex source.
    #[test]
    fn ir_build_dispatch_hydrates_table_verbatim_to_typed_table() {
        let subject = TextContent::from_string("Sales".to_string(), None);
        let body = vec![
            VerbatimContent::VerbatimLine(LexVerbatimLine::new("| q | r |".to_string())),
            VerbatimContent::VerbatimLine(LexVerbatimLine::new("|---|---|".to_string())),
            VerbatimContent::VerbatimLine(LexVerbatimLine::new("| 7 | 9 |".to_string())),
        ];
        let closing_data = lex_core::lex::ast::Data::new(
            lex_core::lex::ast::elements::Label::new("lex.tabular.table".to_string()),
            Vec::new(),
        );
        let verb = LexVerbatim::new(
            subject,
            body,
            closing_data,
            lex_core::lex::ast::elements::verbatim::VerbatimBlockMode::Inflow,
        );
        match from_lex_verbatim(&verb, test_registry()) {
            DocNode::Table(_) => {}
            other => panic!(
                "table verbatim must hydrate via dispatch_ir_build to DocNode::Table; got {other:?}"
            ),
        }
    }

    /// #615: `from_wire_typed` switches on the WireNode kind directly
    /// instead of re-dispatching on the label string. A `lex.media.video`
    /// hydration produces a typed `DocNode::Video`, populated from the
    /// wire node's typed fields (no params HashMap detour).
    #[test]
    fn ir_build_dispatch_hydrates_video_via_wire_kind_switch() {
        let subject = TextContent::from_string("".to_string(), None);
        let label = lex_core::lex::ast::elements::Label::new("lex.media.video".to_string());
        let parameters = vec![
            lex_core::lex::ast::Parameter {
                key: "src".to_string(),
                value: "intro.mp4".to_string(),
                location: lex_core::lex::ast::Range::default(),
            },
            lex_core::lex::ast::Parameter {
                key: "poster".to_string(),
                value: "intro.png".to_string(),
                location: lex_core::lex::ast::Range::default(),
            },
        ];
        let closing_data = lex_core::lex::ast::Data::new(label, parameters);
        let verb = LexVerbatim::new(
            subject,
            Vec::new(),
            closing_data,
            lex_core::lex::ast::elements::verbatim::VerbatimBlockMode::Inflow,
        );
        match from_lex_verbatim(&verb, test_registry()) {
            DocNode::Video(video) => {
                assert_eq!(video.src, "intro.mp4");
                assert_eq!(video.poster.as_deref(), Some("intro.png"));
            }
            other => panic!("expected DocNode::Video, got {other:?}"),
        }
    }

    /// #615: `from_wire_typed` returns `None` for wire variants it
    /// doesn't know how to type (third-party verbatim labels with no
    /// IR-build hook, or hooks returning non-media/non-table wire
    /// kinds), and the caller falls back to a generic `DocNode::Verbatim`.
    /// This pins the fallback path so a future wire-spec addition
    /// can't silently produce `Verbatim`-with-empty-content.
    #[test]
    fn unhandled_label_falls_back_to_generic_verbatim() {
        let subject = TextContent::from_string("".to_string(), None);
        let body = vec![VerbatimContent::VerbatimLine(LexVerbatimLine::new(
            "raw body".to_string(),
        ))];
        let closing_data = lex_core::lex::ast::Data::new(
            // Community-shape label — no built-in handler, no IR-build
            // dispatch. The from_lex_verbatim path falls through to
            // the generic Verbatim IR node.
            lex_core::lex::ast::elements::Label::new("acme.unknown".to_string()),
            Vec::new(),
        );
        let verb = LexVerbatim::new(
            subject,
            body,
            closing_data,
            lex_core::lex::ast::elements::verbatim::VerbatimBlockMode::Inflow,
        );
        match from_lex_verbatim(&verb, test_registry()) {
            DocNode::Verbatim(v) => {
                assert_eq!(v.content, "raw body");
                assert_eq!(v.language.as_deref(), Some("acme.unknown"));
            }
            other => {
                panic!("unrouted verbatim label must fall back to generic Verbatim; got {other:?}")
            }
        }
    }

    /// #615: `doc.*` schemas are registered in the default registry
    /// and accessible via `schema_for`. End-to-end check that the
    /// six built-in canonicals all surface via `Registry::schema_for`
    /// from the lex-babel default registry (the one most callers use).
    #[test]
    fn default_registry_carries_doc_metadata_schemas() {
        let r = test_registry();
        for label in [
            "doc.title",
            "doc.author",
            "doc.date",
            "doc.tags",
            "doc.category",
            "doc.template",
        ] {
            let schema = r
                .schema_for(label)
                .unwrap_or_else(|| panic!("default registry must carry {label}"));
            assert_eq!(schema.label, label);
            // Each doc.* declares both markdown + html render hooks
            // so the unified surface can fire them at serialisation
            // time (Sub D wires the consumer side).
            let formats: Vec<&str> = schema.hooks.render.iter().map(|h| h.0.as_str()).collect();
            assert!(
                formats.contains(&"markdown") && formats.contains(&"html"),
                "{label} must declare markdown + html render hooks; got {formats:?}"
            );
        }
    }

    /// #615: the `doc.*` namespace dispatches through the unified
    /// render surface and produces format-specific text.
    #[test]
    fn doc_render_dispatch_emits_markdown_yaml_line_via_unified_surface() {
        use lex_extension::wire::{AnnotationBody, Format, NodeRef, Position, Range, RenderOut};
        let r = test_registry();
        let ctx = lex_extension::wire::LabelCtx {
            label: "doc.author".into(),
            params: serde_json::Value::Null,
            body: AnnotationBody::Text("Alice".into()),
            node: NodeRef {
                kind: "document".into(),
                range: Range {
                    start: Position(0, 0),
                    end: Position(0, 0),
                },
                origin: None,
            },
        };
        let out = r
            .dispatch_render(&ctx, Format::Markdown)
            .expect("dispatch_render ok")
            .expect("doc.author must produce a rendered output");
        match out {
            RenderOut::String { string } => assert_eq!(string, "author: \"Alice\"\n"),
            other => panic!("expected String, got {other:?}"),
        }
    }
}
