//! `lexd element-at` / `lexd token-at` — query the tree at a position.

use lex_analysis::semantic_tokens::collect_semantic_tokens;
use lex_babel::FormatRegistry;
use lex_core::lex::ast::{find_node_path_at_position, Position};
use std::fs;

/// Handle the element-at command
pub(crate) fn handle_element_at_command(path: &str, row: usize, col: usize, all: bool) {
    let source = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error reading file '{path}': {e}");
        std::process::exit(1);
    });

    let registry = FormatRegistry::default();
    let doc = registry.parse(&source, "lex").unwrap_or_else(|e| {
        eprintln!("Parse error: {e}");
        std::process::exit(1);
    });

    // Convert 1-based to 0-based
    let pos = Position::new(row.saturating_sub(1), col.saturating_sub(1));

    let path_nodes = find_node_path_at_position(&doc, pos);

    if path_nodes.is_empty() {
        eprintln!("No element found at {row}:{col}");
        return;
    }

    if all {
        for node in path_nodes {
            println!("{}: {}", node.node_type(), node.display_label());
        }
    } else if let Some(node) = path_nodes.last() {
        println!("{}: {}", node.node_type(), node.display_label());
    }
}

/// Handle the token-at command
pub(crate) fn handle_token_at_command(path: &str, row: usize, col: usize) {
    let source = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error reading file '{path}': {e}");
        std::process::exit(1);
    });

    let registry = FormatRegistry::default();
    let doc = registry.parse(&source, "lex").unwrap_or_else(|e| {
        eprintln!("Parse error: {e}");
        std::process::exit(1);
    });

    // Convert 1-based row/col to 0-based
    let target_line = row.saturating_sub(1);
    let target_col = col.saturating_sub(1);
    let tokens = collect_semantic_tokens(&doc);
    let lines: Vec<&str> = source.lines().collect();

    let matching: Vec<_> = tokens
        .iter()
        .filter(|t| {
            let s = &t.range.start;
            let e = &t.range.end;
            if s.line == e.line {
                // Single-line token
                s.line == target_line && target_col >= s.column && target_col < e.column
            } else {
                // Multi-line token
                if target_line == s.line {
                    target_col >= s.column
                } else if target_line == e.line {
                    target_col < e.column
                } else {
                    target_line > s.line && target_line < e.line
                }
            }
        })
        .collect();

    if matching.is_empty() {
        println!("No semantic token at {row}:{col}");
    } else {
        for token in &matching {
            let start = &token.range.start;
            let end = &token.range.end;
            let excerpt = if start.line == end.line {
                lines
                    .get(start.line)
                    .map(|l| {
                        let s = start.column.min(l.len());
                        let e = end.column.min(l.len());
                        &l[s..e]
                    })
                    .unwrap_or("")
            } else {
                lines
                    .get(start.line)
                    .map(|l| {
                        let s = start.column.min(l.len());
                        &l[s..]
                    })
                    .unwrap_or("")
            };
            println!(
                "{}:{}-{}:{}  {}  \"{}\"",
                start.line + 1,
                start.column + 1,
                end.line + 1,
                end.column + 1,
                token.kind.as_str(),
                excerpt,
            );
        }
    }
}
