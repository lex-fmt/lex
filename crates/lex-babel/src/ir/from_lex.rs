use lex_core::lex::ast::elements::{
    inlines::InlineNode, Annotation as LexAnnotation, ContentItem as LexContentItem,
    Definition as LexDefinition, Document as LexDocument, List as LexList, ListItem as LexListItem,
    Paragraph as LexParagraph, Session as LexSession, TextLine as LexTextLine,
    Verbatim as LexVerbatim, VerbatimLine as LexVerbatimLine,
};
use lex_core::lex::ast::TextContent;

use super::nodes::{
    Annotation, Definition, DocNode, Document, Heading, InlineContent, List, ListForm, ListItem,
    ListStyle, Paragraph, Verbatim,
};

/// Converts a lex document to the IR.
pub fn from_lex_document(doc: &LexDocument) -> Document {
    let mut children = convert_children(&doc.root.children, 2);

    let mut parameters = Vec::new();

    // 1. Process document-level annotations
    for ann in &doc.annotations {
        let key = ann.data.label.value.clone();
        let value = if !ann.children.is_empty() {
            let mut text = String::new();
            for child in &ann.children {
                if let LexContentItem::Paragraph(p) = child {
                    text.push_str(&p.text());
                }
            }
            text
        } else {
            String::new()
        };

        if !value.is_empty() {
            parameters.push((key, value));
        } else {
            for param in &ann.data.parameters {
                parameters.push((format!("{}.{}", key, param.key), param.value.clone()));
            }
        }
    }

    // 2. Scan children for metadata annotations (e.g. attached to first element)
    let mut indices_to_remove = Vec::new();

    // Whitelist of labels to treat as frontmatter
    let metadata_labels = [
        "author",
        "publishing-date",
        "title",
        "date",
        "tags",
        "category",
        "template",
        "front-matter",
    ];

    for (i, child) in children.iter().enumerate() {
        if let DocNode::Annotation(ann) = child {
            if metadata_labels.contains(&ann.label.as_str()) {
                // It's metadata!
                let key = ann.label.clone();
                // Extract value (content or params)
                let value = if !ann.content.is_empty() {
                    // Flatten content
                    let mut text = String::new();
                    for c in &ann.content {
                        if let DocNode::Paragraph(p) = c {
                            for ic in &p.content {
                                if let InlineContent::Text(t) = ic {
                                    text.push_str(t);
                                }
                            }
                        }
                    }
                    text
                } else {
                    String::new()
                };

                if !value.is_empty() {
                    parameters.push((key, value));
                } else {
                    for (k, v) in &ann.parameters {
                        parameters.push((format!("{key}.{k}"), v.clone()));
                    }
                }

                indices_to_remove.push(i);
            }
        }
    }

    // Remove promoted annotations (in reverse order to keep indices valid)
    for i in indices_to_remove.iter().rev() {
        children.remove(*i);
    }

    if !parameters.is_empty() {
        let frontmatter = DocNode::Annotation(Annotation {
            label: "frontmatter".to_string(),
            parameters,
            content: vec![],
        });
        children.insert(0, frontmatter);
    }

    Document { children }
}

/// Helper: Converts a list of content items, filtering out blank lines
/// Also extracts annotations attached to each element
fn convert_children(items: &[LexContentItem], level: usize) -> Vec<DocNode> {
    items
        .iter()
        .filter(|item| !matches!(item, LexContentItem::BlankLineGroup(_)))
        .flat_map(|item| {
            let mut nodes = extract_attached_annotations(item, level);
            nodes.push(from_lex_content_item_with_level(item, level));
            nodes
        })
        .collect()
}

/// Extracts annotations attached to a content item and converts them to IR nodes
fn extract_attached_annotations(item: &LexContentItem, level: usize) -> Vec<DocNode> {
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
        .map(|anno| from_lex_annotation(anno, level))
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
fn from_lex_content_item_with_level(item: &LexContentItem, level: usize) -> DocNode {
    match item {
        LexContentItem::Session(session) => from_lex_session(session, level),
        LexContentItem::Paragraph(paragraph) => from_lex_paragraph(paragraph),
        LexContentItem::List(list) => from_lex_list(list, level),
        LexContentItem::ListItem(list_item) => from_lex_list_item(list_item, level),
        LexContentItem::Definition(definition) => from_lex_definition(definition, level),
        LexContentItem::VerbatimBlock(verbatim) => from_lex_verbatim(verbatim),
        LexContentItem::Table(table) => from_lex_table(table),
        LexContentItem::Annotation(annotation) => from_lex_annotation(annotation, level),
        LexContentItem::TextLine(text_line) => from_lex_text_line(text_line),
        LexContentItem::VerbatimLine(verbatim_line) => from_lex_verbatim_line(verbatim_line),
        LexContentItem::BlankLineGroup(_) => {
            // Blank lines are filtered out by convert_children, but handle gracefully if encountered
            DocNode::Paragraph(Paragraph { content: vec![] })
        }
    }
}

/// Converts a lex session to an IR heading.
fn from_lex_session(session: &LexSession, level: usize) -> DocNode {
    // Preserve the original session title (including any sequence marker)
    let mut content = Vec::new();

    // If there is a marker, add it as a separate inline element
    if let Some(marker) = &session.marker {
        content.push(InlineContent::Marker(marker.as_str().to_string()));
        // Add a space after marker if title is not empty
        if !session.title.as_string().is_empty() {
            content.push(InlineContent::Text(" ".to_string()));
        }

        // Strip marker from title content to avoid duplication
        let mut title_content = convert_inline_content(&session.title);
        strip_marker_from_content(&mut title_content, marker.as_str());
        content.extend(title_content);
    } else {
        content.extend(convert_inline_content(&session.title));
    }

    let children = convert_children(&session.children, level + 1);
    DocNode::Heading(Heading {
        level,
        content,
        children,
    })
}

fn strip_marker_from_content(content: &mut [InlineContent], marker: &str) {
    if let Some(InlineContent::Text(text)) = content.first_mut() {
        if let Some(pos) = text.find(marker) {
            let after = &text[pos + marker.len()..];
            *text = after.trim_start().to_string();
        }
    }
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
fn from_lex_list(list: &LexList, level: usize) -> DocNode {
    let items: Vec<ListItem> = list
        .items
        .iter()
        .filter_map(|item| {
            if let LexContentItem::ListItem(li) = item {
                Some(convert_list_item(li, level))
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
fn from_lex_list_item(list_item: &LexListItem, level: usize) -> DocNode {
    DocNode::ListItem(convert_list_item(list_item, level))
}

/// Converts a lex list item to an IR list item struct.
fn convert_list_item(list_item: &LexListItem, level: usize) -> ListItem {
    // List item has text (Vec<TextContent>) and children
    let mut content = Vec::new();

    // Add marker
    content.push(InlineContent::Marker(
        list_item.marker.as_string().to_string(),
    ));
    // Add space after marker if there is text
    if !list_item.text.is_empty() {
        content.push(InlineContent::Text(" ".to_string()));
    }

    for text_content in &list_item.text {
        content.extend(convert_inline_content(text_content));
    }
    let children = convert_children(&list_item.children, level);
    ListItem { content, children }
}

/// Converts a lex definition to an IR definition.
fn from_lex_definition(definition: &LexDefinition, level: usize) -> DocNode {
    let term = convert_inline_content(&definition.subject);
    let description = convert_children(&definition.children, level);
    DocNode::Definition(Definition { term, description })
}

/// Converts a lex verbatim block to an IR verbatim block.
fn from_lex_verbatim(verbatim: &LexVerbatim) -> DocNode {
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

    let registry = crate::common::verbatim::VerbatimRegistry::default_with_standard();

    if let Some(handler) = registry.get(&verbatim.closing_data.label.value) {
        let params = verbatim
            .closing_data
            .parameters
            .iter()
            .map(|p| (p.key.clone(), p.value.clone()))
            .collect();
        if let Some(node) = handler.to_ir(&content, &params) {
            return node;
        }
    }

    DocNode::Verbatim(Verbatim {
        subject,
        language,
        content,
    })
}

/// Converts a lex annotation to an IR annotation.
fn from_lex_annotation(annotation: &LexAnnotation, level: usize) -> DocNode {
    let label = annotation.data.label.value.clone();
    let parameters = annotation
        .data
        .parameters
        .iter()
        .map(|p| (p.key.clone(), p.value.clone()))
        .collect();
    let content = convert_children(&annotation.children, level);
    DocNode::Annotation(Annotation {
        label,
        parameters,
        content,
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
fn from_lex_table(table: &lex_core::lex::ast::Table) -> DocNode {
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
                        convert_children(&cell.children, 2)
                    } else {
                        vec![DocNode::Paragraph(Paragraph {
                            content: convert_inline_content(&cell.content),
                        })]
                    };
                    IrTableCell {
                        content,
                        header: cell.header,
                        align: convert_align(cell.align),
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

    DocNode::Table(IrTable {
        rows,
        header,
        caption,
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
        let ir_node = from_lex_session(&session, 1);

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

        let ir_node = from_lex_list(&list, 1);

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

        let ir_node = from_lex_verbatim(&verb);

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

        let children = convert_children(&[para, blank], 1);

        assert_eq!(children.len(), 1);
    }

    #[test]
    fn test_full_document_conversion() {
        let doc = LexDocument::with_content(vec![ContentItem::Paragraph(LexParagraph::from_line(
            "Test paragraph".to_string(),
        ))]);

        let ir_doc = from_lex_document(&doc);

        assert_eq!(ir_doc.children.len(), 1);
        assert!(matches!(ir_doc.children[0], DocNode::Paragraph(_)));
    }
}
