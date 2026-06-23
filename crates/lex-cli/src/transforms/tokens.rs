//! Token-stream transforms (core tokens and line tokens).
//!
//! Backs the `token-core-*` and `token-line-*` transforms — JSON, simple
//! (one name per line), and pprint (indented) renderings of both the core
//! token stream and the line-classified token stream.

use lex_core::lex::token::{to_line_container, LineContainer, LineToken};

/// Convert tokens to JSON-serializable format
pub(super) fn tokens_to_json(
    tokens: &[(lex_core::lex::token::Token, std::ops::Range<usize>)],
) -> serde_json::Value {
    use serde_json::json;

    json!(tokens
        .iter()
        .map(|(token, range)| {
            json!({
                "token": format!("{:?}", token),
                "start": range.start,
                "end": range.end,
            })
        })
        .collect::<Vec<_>>())
}

pub(super) fn tokens_to_simple(
    tokens: &[(lex_core::lex::token::Token, std::ops::Range<usize>)],
) -> String {
    tokens
        .iter()
        .map(|(token, _)| token.simple_name())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn tokens_to_pprint(
    tokens: &[(lex_core::lex::token::Token, std::ops::Range<usize>)],
) -> String {
    use lex_core::lex::token::Token;

    let mut output = String::new();
    for (token, _) in tokens {
        output.push_str(token.simple_name());
        output.push('\n');
        if matches!(token, Token::BlankLine(_)) {
            output.push('\n');
        }
    }
    output
}

/// Convert line tokens into a JSON-friendly structure
pub(super) fn line_tokens_to_json(line_tokens: &[LineToken]) -> serde_json::Value {
    use serde_json::json;

    json!(line_tokens
        .iter()
        .map(|line| {
            json!({
                "line_type": format!("{:?}", line.line_type),
                "tokens": line
                    .source_tokens
                    .iter()
                    .zip(line.token_spans.iter())
                    .map(|(token, span)| {
                        json!({
                            "token": format!("{:?}", token),
                            "start": span.start,
                            "end": span.end,
                        })
                    })
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>())
}

pub(super) fn line_tokens_to_simple(line_tokens: &[LineToken]) -> String {
    line_tokens
        .iter()
        .map(|line| line.line_type.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn line_tokens_to_pprint(line_tokens: &[LineToken]) -> String {
    let container = to_line_container::build_line_container(line_tokens.to_vec());
    let mut output = String::new();
    render_line_tree(&container, 0, true, &mut output);
    output
}

fn render_line_tree(node: &LineContainer, depth: usize, is_root: bool, output: &mut String) {
    match node {
        LineContainer::Token(line) => {
            let indent = "  ".repeat(depth);
            output.push_str(&indent);
            output.push_str(&line.line_type.to_string());
            output.push('\n');
        }
        LineContainer::Container { children } => {
            let next_depth = if is_root { depth } else { depth + 1 };
            for child in children {
                render_line_tree(child, next_depth, false, output);
            }
        }
    }
}
