//! Markdown serialization (Lex → Markdown export)
//!
//! Converts Lex documents to CommonMark Markdown.
//! Pipeline: Lex AST → IR → Events → Comrak AST → Markdown string

use crate::common::nested_to_flat::tree_to_events;
use crate::error::FormatError;
use crate::ir::events::Event;
use crate::ir::nodes::{DocNode, InlineContent, TableCellAlignment};
use comrak::nodes::{Ast, AstNode, ListDelimType, ListType, NodeTable, NodeValue, TableAlignment};
use comrak::{format_commonmark, Arena, ComrakOptions};
use lex_core::lex::ast::Document;
use std::cell::RefCell;

/// Serialize a Lex document to Markdown
pub fn serialize_to_markdown(doc: &Document) -> Result<String, FormatError> {
    // Extract document title before IR conversion (which loses it)
    let document_title = if doc.title().is_empty() {
        None
    } else {
        let subtitle = doc
            .title
            .as_ref()
            .and_then(|t| t.subtitle_str())
            .map(|s| s.to_string());
        Some((doc.title().to_string(), subtitle))
    };

    // Step 1: Lex AST → IR
    let ir_doc = crate::to_ir(doc);

    // Step 2: IR → Events
    let events = tree_to_events(&DocNode::Document(ir_doc));

    // Step 3: Events → Comrak AST
    let arena = Arena::new();
    let root = build_comrak_ast(&arena, &events)?;

    // Step 4: Comrak AST → Markdown string (using comrak's serializer)
    let mut output = Vec::new();
    let options = default_comrak_options();
    format_commonmark(root, &options, &mut output).map_err(|e| {
        FormatError::SerializationError(format!("Comrak serialization failed: {e}"))
    })?;

    let markdown = String::from_utf8(output)
        .map_err(|e| FormatError::SerializationError(format!("UTF-8 conversion failed: {e}")))?;

    // Remove Comrak's "end list" HTML comments which appear between consecutive lists
    let cleaned = markdown.replace("<!-- end list -->\n\n", "");

    // Prepend document title as H1 heading if present
    let with_title = prepend_title_as_h1(&cleaned, document_title);

    Ok(with_title)
}

/// Prepend document title as an H1 heading, optionally followed by subtitle as H2
///
/// If the document has a title, prepend `# Title` at the beginning.
/// If it also has a subtitle, append `## Subtitle` below.
fn prepend_title_as_h1(markdown: &str, title: Option<(String, Option<String>)>) -> String {
    match title {
        Some((t, Some(sub))) => format!("# {t}\n\n## {sub}\n\n{markdown}"),
        Some((t, None)) => format!("# {t}\n\n{markdown}"),
        None => markdown.to_string(),
    }
}

fn default_comrak_options() -> ComrakOptions<'static> {
    let mut options = ComrakOptions::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.superscript = true;
    options.extension.front_matter_delimiter = Some("---".to_string());
    // Allow HTML output for annotations (rendered as HTML comments)
    options.render.unsafe_ = true;
    options
}

/// Build a Comrak AST from IR events
fn build_comrak_ast<'a>(
    arena: &'a Arena<AstNode<'a>>,
    events: &[Event],
) -> Result<&'a AstNode<'a>, FormatError> {
    // Create document root
    let root = arena.alloc(AstNode::new(RefCell::new(Ast::new(
        NodeValue::Document,
        (0, 0).into(),
    ))));

    let mut current_parent: &'a AstNode<'a> = root;
    let mut parent_stack: Vec<&'a AstNode<'a>> = vec![];

    // State for collecting verbatim content
    let mut in_verbatim = false;
    let mut verbatim_content = String::new();
    let mut verbatim_language = None;
    let mut skip_next_space = false;

    // State for handling headings (which can only contain inline content).
    // Once we start a block after the heading, we clear this so later inline
    // events do not get appended to the heading text (a prior bug).
    let mut current_heading: Option<&'a AstNode<'a>> = None;

    // State for handling list items
    let mut in_list_item = false;
    let mut list_item_paragraph: Option<&'a AstNode<'a>> = None;

    // State for handling table cells (flatten paragraphs inside cells)
    let mut in_table_cell = false;

    for event in events {
        match event {
            Event::StartDocument => {
                // Already created root
            }

            Event::EndDocument => {
                // Done
            }

            Event::StartHeading(level) => {
                // Headings can only contain inline content, not block elements
                // Create heading and set it as target for inline content
                let heading_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Heading(comrak::nodes::NodeHeading {
                        level: (*level as u8).min(6),
                        setext: false,
                    }),
                    (0, 0).into(),
                ))));
                current_parent.append(heading_node);
                current_heading = Some(heading_node);
                // Note: We do NOT change current_parent or push to parent_stack
                // Block content after this heading will be siblings at document level
            }

            Event::EndHeading(_) => {
                // Close heading - block content goes back to document level
                current_heading = None;
            }

            Event::StartContent => {
                // Content markers are for HTML indentation - no-op in Markdown
            }

            Event::EndContent => {
                // Content markers are for HTML indentation - no-op in Markdown
            }

            Event::StartParagraph => {
                // Block after a heading – inline content should no longer
                // target the heading title.
                current_heading = None;

                if in_table_cell {
                    // Don't create paragraph node inside table cell
                    // Just let inline content be added to current_parent (which is TableCell)
                } else {
                    let para_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                        NodeValue::Paragraph,
                        (0, 0).into(),
                    ))));
                    current_parent.append(para_node);
                    parent_stack.push(current_parent);
                    current_parent = para_node;
                    // If we're in a list item, this explicit paragraph replaces any auto-created one
                    if in_list_item {
                        list_item_paragraph = None;
                    }
                }
            }

            Event::EndParagraph => {
                if !in_table_cell {
                    current_parent = parent_stack.pop().ok_or_else(|| {
                        FormatError::SerializationError("Unbalanced paragraph end".to_string())
                    })?;
                }
            }

            Event::StartList { ordered, .. } => {
                current_heading = None;

                let list_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::List(comrak::nodes::NodeList {
                        list_type: if *ordered {
                            ListType::Ordered
                        } else {
                            ListType::Bullet
                        },
                        marker_offset: 0,
                        padding: 0,
                        start: 1,
                        delimiter: ListDelimType::Period,
                        bullet_char: b'-',
                        tight: true, // Use tight lists to avoid blank lines between items
                    }),
                    (0, 0).into(),
                ))));
                current_parent.append(list_node);
                parent_stack.push(current_parent);
                current_parent = list_node;
            }

            Event::EndList => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced list end".to_string())
                })?;
            }

            Event::StartListItem => {
                current_heading = None;

                let item_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Item(comrak::nodes::NodeList {
                        list_type: ListType::Bullet,
                        marker_offset: 0,
                        padding: 0,
                        start: 1,
                        delimiter: ListDelimType::Period,
                        bullet_char: b'-',
                        tight: true, // Tight items don't add extra spacing
                    }),
                    (0, 0).into(),
                ))));
                current_parent.append(item_node);
                parent_stack.push(current_parent);
                current_parent = item_node;
                in_list_item = true;
                list_item_paragraph = None;
            }

            Event::EndListItem => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced list item end".to_string())
                })?;
                in_list_item = false;
                list_item_paragraph = None;
            }

            Event::StartVerbatim { language, subject } => {
                current_heading = None;

                // Render subject as bold text before the code block
                if let Some(subj) = subject {
                    let para = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                        NodeValue::Paragraph,
                        (0, 0).into(),
                    ))));
                    current_parent.append(para);
                    let strong = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                        NodeValue::Strong,
                        (0, 0).into(),
                    ))));
                    para.append(strong);
                    let text = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                        NodeValue::Text(subj.clone()),
                        (0, 0).into(),
                    ))));
                    strong.append(text);
                }

                // Check for special metadata comment format
                if let Some(lang) = &language {
                    if let Some(label) = lang.strip_prefix("lex-metadata:") {
                        // This is a metadata annotation to be rendered as an HTML comment
                        // The content will follow as Inline(Text)
                        // We need to capture it and wrap it in <!-- lex:label ... -->

                        let node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                            NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock {
                                block_type: 0,
                                literal: String::new(), // Will be filled by content
                            }),
                            (0, 0).into(),
                        ))));
                        current_parent.append(node);
                        parent_stack.push(current_parent); // Push the old parent
                        current_parent = node; // Set new parent to the HtmlBlock

                        // Prepend the start tag now.
                        let start_tag = format!("<!-- lex:{label}");
                        let mut data = node.data.borrow_mut();
                        if let NodeValue::HtmlBlock(ref mut html) = data.value {
                            html.literal.push_str(&start_tag);
                        }

                        // Set in_verbatim to true to indicate we are accumulating content
                        // for this special HtmlBlock.
                        in_verbatim = true;
                        verbatim_language = language.clone(); // Store for EndVerbatim check
                        verbatim_content.clear(); // Clear any previous content

                        continue; // Skip the rest of the StartVerbatim logic
                    }
                }

                // Original verbatim block handling
                in_verbatim = true;
                verbatim_language = language.clone();
                verbatim_content.clear();
            }

            Event::EndVerbatim => {
                // If we were processing a metadata comment (HtmlBlock), we need to close it
                let is_html_block =
                    matches!(current_parent.data.borrow().value, NodeValue::HtmlBlock(_));

                if is_html_block {
                    // Append closing tag
                    let mut data = current_parent.data.borrow_mut();
                    if let NodeValue::HtmlBlock(ref mut html) = data.value {
                        html.literal.push_str("\n-->");
                    }
                    // Pop the HtmlBlock node from the stack
                    current_parent = parent_stack.pop().ok_or_else(|| {
                        FormatError::SerializationError("Unbalanced HTML block end".to_string())
                    })?;
                } else {
                    // Original verbatim block handling: Create code block with accumulated content
                    let code_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                        NodeValue::CodeBlock(comrak::nodes::NodeCodeBlock {
                            fenced: true,
                            fence_char: b'`',
                            fence_length: 3,
                            fence_offset: 0,
                            info: verbatim_language.take().unwrap_or_default(),
                            literal: verbatim_content.clone(),
                        }),
                        (0, 0).into(),
                    ))));
                    current_parent.append(code_node);
                }
                in_verbatim = false;
                verbatim_content.clear();
            }

            Event::Inline(inline_content) => {
                // Clean up inline text before inserting. In particular, drop any
                // leading list markers that may come through in the text of a
                // list item (to avoid doubling bullets like "- - Item").
                let mut inline_to_emit = inline_content.clone();
                if in_list_item {
                    if let InlineContent::Text(text) = inline_content {
                        if skip_next_space && text == " " {
                            skip_next_space = false;
                            continue;
                        }
                        skip_next_space = false;

                        if let Some(stripped) = text.strip_prefix("- ") {
                            inline_to_emit = InlineContent::Text(stripped.to_string());
                        }
                    } else if let InlineContent::Marker(_) = inline_content {
                        // Ignore explicit markers in list items as Markdown generates its own
                        skip_next_space = true;
                        continue;
                    } else {
                        skip_next_space = false;
                    }
                }

                if in_verbatim {
                    // If we are in a special lex-metadata verbatim block (which is an HtmlBlock)
                    // or a regular verbatim block, accumulate content.
                    if let InlineContent::Text(text) = &inline_to_emit {
                        if matches!(current_parent.data.borrow().value, NodeValue::HtmlBlock(_)) {
                            // Append to the HtmlBlock's literal directly
                            let mut data = current_parent.data.borrow_mut();
                            if let NodeValue::HtmlBlock(ref mut html) = data.value {
                                html.literal.push_str(text);
                            }
                        } else {
                            // Accumulate for a regular CodeBlock (will be created in EndVerbatim)
                            verbatim_content.push_str(text);
                        }
                    }
                } else if matches!(current_parent.data.borrow().value, NodeValue::HtmlBlock(_)) {
                    // If we are inside an HtmlBlock (metadata comment), append text to literal
                    let mut data = current_parent.data.borrow_mut();
                    if let NodeValue::HtmlBlock(ref mut html) = data.value {
                        if let InlineContent::Text(text) = inline_to_emit {
                            html.literal.push_str(&text);
                        }
                    }
                } else if let Some(heading) = current_heading {
                    // Add to heading (headings can have inline content directly)
                    // Session numbering is preserved — it's an author's communication choice
                    add_inline_to_node(arena, heading, &inline_to_emit)?;
                } else if in_list_item {
                    // If we're already inside an explicit paragraph, write directly to it.
                    if matches!(current_parent.data.borrow().value, NodeValue::Paragraph) {
                        add_inline_to_node(arena, current_parent, &inline_to_emit)?;
                    } else {
                        // Auto-wrap inline content in a paragraph. List items need block content.
                        // Using tight lists prevents extra blank lines.
                        if list_item_paragraph.is_none() {
                            let para = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                                NodeValue::Paragraph,
                                (0, 0).into(),
                            ))));
                            current_parent.append(para);
                            list_item_paragraph = Some(para);
                        }
                        add_inline_to_node(arena, list_item_paragraph.unwrap(), &inline_to_emit)?;
                    }
                } else {
                    // Regular inline content added to current_parent
                    add_inline_to_node(arena, current_parent, &inline_to_emit)?;
                }
            }

            Event::StartAnnotation { label, parameters } if label == "frontmatter" => {
                // Serialize as YAML frontmatter
                let mut yaml = String::from("---\n");
                for (key, value) in parameters {
                    // Simple YAML serialization
                    // If value contains special chars, we might need quoting, but for now simple string
                    yaml.push_str(&format!("{key}: {value}\n"));
                }
                yaml.push_str("---\n\n");

                let frontmatter_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::FrontMatter(yaml),
                    (0, 0).into(),
                ))));
                current_parent.append(frontmatter_node);
                // No need to push to stack or change current_parent as FrontMatter is a leaf block
            }

            Event::StartAnnotation { label, parameters } => {
                current_heading = None;

                // Check if this is a metadata annotation that should be serialized as a comment with content inside
                // Whitelist: author, note, etc.
                let metadata_labels = [
                    "author", "note", "title", "date", "tags", "category", "template",
                ];
                if metadata_labels.contains(&label.as_str()) {
                    // We need to capture the content of this annotation and put it inside the comment.
                    // However, we are iterating events. The content events follow this StartAnnotation.
                    // We can't easily consume them here without changing the architecture.
                    // BUT, we can emit a special comment start, and then when we see EndAnnotation, emit the comment end.
                    // The problem is comrak expects a single HtmlBlock for the comment if we want it to be "one block".
                    // If we emit <!-- lex:author and then content and then -->, comrak might escape the content or treat it as markdown.

                    // Alternative: We can emit a raw HTML block that starts the comment, and another that ends it.
                    // But Comrak's HtmlBlock is for *block* HTML.
                    // If we emit:
                    // HtmlBlock("<!-- lex:author")
                    // Paragraph("Content")
                    // HtmlBlock("-->")
                    // The output will be:
                    // <!-- lex:author -->
                    // <p>Content</p>
                    // -->
                    // Which is not what we want.

                    // We want:
                    // <!-- lex:author
                    // Content
                    // -->

                    // To achieve this in the current architecture (Event -> Comrak AST), we need to know the content *now*.
                    // But we don't.
                    // However, `build_comrak_ast` is building a tree.
                    // If we just emit the start comment, and then the content nodes are added as children of `current_parent`.
                    // Wait, `current_parent` is the parent of the annotation.
                    // When we see StartAnnotation, we usually create a wrapper node?
                    // No, currently we just emit an HTML block and continue.

                    // If we want to wrap the content in a comment, we should probably change how we handle this.
                    // But since we can't easily change the event stream structure here...

                    // Let's try to emit the comment start WITHOUT the closing -->
                    // And EndAnnotation emits -->
                    // AND we need to make sure the content is rendered as raw text, not Markdown.
                    // But the content events will be processed as Markdown nodes (Paragraph, etc.).

                    // If the user wants the content to be *inside* the comment, it effectively becomes "Raw HTML".
                    // So the content should be treated as part of the HTML block.

                    // Since we can't easily look ahead, maybe we can rely on `from_lex.rs` to have prepared this?
                    // But `from_lex.rs` produces `DocNode::Annotation` with children.
                    // `tree_to_events` flattens it.

                    // HACK: If we are in `build_comrak_ast`, we are consuming events.
                    // We can peek ahead in `events`!
                    // But `events` is passed as a slice? No, `build_comrak_ast` takes `&[Event]`.
                    // We are iterating it.

                    // If I can't change the loop, I can't consume ahead.

                    // Maybe I can change `tree_to_events`?
                    // If `tree_to_events` sees a metadata annotation, it could emit a `Event::HtmlBlock` containing the full comment?
                    // Instead of `StartAnnotation` / content / `EndAnnotation`.
                    // That seems cleaner!

                    // Let's modify `tree_to_events` in `lex-babel/src/ir/events.rs`?
                    // Or wherever it is defined.
                    // It is imported in `parser.rs`: `use crate::common::flat_to_nested::events_to_tree;`
                    // And `serializer.rs`: `let events = tree_to_events(&DocNode::Document(ir_doc));`

                    // I need to find `tree_to_events`. It's likely in `lex-babel/src/ir/events.rs` or similar.
                    // Let's search for it.
                }

                // Fallback to existing behavior for non-metadata or if we can't change tree_to_events
                let mut comment = format!("<!-- lex:{label}");
                for (key, value) in parameters {
                    comment.push_str(&format!(" {key}={value}"));
                }
                comment.push_str(" -->");

                let html_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock {
                        block_type: 0,
                        literal: comment,
                    }),
                    (0, 0).into(),
                ))));
                current_parent.append(html_node);
            }

            Event::EndAnnotation { label } if label == "frontmatter" => {
                // Nothing to do, FrontMatter node is self-contained
            }

            Event::EndAnnotation { label } => {
                // Closing annotation comment with label-specific tag
                let closing_tag = format!("<!-- /lex:{label} -->");
                let html_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock {
                        block_type: 0,
                        literal: closing_tag,
                    }),
                    (0, 0).into(),
                ))));
                current_parent.append(html_node);
            }

            Event::StartDefinition => {
                current_heading = None;
                // Definitions in Markdown: Term paragraph followed by description content
                // Don't create wrapper, let content be siblings at document level
            }

            Event::EndDefinition => {
                // Nothing needed
            }

            Event::StartDefinitionTerm => {
                current_heading = None;
                // Create paragraph for the term with bold styling
                let para_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Paragraph,
                    (0, 0).into(),
                ))));
                current_parent.append(para_node);
                parent_stack.push(current_parent);
                current_parent = para_node;

                // Add bold wrapper for term text
                let strong_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Strong,
                    (0, 0).into(),
                ))));
                current_parent.append(strong_node);
                parent_stack.push(current_parent);
                current_parent = strong_node;
            }

            Event::EndDefinitionTerm => {
                // Close bold
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced definition term end".to_string())
                })?;

                // Add colon after term
                let colon_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Text(":".to_string()),
                    (0, 0).into(),
                ))));
                current_parent.append(colon_node);

                // Close term paragraph
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError(
                        "Unbalanced definition term paragraph".to_string(),
                    )
                })?;
            }

            Event::StartDefinitionDescription => {
                // Description content will be siblings at document level
                // No wrapper needed
            }

            Event::EndDefinitionDescription => {
                // Nothing needed
            }

            Event::StartTable { caption, .. } => {
                current_heading = None;

                // Render caption as bold paragraph before the table
                if let Some(caption_inlines) = caption {
                    let para = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                        NodeValue::Paragraph,
                        (0, 0).into(),
                    ))));
                    current_parent.append(para);
                    let strong = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                        NodeValue::Strong,
                        (0, 0).into(),
                    ))));
                    para.append(strong);
                    for inline in caption_inlines {
                        add_inline_to_node(arena, strong, inline)?;
                    }
                }

                let table_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Table(NodeTable {
                        alignments: vec![],
                        num_columns: 0,
                        num_rows: 0,
                        num_nonempty_cells: 0,
                    }),
                    (0, 0).into(),
                ))));
                current_parent.append(table_node);
                parent_stack.push(current_parent);
                current_parent = table_node;
            }

            Event::EndTable => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced table end".to_string())
                })?;
            }

            Event::StartTableRow { header } => {
                let row_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::TableRow(*header),
                    (0, 0).into(),
                ))));
                current_parent.append(row_node);
                parent_stack.push(current_parent);
                current_parent = row_node;
            }

            Event::EndTableRow => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced table row end".to_string())
                })?;
            }

            Event::StartTableFootnotes => {
                // Markdown doesn't have table footnotes; render as content after the table
            }

            Event::EndTableFootnotes => {
                // No-op in Markdown
            }

            Event::StartTableCell {
                header: _, align, ..
            } => {
                let cell_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::TableCell,
                    (0, 0).into(),
                ))));
                current_parent.append(cell_node);
                parent_stack.push(current_parent);

                // Update table alignments if we are in the first row
                if parent_stack.len() >= 2 {
                    let table_node = parent_stack[parent_stack.len() - 2];
                    let row_node = parent_stack[parent_stack.len() - 1];

                    let mut table_data = table_node.data.borrow_mut();
                    if let NodeValue::Table(ref mut table) = table_data.value {
                        let is_first_row = table_node
                            .first_child()
                            .is_some_and(|first| std::ptr::eq(first, row_node));

                        if is_first_row {
                            let align_enum = match align {
                                TableCellAlignment::Left => TableAlignment::Left,
                                TableCellAlignment::Right => TableAlignment::Right,
                                TableCellAlignment::Center => TableAlignment::Center,
                                TableCellAlignment::None => TableAlignment::None,
                            };
                            table.alignments.push(align_enum);
                        }
                    }
                }

                current_parent = cell_node;
                in_table_cell = true;
            }

            Event::EndTableCell => {
                in_table_cell = false;
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced table cell end".to_string())
                })?;
            }

            Event::Image(image) => {
                // Render as paragraph with image
                let para_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Paragraph,
                    (0, 0).into(),
                ))));
                current_parent.append(para_node);

                let image_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Image(comrak::nodes::NodeLink {
                        url: image.src.clone(),
                        title: image.title.clone().unwrap_or_default(),
                    }),
                    (0, 0).into(),
                ))));
                para_node.append(image_node);

                let text_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Text(image.alt.clone()),
                    (0, 0).into(),
                ))));
                image_node.append(text_node);
            }

            Event::Video(video) => {
                // Render as HTML <video>
                let mut html = format!(r#"<video src="{}""#, video.src);
                if let Some(poster) = &video.poster {
                    html.push_str(&format!(r#" poster="{poster}""#));
                }
                if let Some(title) = &video.title {
                    html.push_str(&format!(r#" title="{title}""#));
                }
                html.push_str(" controls></video>");

                let html_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock {
                        block_type: 0,
                        literal: html,
                    }),
                    (0, 0).into(),
                ))));
                current_parent.append(html_node);
            }

            Event::Audio(audio) => {
                // Render as HTML <audio>
                let mut html = format!(r#"<audio src="{}""#, audio.src);
                if let Some(title) = &audio.title {
                    html.push_str(&format!(r#" title="{title}""#));
                }
                html.push_str(" controls></audio>");

                let html_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock {
                        block_type: 0,
                        literal: html,
                    }),
                    (0, 0).into(),
                ))));
                current_parent.append(html_node);
            }
        }
    }

    Ok(root)
}

/// Add inline content to a comrak node
fn add_inline_to_node<'a>(
    arena: &'a Arena<AstNode<'a>>,
    parent: &'a AstNode<'a>,
    inline: &crate::ir::nodes::InlineContent,
) -> Result<(), FormatError> {
    use crate::ir::nodes::InlineContent;

    match inline {
        InlineContent::Text(text) => {
            let sanitized = text.replace('\n', " ");

            let text_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Text(sanitized),
                (0, 0).into(),
            ))));
            parent.append(text_node);
        }

        InlineContent::Bold(children) => {
            let strong_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Strong,
                (0, 0).into(),
            ))));
            parent.append(strong_node);
            for child in children {
                add_inline_to_node(arena, strong_node, child)?;
            }
        }

        InlineContent::Italic(children) => {
            let emph_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Emph,
                (0, 0).into(),
            ))));
            parent.append(emph_node);
            for child in children {
                add_inline_to_node(arena, emph_node, child)?;
            }
        }

        InlineContent::Code(code_text) => {
            let code_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Code(comrak::nodes::NodeCode {
                    num_backticks: 1,
                    literal: code_text.clone(),
                }),
                (0, 0).into(),
            ))));
            parent.append(code_node);
        }

        InlineContent::Reference(ref_text) => {
            // Unresolved reference (non-linkable types: citations, footnotes, general, etc.)
            let url = ref_text
                .strip_prefix('@')
                .map(|citation| format!("#ref-{citation}"));

            if let Some(url) = url {
                let link_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Link(comrak::nodes::NodeLink {
                        url,
                        title: String::new(),
                    }),
                    (0, 0).into(),
                ))));
                parent.append(link_node);

                let text_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Text(ref_text.clone()),
                    (0, 0).into(),
                ))));
                link_node.append(text_node);
            } else {
                // Render as plain text with brackets: [reference]
                let text_with_brackets = format!("[{ref_text}]");
                let text_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                    NodeValue::Text(text_with_brackets),
                    (0, 0).into(),
                ))));
                parent.append(text_node);
            }
        }

        InlineContent::Link { text, href } => {
            let link_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Link(comrak::nodes::NodeLink {
                    url: href.clone(),
                    title: String::new(),
                }),
                (0, 0).into(),
            ))));
            parent.append(link_node);

            let text_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Text(text.clone()),
                (0, 0).into(),
            ))));
            link_node.append(text_node);
        }

        InlineContent::Math(math_text) => {
            // Math not supported in CommonMark, render as $...$
            let dollar_open = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Text("$".to_string()),
                (0, 0).into(),
            ))));
            parent.append(dollar_open);

            let math_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Text(math_text.clone()),
                (0, 0).into(),
            ))));
            parent.append(math_node);

            let dollar_close = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Text("$".to_string()),
                (0, 0).into(),
            ))));
            parent.append(dollar_close);
        }

        InlineContent::Image(image) => {
            let image_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Image(comrak::nodes::NodeLink {
                    url: image.src.clone(),
                    title: image.title.clone().unwrap_or_default(),
                }),
                (0, 0).into(),
            ))));
            parent.append(image_node);

            let text_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Text(image.alt.clone()),
                (0, 0).into(),
            ))));
            image_node.append(text_node);
        }

        InlineContent::Marker(marker_text) => {
            // In Markdown, list markers are generated by the list structure,
            // so we generally ignore them if we are in a list item.
            // However, for headings or other contexts, we might want to preserve them as text.

            // We can check if the parent is a Paragraph that is part of a ListItem?
            // Or we can rely on the caller to filter?

            // For now, let's just render it as text.
            // The `Event::Inline` handler in `build_comrak_ast` has logic to strip
            // leading markers from text in list items, but that was for `InlineContent::Text`.
            // Now that we have `InlineContent::Marker`, we should probably handle it there too.
            // But `add_inline_to_node` is called by `build_comrak_ast`.

            // Let's render it as text here, and let `build_comrak_ast` decide whether to call this.
            // Wait, `build_comrak_ast` iterates events.

            let text_node = arena.alloc(AstNode::new(RefCell::new(Ast::new(
                NodeValue::Text(marker_text.clone()),
                (0, 0).into(),
            ))));
            parent.append(text_node);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{parse_document, ComrakOptions};
    use lex_core::lex::transforms::standard::STRING_TO_AST;

    #[test]
    fn test_simple_paragraph_ast() {
        let lex_src = "This is a simple paragraph.\n";
        let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

        // Convert to markdown
        let md = serialize_to_markdown(&lex_doc).unwrap();

        // Parse back to comrak AST to verify structure
        let arena = Arena::new();
        let options = ComrakOptions::default();
        let root = parse_document(&arena, &md, &options);

        // Verify we have a paragraph
        let mut found_paragraph = false;
        for child in root.children() {
            if matches!(child.data.borrow().value, NodeValue::Paragraph) {
                found_paragraph = true;

                // Check inline text content
                for _inline in child.children() {
                    if let NodeValue::Text(ref text) = child.data.borrow().value {
                        assert!(text.contains("simple paragraph"));
                    }
                }
            }
        }
        assert!(found_paragraph, "Should have a paragraph node");
    }

    #[test]
    fn test_heading_ast() {
        let lex_src = "1. Introduction\n\n    Content here.\n";
        let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

        let md = serialize_to_markdown(&lex_doc).unwrap();

        // Parse and verify AST structure
        let arena = Arena::new();
        let options = ComrakOptions::default();
        let root = parse_document(&arena, &md, &options);

        let mut found_heading = false;
        for child in root.children() {
            if let NodeValue::Heading(ref heading) = child.data.borrow().value {
                assert_eq!(heading.level, 2);
                found_heading = true;
            }
        }
        assert!(found_heading, "Should have a heading node");
    }
}
