//! AST → parity skeleton transform.
//!
//! Produces a plain-text block skeleton for parity checking against
//! tree-sitter. Backs the `parity` transform.

/// Produce a plain-text block skeleton for parity checking against tree-sitter.
///
/// The format is designed so that both the reference parser (lex-core) and the
/// tree-sitter parser can produce identical output with minimal transformation.
/// The reference side (this function) adapts to tree-sitter's natural structure:
/// - Individual BlankLine entries (not collapsed BlankLineGroup)
/// - Verbatim groups emitted as separate subject/content pairs
/// - No inline markup, locations, parameters, or closing labels
pub(super) fn ast_to_parity(doc: &lex_core::lex::parsing::Document) -> String {
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
            parity_line(out, depth, &format!("Table \"{}\"", t.subject.as_string()));
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
