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

use super::nodes::{
    Annotation, Definition, DocNode, Document, Heading, InlineContent, List, ListItem, Paragraph,
    Table, TableCell, TableRow, Verbatim,
};

/// Converts an IR document to a Lex document.
pub fn to_lex_document(doc: &Document) -> LexDocument {
    let mut children = Vec::new();

    for node in &doc.children {
        children.extend(to_lex_content_items(node, 1));
    }

    LexDocument::with_content(children)
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

/// Converts an IR Table to a Lex Annotation (nested).
fn to_lex_table(table: &Table, level: usize) -> LexContentItem {
    let registry = crate::common::verbatim::VerbatimRegistry::default_with_standard();
    let node = DocNode::Table(table.clone());

    if let Some(handler) = registry.get("doc.table") {
        if let Some((content, params)) = handler.convert_from_ir(&node) {
            let label = Label::new("doc.table".to_string());
            let parameters = params
                .into_iter()
                .map(|(k, v)| Parameter {
                    key: k,
                    value: v,
                    location: default_range(),
                })
                .collect();

            let subject = TextContent::from_string("".to_string(), None);
            let lines = content
                .lines()
                .map(|l| VerbatimContent::VerbatimLine(LexVerbatimLine::new(l.to_string())))
                .collect();

            let closing_data = Data::new(label, parameters);

            return LexContentItem::VerbatimBlock(Box::new(LexVerbatim::new(
                subject,
                lines,
                closing_data,
                VerbatimBlockMode::Inflow,
            )));
        }
    }

    // Fallback to annotation if registry fails (though TableHandler should handle it)
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
fn to_lex_list(list: &List) -> LexContentItem {
    let items = list.items.iter().map(to_lex_list_item_struct).collect();
    LexContentItem::List(LexList::new(items))
}

/// Converts an IR ListItem to a ContentItem::ListItem.
fn to_lex_list_item(item: &ListItem) -> LexContentItem {
    LexContentItem::ListItem(to_lex_list_item_struct(item))
}

/// Converts an IR ListItem to a Lex ListItem struct.
fn to_lex_list_item_struct(item: &ListItem) -> LexListItem {
    // Extract marker from inline content if present
    let marker = item
        .content
        .iter()
        .find_map(|ic| {
            if let InlineContent::Marker(m) = ic {
                Some(m.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "-".to_string());

    // Build text from non-marker inline content
    let text = inline_content_to_text_skip_marker(&item.content);

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

/// Converts IR inline content to plain text, skipping Marker elements and
/// the whitespace immediately following them.
fn inline_content_to_text_skip_marker(content: &[InlineContent]) -> String {
    let mut skip_next_space = false;
    let mut parts = Vec::new();
    for inline in content {
        if matches!(inline, InlineContent::Marker(_)) {
            skip_next_space = true;
            continue;
        }
        if skip_next_space {
            skip_next_space = false;
            if let InlineContent::Text(t) = inline {
                if t == " " {
                    continue;
                }
            }
        }
        parts.push(inline_to_text(inline));
    }
    parts.join("")
}

fn inline_to_text(inline: &InlineContent) -> String {
    match inline {
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
        InlineContent::Marker(marker) => marker.clone(),
        InlineContent::Image(image) => {
            let mut text = format!("![{}]({})", image.alt, image.src);
            if let Some(title) = &image.title {
                text.push_str(&format!(" \"{title}\""));
            }
            text
        }
    }
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
            InlineContent::Marker(marker) => marker.clone(),
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
    let registry = crate::common::verbatim::VerbatimRegistry::default_with_standard();

    let label = match node {
        DocNode::Image(_) => "doc.image",
        DocNode::Video(_) => "doc.video",
        DocNode::Audio(_) => "doc.audio",
        _ => return LexContentItem::Paragraph(LexParagraph::new(vec![])),
    };

    if let Some(handler) = registry.get(label) {
        if let Some((content, params)) = handler.convert_from_ir(node) {
            let label = Label::new(label.to_string());
            let parameters = params
                .into_iter()
                .map(|(k, v)| Parameter {
                    key: k,
                    value: v,
                    location: default_range(),
                })
                .collect();

            let subject = TextContent::from_string("".to_string(), None);
            let lines = content
                .lines()
                .map(|l| VerbatimContent::VerbatimLine(LexVerbatimLine::new(l.to_string())))
                .collect();

            let closing_data = Data::new(label, parameters);

            return LexContentItem::VerbatimBlock(Box::new(LexVerbatim::new(
                subject,
                lines,
                closing_data,
                VerbatimBlockMode::Inflow,
            )));
        }
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
            children: vec![
                DocNode::Paragraph(Paragraph {
                    content: vec![InlineContent::Text("First paragraph".to_string())],
                }),
                DocNode::Paragraph(Paragraph {
                    content: vec![InlineContent::Text("Second paragraph".to_string())],
                }),
            ],
        };

        let lex_doc = to_lex_document(&ir_doc);

        // Document should have root session with our content
        assert!(!lex_doc.root.children.is_empty());
    }
}
