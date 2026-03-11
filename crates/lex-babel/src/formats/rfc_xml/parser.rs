use crate::error::FormatError;
use crate::ir::nodes::{
    Definition, DocNode, Document, Heading, InlineContent, List, ListForm, ListItem, ListStyle,
    Paragraph, Verbatim,
};
use roxmltree::{Node, NodeType};

pub fn parse_to_ir(source: &str) -> Result<Document, FormatError> {
    let doc = roxmltree::Document::parse(source)
        .map_err(|e| FormatError::ParseError(format!("XML parsing error: {e}")))?;

    let root = doc.root_element();
    // RFC XML root is <rfc> or sometimes <internet-draft> (older?) -> spec says <rfc>
    if root.tag_name().name() != "rfc" {
        return Err(FormatError::ParseError(format!(
            "Root element is <{}>, expected <rfc>",
            root.tag_name().name()
        )));
    }

    let mut doc_children = Vec::new();
    let mut title_content = vec![InlineContent::Text("Untitled RFC".to_string())];

    // Process <front>
    if let Some(front) = root.children().find(|n| n.tag_name().name() == "front") {
        // Extract Title
        if let Some(title) = front.children().find(|n| n.tag_name().name() == "title") {
            title_content = parse_inline_content(title)?;
        }

        // Extract Abstract
        if let Some(abstract_node) = front.children().find(|n| n.tag_name().name() == "abstract") {
            doc_children.push(DocNode::Heading(Heading {
                level: 2,
                content: vec![InlineContent::Text("Abstract".to_string())],
                children: process_container_children(abstract_node, 2)?,
            }));
        }
    }

    // Process <middle>
    if let Some(middle) = root.children().find(|n| n.tag_name().name() == "middle") {
        doc_children.extend(process_middle(middle)?);
    }

    // Process <back>
    if let Some(back) = root.children().find(|n| n.tag_name().name() == "back") {
        doc_children.extend(process_back(back)?);
    }

    // Create the root session
    let root_heading = DocNode::Heading(Heading {
        level: 1,
        content: title_content,
        children: doc_children,
    });

    Ok(Document {
        children: vec![root_heading],
    })
}

fn process_middle(node: Node) -> Result<Vec<DocNode>, FormatError> {
    // Middle sections are level 2 (under Root)
    process_container_children(node, 1)
}

fn process_back(node: Node) -> Result<Vec<DocNode>, FormatError> {
    // Back sections are level 2
    process_container_children(node, 1)
}

fn process_container_children(
    node: Node,
    current_level: usize,
) -> Result<Vec<DocNode>, FormatError> {
    let mut nodes = Vec::new();
    for child in node.children() {
        if child.node_type() != NodeType::Element {
            continue;
        }

        match child.tag_name().name() {
            "section" => {
                let title_text = child
                    .attribute("title")
                    .map(|s| vec![InlineContent::Text(s.to_string())])
                    .or_else(|| {
                        child
                            .children()
                            .find(|n| n.tag_name().name() == "name")
                            .map(|n| parse_inline_content(n).unwrap_or_default())
                    })
                    .unwrap_or_else(|| vec![InlineContent::Text("Untitled".to_string())]);

                nodes.push(DocNode::Heading(Heading {
                    level: current_level + 1,
                    content: title_text,
                    children: process_container_children(child, current_level + 1)?,
                }));
            }
            "t" => {
                nodes.push(DocNode::Paragraph(Paragraph {
                    content: parse_inline_content(child)?,
                }));
            }
            "ul" | "ol" => {
                let ordered = child.tag_name().name() == "ol";
                let style = if ordered {
                    ListStyle::Numeric
                } else {
                    ListStyle::Bullet
                };
                nodes.push(DocNode::List(List {
                    ordered,
                    style,
                    form: ListForm::Short,
                    items: process_list_items(child, current_level)?,
                }));
            }
            "list" => {
                let xml_style = child.attribute("style").unwrap_or("empty");
                let ordered =
                    xml_style == "numbers" || xml_style == "letters" || xml_style == "format";
                let style = if ordered {
                    ListStyle::Numeric
                } else {
                    ListStyle::Bullet
                };
                nodes.push(DocNode::List(List {
                    ordered,
                    style,
                    form: ListForm::Short,
                    items: process_list_items(child, current_level)?,
                }));
            }
            "dl" => {
                nodes.extend(process_definition_list(child, current_level)?);
            }
            "figure" => {
                if let Some(artwork) = child.children().find(|n| {
                    n.tag_name().name() == "artwork" || n.tag_name().name() == "sourcecode"
                }) {
                    nodes.push(parse_verbatim(artwork)?);
                }
            }
            "artwork" | "sourcecode" => {
                nodes.push(parse_verbatim(child)?);
            }
            "note" => {
                let title_text = child
                    .attribute("title")
                    .map(|s| vec![InlineContent::Text(s.to_string())])
                    .or_else(|| {
                        child
                            .children()
                            .find(|n| n.tag_name().name() == "name")
                            .map(|n| parse_inline_content(n).unwrap_or_default())
                    })
                    .unwrap_or_else(|| vec![InlineContent::Text("Note".to_string())]);

                nodes.push(DocNode::Heading(Heading {
                    level: current_level + 1,
                    content: title_text,
                    children: process_container_children(child, current_level + 1)?,
                }));
            }
            "references" => {
                let title_text = child
                    .attribute("title")
                    .map(|s| vec![InlineContent::Text(s.to_string())])
                    .or_else(|| {
                        child
                            .children()
                            .find(|n| n.tag_name().name() == "name")
                            .map(|n| parse_inline_content(n).unwrap_or_default())
                    })
                    .unwrap_or_else(|| vec![InlineContent::Text("References".to_string())]);

                nodes.push(DocNode::Heading(Heading {
                    level: current_level + 1,
                    content: title_text,
                    children: process_container_children(child, current_level + 1)?,
                }));
            }
            "reference" => {
                let anchor = child.attribute("anchor").unwrap_or("?");
                let mut content = vec![
                    InlineContent::Reference(format!("[{anchor}]")),
                    InlineContent::Text(" ".to_string()),
                ];

                if let Some(front) = child.children().find(|n| n.tag_name().name() == "front") {
                    if let Some(title) = front.children().find(|n| n.tag_name().name() == "title") {
                        content.extend(parse_inline_content(title)?);
                    }
                }

                nodes.push(DocNode::Paragraph(Paragraph { content }));
            }
            _ => {
                // Skip unknown
            }
        }
    }
    Ok(nodes)
}

fn process_list_items(node: Node, level: usize) -> Result<Vec<ListItem>, FormatError> {
    let mut items = Vec::new();
    for child in node.children() {
        if child.node_type() != NodeType::Element {
            continue;
        }

        if child.tag_name().name() == "li" {
            let mut children = process_container_children(child, level)?;
            let mut content = Vec::new();

            if children.len() == 1 {
                if let Some(DocNode::Paragraph(para)) = children.first() {
                    content = para.content.clone();
                    children.clear();
                }
            }

            items.push(ListItem { content, children });
        } else if child.tag_name().name() == "t" {
            items.push(ListItem {
                content: parse_inline_content(child)?,
                children: Vec::new(),
            });
        }
    }
    Ok(items)
}

fn process_definition_list(node: Node, level: usize) -> Result<Vec<DocNode>, FormatError> {
    let mut definitions = Vec::new();
    let mut current_term = Vec::new();

    for child in node.children() {
        if child.node_type() != NodeType::Element {
            continue;
        }

        if child.tag_name().name() == "dt" {
            current_term = parse_inline_content(child)?;
        } else if child.tag_name().name() == "dd" {
            let description = process_container_children(child, level)?;
            definitions.push(DocNode::Definition(Definition {
                term: current_term.clone(),
                description,
            }));
            current_term.clear();
        }
    }
    Ok(definitions)
}

fn parse_verbatim(node: Node) -> Result<DocNode, FormatError> {
    let text = node.text().unwrap_or("").to_string();
    Ok(DocNode::Verbatim(Verbatim {
        subject: None,
        language: node.attribute("type").map(|s| s.to_string()),
        content: text,
    }))
}

fn parse_inline_content(node: Node) -> Result<Vec<InlineContent>, FormatError> {
    let mut content = Vec::new();
    for child in node.children() {
        match child.node_type() {
            NodeType::Text => {
                let text = child.text().unwrap_or("");
                if !text.is_empty() {
                    content.push(InlineContent::Text(text.to_string()));
                }
            }
            NodeType::Element => match child.tag_name().name() {
                "strong" | "b" => {
                    content.push(InlineContent::Bold(parse_inline_content(child)?));
                }
                "em" | "i" => {
                    content.push(InlineContent::Italic(parse_inline_content(child)?));
                }
                "tt" | "code" => {
                    content.push(InlineContent::Code(child.text().unwrap_or("").to_string()));
                }
                "xref" => {
                    let target = child.attribute("target").unwrap_or("?");
                    let mut inner_text = String::new();
                    for c in child.children() {
                        if c.node_type() == NodeType::Text {
                            inner_text.push_str(c.text().unwrap_or(""));
                        }
                    }

                    let text_to_show = if !inner_text.trim().is_empty() {
                        inner_text
                    } else {
                        target.to_string()
                    };

                    content.push(InlineContent::Reference(text_to_show));
                }
                "eref" => {
                    let target = child.attribute("target").unwrap_or("?");
                    let mut inner_text = String::new();
                    for c in child.children() {
                        if c.node_type() == NodeType::Text {
                            inner_text.push_str(c.text().unwrap_or(""));
                        }
                    }
                    let text = if !inner_text.trim().is_empty() {
                        inner_text
                    } else {
                        target.to_string()
                    };
                    content.push(InlineContent::Link {
                        text,
                        href: target.to_string(),
                    });
                }
                _ => {
                    content.extend(parse_inline_content(child)?);
                }
            },
            _ => {}
        }
    }
    Ok(content)
}
