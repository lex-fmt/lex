use lex_analysis::inline::extract_references;
use lex_analysis::utils::collect_footnote_definitions;
use lex_core::lex::ast::{ContentItem, Document, Range, Session, TextContent};
use lex_core::lex::inlines::ReferenceType;
use std::collections::HashMap;

/// Reorders footnotes in the document to be sequential (1, 2, 3...) based on appearance.
/// Returns the new document content.
pub fn reorder_footnotes(document: &Document, source: &str) -> String {
    let mut references = Vec::new();

    // 1. Collect all footnote references in order
    // Use local traversal to avoid issues with lex-analysis traversal
    traverse_document(document, &mut |text| {
        for reference in extract_references(text) {
            if let ReferenceType::FootnoteNumber { number } = reference.reference_type {
                references.push((number, reference.range));
            }
        }
    });

    let mut mapping: HashMap<u32, u32> = HashMap::new();
    let mut intended_reference_replacements = Vec::new();
    let mut next_id = 1;

    for (old_number, range) in references {
        let new_number = *mapping.entry(old_number).or_insert_with(|| {
            let n = next_id;
            next_id += 1;
            n
        });
        intended_reference_replacements.push((range, new_number));
    }

    // 2. Identify Definition replacements from :: notes ::-annotated lists
    let mut definition_replacements = Vec::new();
    let offsets = line_offsets(source);

    for (label, item_range) in collect_footnote_definitions(document) {
        if let Ok(old_number) = label.parse::<u32>() {
            if let Some(&new_number) = mapping.get(&old_number) {
                // Find the marker number within the list item's first line
                if let Some(marker_range) =
                    find_number_in_range(source, &item_range, &label, &offsets)
                {
                    definition_replacements.push((marker_range, new_number));
                }
            }
        }
    }

    // 3. Apply replacements
    #[derive(Clone, Copy)]
    enum ReplacementKind {
        Reference(u32),
        Definition(u32),
    }

    let mut edits: Vec<(Range, ReplacementKind)> = Vec::new();
    for (range, new_val) in intended_reference_replacements {
        edits.push((range, ReplacementKind::Reference(new_val)));
    }
    for (range, new_val) in definition_replacements {
        edits.push((range, ReplacementKind::Definition(new_val)));
    }

    // Convert Range to (start_byte, end_byte, kind)
    let mut byte_edits: Vec<(usize, usize, ReplacementKind)> = edits
        .iter()
        .map(|(range, kind)| {
            let start = pos_to_byte(&offsets, range.start);
            let end = pos_to_byte(&offsets, range.end);
            (start, end, *kind)
        })
        .collect();

    // Sort by start desc
    // Note: If ranges overlap, this handling is naive. But references/definitions shouldn't overlap.
    byte_edits.sort_by(|a, b| b.0.cmp(&a.0));

    let mut new_source = source.to_string();
    for (start, end, kind) in byte_edits {
        if start <= end && end <= new_source.len() {
            let original = &new_source[start..end];
            let replacement = match kind {
                ReplacementKind::Reference(n) => n.to_string(),
                ReplacementKind::Definition(n) => {
                    // Preserve padding
                    let leading_space = original
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .collect::<String>();
                    let trailing_space = original
                        .chars()
                        .rev()
                        .take_while(|c| c.is_whitespace())
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>();
                    format!("{leading_space}{n}{trailing_space}")
                }
            };

            new_source.replace_range(start..end, &replacement);
        }
    }

    new_source
}

fn line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (i, ch) in source.char_indices() {
        if ch == '\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

/// Finds the byte range of a number string within the first line of a range.
fn find_number_in_range(
    source: &str,
    item_range: &Range,
    number_str: &str,
    offsets: &[usize],
) -> Option<Range> {
    let start_byte = pos_to_byte(offsets, item_range.start);
    if start_byte >= source.len() {
        return None;
    }
    let search_area = &source[start_byte..];
    let line_end = search_area.find('\n').unwrap_or(search_area.len());
    let line = &search_area[..line_end];
    if let Some(idx) = line.find(number_str) {
        let abs_start = start_byte + idx;
        let abs_end = abs_start + number_str.len();
        let start_pos = byte_to_pos(abs_start, offsets);
        let end_pos = byte_to_pos(abs_end, offsets);
        return Some(Range {
            start: start_pos,
            end: end_pos,
            span: abs_start..abs_end,
        });
    }
    None
}

fn byte_to_pos(byte: usize, offsets: &[usize]) -> lex_core::lex::ast::Position {
    let mut line = 0;
    for (i, &off) in offsets.iter().enumerate() {
        if off > byte {
            break;
        }
        line = i;
    }
    let line_start = offsets[line];
    let col = byte - line_start;
    lex_core::lex::ast::Position::new(line, col)
}

fn pos_to_byte(offsets: &[usize], pos: lex_core::lex::ast::Position) -> usize {
    let line_start = *offsets
        .get(pos.line)
        .unwrap_or(offsets.last().unwrap_or(&0));
    line_start + pos.column
}

fn traverse_document<F>(document: &Document, f: &mut F)
where
    F: FnMut(&TextContent),
{
    if let Some(title) = &document.title {
        f(&title.content);
    }
    visit_session(&document.root, true, f);
    for annotation in document.annotations() {
        for child in annotation.children.iter() {
            visit_content(child, f);
        }
    }
}

fn visit_session<F>(session: &Session, is_root: bool, f: &mut F)
where
    F: FnMut(&TextContent),
{
    if !is_root {
        f(&session.title);
    }
    for child in &session.children {
        visit_content(child, f);
    }
    for annotation in session.annotations() {
        for child in annotation.children.iter() {
            visit_content(child, f);
        }
    }
}

fn visit_content<F>(item: &ContentItem, f: &mut F)
where
    F: FnMut(&TextContent),
{
    match item {
        ContentItem::Paragraph(p) => {
            for line in &p.lines {
                if let ContentItem::TextLine(l) = line {
                    f(&l.content);
                }
            }
        }
        ContentItem::Session(s) => visit_session(s, false, f),
        ContentItem::Definition(d) => {
            for child in &d.children {
                visit_content(child, f);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::testing::lexplore::Lexplore;

    #[test]
    fn reorders_references_and_definitions() {
        // footnotes-06: [2] then [1] at root; renumbering swaps both refs and list markers.
        let loader = Lexplore::footnotes(6);
        let source = loader.source();
        let doc = loader.parse().unwrap();
        let new_source = reorder_footnotes(&doc, &source);

        // Refs swap: first [2]→[1], then [1]→[2].
        assert!(new_source.contains("swapped: [1] before [2]"));
        // List markers swap: the item originally at "1." now prints as "2." and vice versa.
        assert!(new_source.contains("2. Note A"));
        assert!(new_source.contains("1. Note B"));
    }

    #[test]
    fn keeps_correct_order_for_repeated_refs() {
        // footnotes-07: refs [10] [10] [5]; definitions 5 and 10 inside a Notes session.
        let loader = Lexplore::footnotes(7);
        let source = loader.source();
        let doc = loader.parse().unwrap();
        let new_source = reorder_footnotes(&doc, &source);

        // Reference 10 → 1 (seen first, twice), reference 5 → 2.
        assert!(new_source.contains("References [1] then [1] then [2]"));
        // Definition "5." → "2.", definition "10." → "1.".
        assert!(new_source.contains("    2. Content."));
        assert!(new_source.contains("    1. Other."));
    }

    #[test]
    fn reorders_list_items_in_notes_session() {
        // footnotes-10: [2] before [1] inside a session-wrapped :: notes :: list.
        let loader = Lexplore::footnotes(10);
        let source = loader.source();
        let doc = loader.parse().unwrap();
        let new_source = reorder_footnotes(&doc, &source);

        assert!(new_source.contains("References [1] and [2] appear"));
    }
}
