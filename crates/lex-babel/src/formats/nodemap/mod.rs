use lex_core::lex::ast::elements::annotation::Annotation;
use lex_core::lex::ast::elements::blank_line_group::BlankLineGroup;
use lex_core::lex::ast::elements::definition::Definition;
use lex_core::lex::ast::elements::list::{List, ListItem};
use lex_core::lex::ast::elements::paragraph::{Paragraph, TextLine};
use lex_core::lex::ast::elements::session::Session;
use lex_core::lex::ast::elements::verbatim::{Verbatim, VerbatimGroupItemRef};
use lex_core::lex::ast::elements::verbatim_line::VerbatimLine;
use lex_core::lex::ast::range::Position;
use lex_core::lex::ast::traits::{AstNode, Visitor};
use lex_core::lex::ast::Document;
use std::collections::{HashMap, HashSet};

/// Renders the AST as a character map where each source position is represented
/// by a character corresponding to the deepest AST node at that position.
pub fn to_nodemap_str_with_params(
    doc: &Document,
    source: &str,
    params: &HashMap<String, String>,
) -> String {
    let use_color = params
        .get("color")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    let use_color_char = params
        .get("colorchar")
        .or(params.get("color-char"))
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    let include_summary = params
        .get("nodesummary")
        .or(params.get("node-summary"))
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    let render_mode = if use_color_char {
        RenderMode::ColorChar
    } else if use_color {
        RenderMode::Color
    } else {
        RenderMode::Base2048
    };

    // 1. Pre-compute line offsets (in chars)
    let mut line_starts = Vec::new();
    let mut current_char_idx = 0;
    line_starts.push(0);

    for c in source.chars() {
        current_char_idx += 1;
        if c == '\n' {
            line_starts.push(current_char_idx);
        }
    }

    let total_chars = current_char_idx;

    // Map of linear index to NodeId (usize)
    // 0 means no node (or root context)
    let mut node_map: Vec<usize> = vec![0; total_chars];

    // 2. Traverse AST and fill node_map
    let mut node_sizes = HashMap::new();
    let mut visitor = NodeMapVisitor {
        map: &mut node_map,
        line_starts: &line_starts,
        next_id: 1,
        total_chars,
        node_sizes: &mut node_sizes,
    };

    doc.accept(&mut visitor);

    // 3. Render
    let mut final_output = String::with_capacity(total_chars * 20);
    let chars: Vec<char> = source.chars().collect();

    for (i, &node_id) in node_map.iter().enumerate() {
        if i < chars.len() && chars[i] == '\n' {
            if matches!(render_mode, RenderMode::Color | RenderMode::ColorChar) {
                final_output.push_str("\x1b[0m");
            }
            final_output.push('\n');
            continue;
        }

        match render_mode {
            RenderMode::Base2048 => {
                final_output.push(get_base2048_char(node_id));
            }
            RenderMode::Color => {
                let (r, g, b) = get_color_for_id(node_id);
                final_output.push_str(&format!("\x1b[38;2;{r};{g};{b}m█"));
            }
            RenderMode::ColorChar => {
                let (r, g, b) = get_color_for_id(node_id);
                let c = get_base2048_char(node_id);
                final_output.push_str(&format!("\x1b[38;2;{r};{g};{b}m{c}"));
            }
        }
    }
    if matches!(render_mode, RenderMode::Color | RenderMode::ColorChar) {
        final_output.push_str("\x1b[0m");
    }

    // 4. Summary
    if include_summary {
        final_output.push_str(
            "\n--------------------------------------------------------------------------------\n",
        );

        // Identify represented nodes (unique non-zero IDs in map)
        let represented_ids: HashSet<usize> =
            node_map.iter().cloned().filter(|&id| id != 0).collect();
        let count = represented_ids.len();

        final_output.push_str(&format!("Ast Nodes = {count}\n\n"));

        // Collect sizes of represented nodes
        let mut sizes: Vec<usize> = represented_ids
            .iter()
            .filter_map(|id| node_sizes.get(id).cloned())
            .collect();
        sizes.sort_unstable();

        let median = if sizes.is_empty() {
            0
        } else {
            sizes[sizes.len() / 2]
        };

        final_output.push_str(&format!("Median Node Size = {median}\n\n"));

        // Size distribution for 1..=5
        for size in 1..=5 {
            let count_size = sizes.iter().filter(|&&s| s == size).count();
            final_output.push_str(&format!("{size} char ast node = {count_size}\n"));
        }
    }

    final_output
}

enum RenderMode {
    Base2048,
    Color,
    ColorChar,
}

struct NodeMapVisitor<'a> {
    map: &'a mut Vec<usize>,
    line_starts: &'a [usize],
    next_id: usize,
    total_chars: usize,
    node_sizes: &'a mut HashMap<usize, usize>,
}

impl<'a> NodeMapVisitor<'a> {
    fn fill_range(&mut self, range: &lex_core::lex::ast::range::Range) {
        let start_idx = self.pos_to_index(range.start);
        let end_idx = self.pos_to_index(range.end);
        let id = self.next_id;
        self.next_id += 1;

        // Record node size (length in chars)
        // Range end is typically exclusive in standard range notation but inclusive in some ASTs.
        // Lex parser ranges are usually byte offsets or line/col.
        // Here we are calculating char length in the source string via index mapping.
        // start_idx and end_idx are indices in `chars` vector space.
        // Size = end_idx - start_idx.
        let size = end_idx.saturating_sub(start_idx);
        self.node_sizes.insert(id, size);

        for i in start_idx..end_idx.min(self.total_chars) {
            self.map[i] = id;
        }
    }

    fn pos_to_index(&self, pos: Position) -> usize {
        if pos.line >= self.line_starts.len() {
            return self.total_chars;
        }
        self.line_starts[pos.line] + pos.column
    }
}

impl<'a> Visitor for NodeMapVisitor<'a> {
    fn visit_session(&mut self, node: &Session) {
        self.fill_range(node.range());
    }

    fn visit_definition(&mut self, node: &Definition) {
        self.fill_range(node.range());
    }

    fn visit_list(&mut self, node: &List) {
        self.fill_range(node.range());
    }

    fn visit_list_item(&mut self, node: &ListItem) {
        self.fill_range(node.range());
    }

    fn visit_paragraph(&mut self, node: &Paragraph) {
        self.fill_range(node.range());
    }

    fn visit_text_line(&mut self, node: &TextLine) {
        self.fill_range(node.range());
    }

    fn visit_verbatim_block(&mut self, node: &Verbatim) {
        self.fill_range(node.range());
    }

    fn visit_verbatim_group(&mut self, _node: &VerbatimGroupItemRef) {
        // No range on group wrapper
    }

    fn visit_verbatim_line(&mut self, node: &VerbatimLine) {
        self.fill_range(node.range());
    }

    fn visit_annotation(&mut self, node: &Annotation) {
        self.fill_range(node.range());
    }

    fn visit_blank_line_group(&mut self, node: &BlankLineGroup) {
        self.fill_range(node.range());
    }
}

fn get_base2048_char(id: usize) -> char {
    if id == 0 {
        return ' '; // No node
    }
    let offset = (id - 1) % 2048;
    char::from_u32(0x2200 + offset as u32).unwrap_or('?')
}

fn get_color_for_id(id: usize) -> (u8, u8, u8) {
    if id == 0 {
        return (128, 128, 128);
    }
    let golden_angle = 137.508;
    let hue = (id as f64 * golden_angle) % 360.0;
    let saturation = 0.7 + 0.2 * ((id % 2) as f64);
    let lightness = 0.5 + 0.15 * (((id % 3) as i32 - 1) as f64);
    hsl_to_rgb(hue, saturation, lightness)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r_prime, g_prime, b_prime) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((r_prime + m) * 255.0) as u8,
        ((g_prime + m) * 255.0) as u8,
        ((b_prime + m) * 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::ast::elements::paragraph::Paragraph;
    use lex_core::lex::ast::elements::session::Session;
    use lex_core::lex::ast::range::{Position, Range};
    use lex_core::lex::ast::ContentItem;
    use lex_core::lex::ast::Document;
    use std::collections::HashMap;

    fn create_simple_doc() -> (Document, String) {
        let source = "# Title\n\nPara 1\n";

        let session = Session::with_title("Title".to_string()).at(Range::new(
            0..8,
            Position::new(0, 0),
            Position::new(0, 8),
        ));

        let para = Paragraph::from_line("Para 1".to_string()).at(Range::new(
            9..16,
            Position::new(2, 0),
            Position::new(2, 7),
        ));

        let doc = Document::with_content(vec![
            ContentItem::Session(session),
            ContentItem::Paragraph(para),
        ]);

        (doc, source.to_string())
    }

    #[test]
    fn test_nodemap_generation_base2048() {
        let (doc, source) = create_simple_doc();
        let params = HashMap::new();

        let output = to_nodemap_str_with_params(&doc, &source, &params);

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3);

        let _root_char = get_base2048_char(1); // Root Session
        let session_char = get_base2048_char(2); // Inner Session
        let _para_char = get_base2048_char(3); // Paragraph
        let text_line_char = get_base2048_char(4); // TextLine (inside Paragraph)

        // Line 1: 7 chars (stripped newline)
        // Should be Inner Session (ID 2)
        assert_eq!(lines[0].chars().count(), 7);
        assert!(lines[0].chars().all(|c| c == session_char));

        // Line 2: Empty
        assert_eq!(lines[1], "");

        // Line 3: "Para 1" (6 chars)
        // Should be TextLine (ID 4) because Paragraph::at updates child TextLine location
        // and child overwrites parent in the map.
        assert_eq!(lines[2].chars().count(), 6);
        assert!(lines[2].chars().all(|c| c == text_line_char));

        assert_ne!(session_char, text_line_char);
    }

    #[test]
    fn test_nodemap_color() {
        let (doc, source) = create_simple_doc();
        let mut params = HashMap::new();
        params.insert("color".to_string(), "true".to_string());

        let output = to_nodemap_str_with_params(&doc, &source, &params);

        assert!(output.contains("\x1b["));
        assert!(output.contains("█"));
    }

    #[test]
    fn test_nodemap_color_char() {
        let (doc, source) = create_simple_doc();
        let mut params = HashMap::new();
        params.insert("color-char".to_string(), "true".to_string());

        let output = to_nodemap_str_with_params(&doc, &source, &params);

        // Should contain ANSI codes
        assert!(output.contains("\x1b["));
        // Should NOT contain block char █ if characters are different (most likely they are)
        // But Base2048 char might randomly be a block char? Unlikely.
        // Check that it contains the mapped char for session.
        let session_char = get_base2048_char(2);
        assert!(output.contains(session_char));
    }

    #[test]
    fn test_nodemap_summary() {
        let (doc, source) = create_simple_doc();
        let mut params = HashMap::new();
        params.insert("nodesummary".to_string(), "true".to_string());

        let output = to_nodemap_str_with_params(&doc, &source, &params);

        assert!(output.contains("Ast Nodes ="));
        assert!(output.contains("Median Node Size ="));
        assert!(output.contains("1 char ast node ="));
    }
}
