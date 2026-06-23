//! Semantic-token transforms.
//!
//! Backs the `semantic-tokens` and `semantic-tokens-json` transforms —
//! one-line-per-token text and JSON renderings of the semantic tokens
//! collected by lex-analysis.

use lex_analysis::semantic_tokens::collect_semantic_tokens;

/// Format semantic tokens as one line per token:
///   startLine:startCol-endLine:endCol  TokenKind  "text excerpt"
pub(super) fn semantic_tokens_to_simple(
    doc: &lex_core::lex::parsing::Document,
    source: &str,
) -> String {
    let tokens = collect_semantic_tokens(doc);
    let lines: Vec<&str> = source.lines().collect();
    let mut output = String::new();

    for token in &tokens {
        let start = &token.range.start;
        let end = &token.range.end;

        // Extract text excerpt from source (single-line only for readability)
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

        // 1-based line and column numbers for display
        output.push_str(&format!(
            "{}:{}-{}:{}  {}  \"{}\"\n",
            start.line + 1,
            start.column + 1,
            end.line + 1,
            end.column + 1,
            token.kind.as_str(),
            excerpt.chars().take(60).collect::<String>(),
        ));
    }

    output
}

/// Format semantic tokens as JSON array
pub(super) fn semantic_tokens_to_json(
    doc: &lex_core::lex::parsing::Document,
    source: &str,
) -> serde_json::Value {
    use serde_json::json;

    let tokens = collect_semantic_tokens(doc);
    let lines: Vec<&str> = source.lines().collect();

    json!(tokens
        .iter()
        .map(|token| {
            let start = &token.range.start;
            let end = &token.range.end;
            let excerpt = if start.line == end.line {
                lines
                    .get(start.line)
                    .map(|l| {
                        let s = start.column.min(l.len());
                        let e = end.column.min(l.len());
                        l[s..e].to_string()
                    })
                    .unwrap_or_default()
            } else {
                lines
                    .get(start.line)
                    .map(|l| {
                        let s = start.column.min(l.len());
                        l[s..].to_string()
                    })
                    .unwrap_or_default()
            };
            json!({
                "kind": token.kind.as_str(),
                "start_line": start.line + 1,
                "start_col": start.column + 1,
                "end_line": end.line + 1,
                "end_col": end.column + 1,
                "text": excerpt,
            })
        })
        .collect::<Vec<_>>())
}
