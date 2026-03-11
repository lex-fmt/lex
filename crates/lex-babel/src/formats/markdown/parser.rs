//! Markdown parsing (Markdown → Lex import)
//!
//! Converts CommonMark Markdown to Lex documents.
//! Pipeline: Markdown string → Comrak AST → Events → IR → Lex AST

use crate::common::flat_to_nested::events_to_tree;
use crate::error::FormatError;
use crate::ir::events::Event;
use crate::ir::nodes::{InlineContent, TableCellAlignment};
use comrak::nodes::{AstNode, NodeValue, TableAlignment};
use comrak::{parse_document, Arena, ComrakOptions};
use lex_core::lex::ast::Document;

/// Parse Markdown string to Lex document
pub fn parse_from_markdown(source: &str) -> Result<Document, FormatError> {
    // Step 1: Parse Markdown string to Comrak AST
    let arena = Arena::new();
    let options = default_comrak_options();
    let root = parse_document(&arena, source, &options);

    // Step 2: Convert Comrak AST to IR events
    let events = comrak_ast_to_events(root)?;

    // Step 3: Convert events to IR tree
    let ir_doc = events_to_tree(&events).map_err(|e| {
        FormatError::ParseError(format!("Failed to build IR tree from events: {e}"))
    })?;

    // Step 4: Convert IR to Lex AST
    let lex_doc = crate::from_ir(&ir_doc);
    Ok(lex_doc)
}

fn default_comrak_options() -> ComrakOptions<'static> {
    let mut options = ComrakOptions::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.superscript = true;
    options.extension.front_matter_delimiter = Some("---".to_string());
    options
}

type DefinitionPieces = Option<(Vec<InlineContent>, Vec<InlineContent>)>;

/// Convert Comrak AST to IR events
fn comrak_ast_to_events<'a>(root: &'a AstNode<'a>) -> Result<Vec<Event>, FormatError> {
    let mut events = vec![Event::StartDocument];

    // Check if first child is an H1 heading - if so, treat it as document title
    let mut children_iter = root.children().peekable();
    let mut document_title: Option<String> = None;

    if let Some(first_child) = children_iter.peek() {
        if let NodeValue::Heading(heading) = &first_child.data.borrow().value {
            if heading.level == 1 {
                // Extract H1 text as document title
                let first_child = children_iter.next().unwrap();
                let mut title_text = String::new();
                for child in first_child.children() {
                    collect_text_content(child, &mut title_text);
                }
                document_title = Some(title_text.trim().to_string());
            }
        }
    }

    // If we found a document title, emit it as a special event
    // For now, we'll emit it as a paragraph that becomes the document title
    // when converted back to Lex AST
    if let Some(title) = document_title {
        // Emit document title as a paragraph followed by an implicit blank
        // The from_ir conversion will recognize this as the document title
        events.push(Event::StartParagraph);
        events.push(Event::Inline(InlineContent::Text(title)));
        events.push(Event::EndParagraph);
    }

    collect_children_with_definitions(children_iter, &mut events)?;

    events.push(Event::EndDocument);
    Ok(events)
}

/// Collect text content from a node (for extracting document title)
fn collect_text_content<'a>(node: &'a AstNode<'a>, output: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Text(text) => output.push_str(text),
        NodeValue::SoftBreak | NodeValue::LineBreak => output.push(' '),
        _ => {
            for child in node.children() {
                collect_text_content(child, output);
            }
        }
    }
}

/// Recursively collect events from a Comrak AST node
fn collect_events_from_node<'a>(
    node: &'a AstNode<'a>,
    events: &mut Vec<Event>,
) -> Result<(), FormatError> {
    let node_data = node.data.borrow();

    match &node_data.value {
        NodeValue::Document => {
            // Skip document wrapper, process children
            collect_children_with_definitions(node.children(), events)?;
        }

        NodeValue::Heading(heading) => {
            let level = heading.level as usize;

            // Just emit StartHeading - flat_to_nested will auto-close headings
            events.push(Event::StartHeading(level));

            // Process heading text (inline content)
            for child in node.children() {
                collect_inline_events(child, events)?;
            }

            // No EndHeading needed - the generic flat_to_nested converter
            // automatically closes headings when it sees a new heading at same/higher level
        }

        NodeValue::Paragraph => {
            events.push(Event::StartParagraph);

            // Process inline content
            for child in node.children() {
                collect_inline_events(child, events)?;
            }

            events.push(Event::EndParagraph);
        }

        NodeValue::List(list) => {
            let ordered = matches!(list.list_type, comrak::nodes::ListType::Ordered);
            let style = if ordered {
                crate::ir::nodes::ListStyle::Numeric
            } else {
                crate::ir::nodes::ListStyle::Bullet
            };
            events.push(Event::StartList {
                ordered,
                style,
                form: crate::ir::nodes::ListForm::Short,
            });

            // Process list items
            for child in node.children() {
                collect_events_from_node(child, events)?;
            }

            events.push(Event::EndList);
        }

        NodeValue::Item(_) => {
            events.push(Event::StartListItem);

            // Process list item content
            collect_children_with_definitions(node.children(), events)?;

            events.push(Event::EndListItem);
        }

        NodeValue::CodeBlock(code_block) => {
            let language = if code_block.info.is_empty() {
                None
            } else {
                Some(code_block.info.clone())
            };

            events.push(Event::StartVerbatim {
                language,
                subject: None,
            });
            events.push(Event::Inline(InlineContent::Text(
                code_block.literal.clone(),
            )));
            events.push(Event::EndVerbatim);
        }

        NodeValue::HtmlBlock(html) => {
            // Try to parse as Lex annotation
            if let Some((label, parameters, content)) = parse_lex_annotation(&html.literal) {
                events.push(Event::StartAnnotation {
                    label: label.clone(),
                    parameters,
                });

                if let Some(text) = content {
                    events.push(Event::StartParagraph);
                    events.push(Event::Inline(InlineContent::Text(text)));
                    events.push(Event::EndParagraph);
                    // If it had content, it's a self-contained annotation block, so we close it immediately
                    events.push(Event::EndAnnotation { label });
                }
                // If no content, it's a start tag, closing tag will be found later
            } else if let Some(label) = parse_lex_annotation_close(&html.literal) {
                events.push(Event::EndAnnotation { label });
            }
            // Otherwise skip HTML blocks
        }

        NodeValue::FrontMatter(content) => {
            // Parse YAML frontmatter
            // We'll treat it as a special annotation with label "frontmatter"
            // and parameters derived from the YAML content (flattened)

            let yaml_str = content.trim();
            // Remove delimiters if present (Comrak might include them or not depending on version/options)
            let yaml_str = yaml_str
                .trim_start_matches("---")
                .trim_end_matches("---")
                .trim();

            let mut parameters = vec![];

            // Simple YAML parsing: key: value
            // For a real implementation, we should use serde_yaml, but for now we'll do basic parsing
            // to avoid adding a dependency if possible, or we can just store the raw content
            // as a special parameter if we want to be safe.
            // Let's try to parse simple key-value pairs.

            for line in yaml_str.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    let key = key.trim().to_string();
                    let value = value.trim().to_string();
                    // Remove quotes if present
                    let value = value.trim_matches('"').trim_matches('\'').to_string();

                    // Handle arrays like [a, b] -> just store as string for now
                    parameters.push((key, value));
                }
            }

            events.push(Event::StartAnnotation {
                label: "frontmatter".to_string(),
                parameters,
            });
            events.push(Event::EndAnnotation {
                label: "frontmatter".to_string(),
            });
        }

        NodeValue::ThematicBreak => {
            // Thematic breaks (---) don't have direct Lex equivalent, skip
        }

        NodeValue::BlockQuote => {
            // Block quotes don't have direct Lex equivalent
            // Process children as regular content
            for child in node.children() {
                collect_events_from_node(child, events)?;
            }
        }

        NodeValue::Table(_) => {
            events.push(Event::StartTable);
            for child in node.children() {
                collect_events_from_node(child, events)?;
            }
            events.push(Event::EndTable);
        }

        NodeValue::TableRow(header) => {
            events.push(Event::StartTableRow { header: *header });
            for child in node.children() {
                collect_events_from_node(child, events)?;
            }
            events.push(Event::EndTableRow);
        }

        NodeValue::TableCell => {
            let (header, align) = get_table_cell_info(node);
            events.push(Event::StartTableCell { header, align });

            events.push(Event::StartParagraph);
            for child in node.children() {
                collect_inline_events(child, events)?;
            }
            events.push(Event::EndParagraph);

            events.push(Event::EndTableCell);
        }

        _ => {
            // Unknown block type, skip
        }
    }

    Ok(())
}

fn get_table_cell_info<'a>(node: &'a AstNode<'a>) -> (bool, TableCellAlignment) {
    let parent = match node.parent() {
        Some(p) => p,
        None => return (false, TableCellAlignment::None),
    };

    let is_header = if let NodeValue::TableRow(header) = &parent.data.borrow().value {
        *header
    } else {
        false
    };

    let mut col_index = 0;
    let mut curr = node.previous_sibling();
    while let Some(sibling) = curr {
        col_index += 1;
        curr = sibling.previous_sibling();
    }

    let grandparent = match parent.parent() {
        Some(p) => p,
        None => return (is_header, TableCellAlignment::None),
    };

    let align = if let NodeValue::Table(table) = &grandparent.data.borrow().value {
        if col_index < table.alignments.len() {
            match table.alignments[col_index] {
                TableAlignment::Left => TableCellAlignment::Left,
                TableAlignment::Right => TableCellAlignment::Right,
                TableAlignment::Center => TableCellAlignment::Center,
                TableAlignment::None => TableCellAlignment::None,
            }
        } else {
            TableCellAlignment::None
        }
    } else {
        TableCellAlignment::None
    };

    (is_header, align)
}

/// Collect inline events from a Comrak node
fn collect_inline_events<'a>(
    node: &'a AstNode<'a>,
    events: &mut Vec<Event>,
) -> Result<(), FormatError> {
    let node_data = node.data.borrow();

    match &node_data.value {
        NodeValue::Text(text) => {
            events.push(Event::Inline(InlineContent::Text(text.clone())));
        }

        NodeValue::Strong => {
            // Collect children as bold content
            let mut children = vec![];
            for child in node.children() {
                collect_inline_content(child, &mut children)?;
            }
            events.push(Event::Inline(InlineContent::Bold(children)));
        }

        NodeValue::Emph => {
            // Collect children as italic content
            let mut children = vec![];
            for child in node.children() {
                collect_inline_content(child, &mut children)?;
            }
            events.push(Event::Inline(InlineContent::Italic(children)));
        }

        NodeValue::Code(code) => {
            events.push(Event::Inline(InlineContent::Code(code.literal.clone())));
        }

        NodeValue::Link(link) => {
            let text = collect_text_from_children(node);
            events.push(Event::Inline(InlineContent::Link {
                text,
                href: link.url.clone(),
            }));
        }

        NodeValue::SoftBreak | NodeValue::LineBreak => {
            events.push(Event::Inline(InlineContent::Text(" ".to_string())));
        }

        NodeValue::Image(link) => {
            let alt = collect_text_from_children(node);
            events.push(Event::Inline(InlineContent::Image(
                crate::ir::nodes::Image {
                    src: link.url.clone(),
                    alt,
                    title: if link.title.is_empty() {
                        None
                    } else {
                        Some(link.title.clone())
                    },
                },
            )));
        }

        _ => {
            // Skip unknown inline types
        }
    }

    Ok(())
}

fn collect_text_from_children<'a>(node: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    for child in node.children() {
        collect_text_content(child, &mut text);
    }
    text
}

/// Recursively collect inline content (for nested inlines like bold/italic)
fn collect_inline_content<'a>(
    node: &'a AstNode<'a>,
    content: &mut Vec<InlineContent>,
) -> Result<(), FormatError> {
    let node_data = node.data.borrow();

    match &node_data.value {
        NodeValue::Text(text) => {
            content.push(InlineContent::Text(text.clone()));
        }

        NodeValue::Strong => {
            let mut children = vec![];
            for child in node.children() {
                collect_inline_content(child, &mut children)?;
            }
            content.push(InlineContent::Bold(children));
        }

        NodeValue::Emph => {
            let mut children = vec![];
            for child in node.children() {
                collect_inline_content(child, &mut children)?;
            }
            content.push(InlineContent::Italic(children));
        }

        NodeValue::Code(code) => {
            content.push(InlineContent::Code(code.literal.clone()));
        }

        NodeValue::Link(link) => {
            let text = collect_text_from_children(node);
            content.push(InlineContent::Link {
                text,
                href: link.url.clone(),
            });
        }

        NodeValue::SoftBreak | NodeValue::LineBreak => {
            content.push(InlineContent::Text(" ".to_string()));
        }

        _ => {}
    }

    Ok(())
}

/// Determine if a node is a heading (used to know when to stop collecting
/// definition description siblings).
fn is_heading_node(node: &AstNode<'_>) -> bool {
    matches!(node.data.borrow().value, NodeValue::Heading(_))
}

/// Attempt to parse a paragraph as a definition term of the form
/// `**Term**: Description...` returning the term inline content and any
/// inline description that appears in the same paragraph after the colon.
fn try_parse_definition_term<'a>(node: &'a AstNode<'a>) -> Result<DefinitionPieces, FormatError> {
    if !matches!(node.data.borrow().value, NodeValue::Paragraph) {
        return Ok(None);
    }

    let mut children = node.children();
    let first = match children.next() {
        Some(child) => child,
        None => return Ok(None),
    };

    if !matches!(first.data.borrow().value, NodeValue::Strong) {
        return Ok(None);
    }

    // Gather the term content from the strong node
    let mut term_inlines = Vec::new();
    for child in first.children() {
        collect_inline_content(child, &mut term_inlines)?;
    }

    let mut description_inlines = Vec::new();
    let mut saw_colon = false;

    for child in children {
        let child_data = child.data.borrow();
        match &child_data.value {
            NodeValue::Text(text) => {
                if !saw_colon {
                    let trimmed = text.trim_start();
                    if let Some(rest) = trimmed.strip_prefix(':') {
                        saw_colon = true;
                        let rest = rest.trim_start();
                        if !rest.is_empty() {
                            description_inlines.push(InlineContent::Text(rest.to_string()));
                        }
                    } else {
                        // Text before a colon means this is not a definition pattern
                        return Ok(None);
                    }
                } else if !text.is_empty() {
                    description_inlines.push(InlineContent::Text(text.clone()));
                }
            }
            // Only collect additional inline nodes after we have seen the colon
            NodeValue::Strong
            | NodeValue::Emph
            | NodeValue::Code(_)
            | NodeValue::Link(_)
            | NodeValue::SoftBreak
            | NodeValue::LineBreak => {
                if !saw_colon {
                    return Ok(None);
                }
                collect_inline_content(child, &mut description_inlines)?;
            }
            _ => {
                if !saw_colon {
                    return Ok(None);
                }
            }
        }
    }

    if !saw_colon {
        return Ok(None);
    }

    Ok(Some((term_inlines, description_inlines)))
}

/// Collect sibling nodes, treating definition term patterns as Definition IR
/// and consuming subsequent siblings as the description until a heading or
/// another definition term is encountered.
fn collect_children_with_definitions<'a, I>(
    children: I,
    events: &mut Vec<Event>,
) -> Result<(), FormatError>
where
    I: Iterator<Item = &'a AstNode<'a>>,
{
    let mut iter = children.peekable();

    while let Some(node) = iter.next() {
        if let Some((term_inlines, inline_description)) = try_parse_definition_term(node)? {
            events.push(Event::StartDefinition);
            events.push(Event::StartDefinitionTerm);
            for inline in term_inlines {
                events.push(Event::Inline(inline));
            }
            events.push(Event::EndDefinitionTerm);

            events.push(Event::StartDefinitionDescription);
            if !inline_description.is_empty() {
                events.push(Event::StartParagraph);
                for inline in inline_description {
                    events.push(Event::Inline(inline));
                }
                events.push(Event::EndParagraph);
            }

            // Consume subsequent siblings as the description body until we hit
            // a heading or another definition term.
            while let Some(peek) = iter.peek() {
                if is_heading_node(peek) {
                    break;
                }

                // Stop if the next paragraph is another definition term
                let should_stop = try_parse_definition_term(peek)?.is_some();
                if should_stop {
                    break;
                }

                let next = iter.next().expect("peek yielded a node");
                collect_events_from_node(next, events)?;
            }

            events.push(Event::EndDefinitionDescription);
            events.push(Event::EndDefinition);
        } else {
            collect_events_from_node(node, events)?;
        }
    }

    Ok(())
}

/// Parse Lex annotation from HTML comment
/// Format: <!-- lex:label key1=val1 key2=val2 -->
/// Or multi-line:
/// <!-- lex:label key=val
/// Content
/// -->
#[allow(clippy::type_complexity)]
fn parse_lex_annotation(html: &str) -> Option<(String, Vec<(String, String)>, Option<String>)> {
    let trimmed = html.trim();
    if !trimmed.starts_with("<!-- lex:") || !trimmed.ends_with("-->") {
        return None;
    }

    // Remove <!-- lex: prefix and --> suffix
    let content_block = trimmed
        .strip_prefix("<!-- lex:")?
        .strip_suffix("-->")?
        .trim();

    // Split into header (label + params) and body (content)
    // If there is a newline, everything after is content
    let (header, body) = if let Some((h, b)) = content_block.split_once('\n') {
        (h, Some(b.trim().to_string()))
    } else {
        (content_block, None)
    };

    // Split header on whitespace
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    let label = parts[0].to_string();
    let mut parameters = vec![];

    for part in &parts[1..] {
        if let Some((key, value)) = part.split_once('=') {
            parameters.push((key.to_string(), value.to_string()));
        }
    }

    Some((label, parameters, body))
}

/// Parse Lex annotation closing tag from HTML comment
/// Supports both formats:
/// - Generic: <!-- /lex -->
/// - Label-specific: <!-- /lex:label -->
fn parse_lex_annotation_close(html: &str) -> Option<String> {
    let trimmed = html.trim();

    // Try label-specific format first: <!-- /lex:label -->
    if trimmed.starts_with("<!-- /lex:") && trimmed.ends_with("-->") {
        let label = trimmed
            .strip_prefix("<!-- /lex:")?
            .strip_suffix("-->")?
            .trim();
        return Some(label.to_string());
    }

    // Fall back to generic format: <!-- /lex -->
    if trimmed == "<!-- /lex -->" {
        // For generic closing tags, we'll use an empty label
        // The flat_to_nested converter will need to handle this gracefully
        return Some(String::new());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::ast::AstNode;

    #[test]
    fn test_simple_paragraph() {
        let md = "This is a simple paragraph.\n";
        let doc = parse_from_markdown(md).unwrap();

        // Verify we got a Lex document with a root session
        assert!(!doc.root.children.is_empty());
    }

    #[test]
    fn test_heading_to_session() {
        let md = "# Introduction\n\nSome content.\n";
        let doc = parse_from_markdown(md).unwrap();

        // Should have session with content
        assert!(!doc.root.children.is_empty());
    }

    #[test]
    fn test_code_block_to_verbatim() {
        let md = "```rust\nfn main() {}\n```\n";
        let doc = parse_from_markdown(md).unwrap();

        assert!(!doc.root.children.is_empty());
    }

    #[test]
    fn test_table_parsing() {
        let md = "|A|B|\n|-|-|\n|1|2|\n";
        let doc = parse_from_markdown(md).unwrap();

        println!("Children count: {}", doc.root.children.len());
        for child in &doc.root.children {
            println!("Child type: {}", child.node_type());
        }

        // Should have a table
        let has_table = doc.root.children.iter().any(|c| {
            if let lex_core::lex::ast::ContentItem::VerbatimBlock(v) = c {
                let mut text = String::new();
                for child in &v.children {
                    if let lex_core::lex::ast::ContentItem::VerbatimLine(l) = child {
                        text.push_str(l.content.as_string());
                        text.push('\n');
                    }
                }
                // Check for alignment (at least 3 dashes for separator)
                text.contains("| --- |")
            } else {
                false
            }
        });
        assert!(has_table, "Document should contain an aligned table");
    }
}
