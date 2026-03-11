//! CLI-specific transforms
//!
//! This module defines all the transform combinations available in the CLI.
//! Each transform is a stage + format combination (e.g., "ast-tag", "token-core-json").
//!
//! ## Transform Pipeline
//!
//! The lex compiler has several processing stages:
//!
//! 1. **Tokenization** - Raw text → Token stream
//!    - `token-core-*`: Core tokens (no semantic indentation)
//!    - `token-line-*`: Line tokens (with semantic indentation)
//!
//! 2. **Parsing** - Tokens → Intermediate Representation (IR)
//!    - `ir-json`: Parse tree representation
//!
//! 3. **Assembly** - IR → Abstract Syntax Tree (AST)
//!    - `ast-tag`: XML-like tag format
//!    - `ast-treeviz`: Tree visualization with Unicode icons
//!    - `ast-json`: JSON representation
//!
//! ## Extra Parameters
//!
//! Transforms can accept extra parameters via `--extra-<name> [value]`:
//!
//! - `ast-full`: When set to "true", shows complete AST including:
//!   * Document-level annotations
//!   * All node properties (labels, subjects, parameters, etc.)
//!   * Session titles, list item markers, definition subjects
//!
//! Example: `lex inspect file.lex ast-tag --extra-ast-full`

use lex_babel::formats::{
    linetreeviz::to_linetreeviz_str_with_params, nodemap::to_nodemap_str_with_params,
    tag::serialize_document_with_params as serialize_ast_tag_with_params,
    treeviz::to_treeviz_str_with_params,
};
use lex_core::lex::lexing::transformations::line_token_grouping::GroupedTokens;
use lex_core::lex::lexing::transformations::LineTokenGroupingMapper;
use lex_core::lex::loader::DocumentLoader;
use lex_core::lex::token::{to_line_container, LineContainer, LineToken};
use lex_core::lex::transforms::standard::{CORE_TOKENIZATION, LEXING, TO_IR};
use std::collections::HashMap;

/// All available CLI transforms (stage + format combinations)
pub const AVAILABLE_TRANSFORMS: &[&str] = &[
    "token-core-json",
    "token-core-simple",
    "token-core-pprint",
    "token-simple", // alias for token-core-simple
    "token-pprint", // alias for token-core-pprint
    "token-line-json",
    "token-line-simple",
    "token-line-pprint",
    "ir-json",
    "ast-json",
    "ast-tag",
    "ast-treeviz",
    "ast-linetreeviz",
    "ast-nodemap",
];

/// Execute a named transform on a source file with optional extra parameters
///
/// # Arguments
///
/// * `source` - The source text to transform
/// * `transform_name` - The transform to apply (e.g., "ast-tag", "token-core-json")
/// * `extra_params` - Optional parameters for the transform
///
/// # Extra Parameters
///
/// - `ast-full`: "true" - Show complete AST including all node properties
///
/// # Returns
///
/// The transformed output as a string, or an error message
///
/// # Examples
///
/// ```ignore
/// let source = "# Session\n\nContent";
/// let params = HashMap::new();
///
/// // Get tree visualization (default view)
/// let output = execute_transform(source, "ast-treeviz", &params)?;
///
/// // Get complete AST with all properties
/// let mut full_params = HashMap::new();
/// full_params.insert("ast-full".to_string(), "true".to_string());
/// let output = execute_transform(source, "ast-tag", &full_params)?;
/// ```
pub fn execute_transform(
    source: &str,
    transform_name: &str,
    extra_params: &HashMap<String, String>,
) -> Result<String, String> {
    let loader = DocumentLoader::from_string(source);

    // Default show-linum to true for inspect command if not specified
    let mut params = extra_params.clone();
    if !params.contains_key("show-linum") {
        params.insert("show-linum".to_string(), "true".to_string());
    }

    match transform_name {
        "token-core-json" => {
            let tokens = loader
                .with(&CORE_TOKENIZATION)
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(serde_json::to_string_pretty(&tokens_to_json(&tokens))
                .map_err(|e| format!("JSON serialization failed: {e}"))?)
        }
        "token-core-simple" | "token-simple" => {
            let tokens = loader
                .with(&CORE_TOKENIZATION)
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(tokens_to_simple(&tokens))
        }
        "token-core-pprint" | "token-pprint" => {
            let tokens = loader
                .with(&CORE_TOKENIZATION)
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(tokens_to_pprint(&tokens))
        }
        "token-line-json" => {
            let tokens = loader
                .with(&LEXING)
                .map_err(|e| format!("Transform failed: {e}"))?;
            let mut mapper = LineTokenGroupingMapper::new();
            let grouped = mapper.map(tokens);
            let line_tokens: Vec<LineToken> = grouped
                .into_iter()
                .map(GroupedTokens::into_line_token)
                .collect();
            Ok(
                serde_json::to_string_pretty(&line_tokens_to_json(&line_tokens))
                    .map_err(|e| format!("JSON serialization failed: {e}"))?,
            )
        }
        "token-line-simple" => {
            let tokens = loader
                .with(&LEXING)
                .map_err(|e| format!("Transform failed: {e}"))?;
            let mut mapper = LineTokenGroupingMapper::new();
            let grouped = mapper.map(tokens);
            let line_tokens: Vec<LineToken> = grouped
                .into_iter()
                .map(GroupedTokens::into_line_token)
                .collect();
            Ok(line_tokens_to_simple(&line_tokens))
        }
        "token-line-pprint" => {
            let tokens = loader
                .with(&LEXING)
                .map_err(|e| format!("Transform failed: {e}"))?;
            let mut mapper = LineTokenGroupingMapper::new();
            let grouped = mapper.map(tokens);
            let line_tokens: Vec<LineToken> = grouped
                .into_iter()
                .map(GroupedTokens::into_line_token)
                .collect();
            Ok(line_tokens_to_pprint(&line_tokens))
        }
        "ir-json" => {
            let ir = loader
                .with(&TO_IR)
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(serde_json::to_string_pretty(&ir_to_json(&ir))
                .map_err(|e| format!("JSON serialization failed: {e}"))?)
        }
        "ast-json" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(serde_json::to_string_pretty(&ast_to_json(&doc))
                .map_err(|e| format!("JSON serialization failed: {e}"))?)
        }
        "ast-tag" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(serialize_ast_tag_with_params(&doc, &params))
        }
        "ast-treeviz" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            // Pass extra_params to to_treeviz_str
            // Supports: --extra-ast-full true
            Ok(to_treeviz_str_with_params(&doc, &params))
        }
        "ast-linetreeviz" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            // linetreeviz collapses containers like Paragraph and List
            Ok(to_linetreeviz_str_with_params(&doc, &params))
        }
        "ast-nodemap" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(to_nodemap_str_with_params(&doc, source, &params))
        }
        _ => Err(format!("Unknown transform: {transform_name}")),
    }
}

/// Convert tokens to JSON-serializable format
fn tokens_to_json(
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

fn tokens_to_simple(tokens: &[(lex_core::lex::token::Token, std::ops::Range<usize>)]) -> String {
    tokens
        .iter()
        .map(|(token, _)| token.simple_name())
        .collect::<Vec<_>>()
        .join("\n")
}

fn tokens_to_pprint(tokens: &[(lex_core::lex::token::Token, std::ops::Range<usize>)]) -> String {
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
fn line_tokens_to_json(line_tokens: &[LineToken]) -> serde_json::Value {
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

fn line_tokens_to_simple(line_tokens: &[LineToken]) -> String {
    line_tokens
        .iter()
        .map(|line| line.line_type.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_tokens_to_pprint(line_tokens: &[LineToken]) -> String {
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

/// Convert IR (ParseNode) to JSON-serializable format
fn ir_to_json(node: &lex_core::lex::parsing::ir::ParseNode) -> serde_json::Value {
    use serde_json::json;

    json!({
        "type": format!("{:?}", node.node_type),
        "tokens": tokens_to_json(&node.tokens),
        "children": node.children.iter().map(ir_to_json).collect::<Vec<_>>(),
        "has_payload": node.payload.is_some(),
    })
}

/// Convert AST (Document) to JSON-serializable format
fn ast_to_json(doc: &lex_core::lex::parsing::Document) -> serde_json::Value {
    use serde_json::json;

    json!({
        "type": "Document",
        "children_count": doc.root.children.len(),
        // For now, just a basic representation
        // Can be expanded to include full AST details
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_line_transform_emits_line_tokens() {
        let source = "Session:\n    Content\n";
        let extra_params = HashMap::new();
        let output =
            execute_transform(source, "token-line-json", &extra_params).expect("transform to run");

        assert!(output.contains("\"line_type\""));
        assert!(output.contains("SubjectLine"));
        assert!(output.contains("ParagraphLine"));
    }

    #[test]
    fn token_simple_outputs_names() {
        let source = "Session:\n    Content\n";
        let extra_params = HashMap::new();
        let output =
            execute_transform(source, "token-simple", &extra_params).expect("transform to run");

        assert!(output.contains("TEXT"));
        assert!(output.contains("BLANK_LINE"));
    }

    #[test]
    fn token_line_simple_outputs_names() {
        let source = "Session:\n    Content\n";
        let extra_params = HashMap::new();
        let output = execute_transform(source, "token-line-simple", &extra_params)
            .expect("transform to run");

        assert!(output.contains("SUBJECT_LINE"));
        assert!(output.contains("PARAGRAPH_LINE"));
    }

    #[test]
    fn token_pprint_inserts_blank_line() {
        let source = "Hello\n\nWorld\n";
        let extra_params = HashMap::new();
        let output =
            execute_transform(source, "token-pprint", &extra_params).expect("transform to run");

        assert!(output.contains("BLANK_LINE\n\n"));
    }

    #[test]
    fn token_line_pprint_indents_children() {
        let source = "Session:\n    Content\n";
        let extra_params = HashMap::new();
        let output = execute_transform(source, "token-line-pprint", &extra_params)
            .expect("transform to run");

        assert!(output.contains("SUBJECT_LINE"));
        assert!(output.contains("  PARAGRAPH_LINE"));
    }

    #[test]
    fn execute_transform_accepts_extra_params() {
        let source = "# Test\n";
        let mut extra_params = HashMap::new();
        extra_params.insert("all-nodes".to_string(), "true".to_string());
        extra_params.insert("max-depth".to_string(), "5".to_string());

        // Should not error with unknown params
        let result = execute_transform(source, "ast-treeviz", &extra_params);
        assert!(result.is_ok());
    }

    #[test]
    fn ast_full_param_includes_annotations() {
        use lex_babel::formats::treeviz::to_treeviz_str_with_params;
        use lex_core::lex::ast::elements::annotation::Annotation;
        use lex_core::lex::ast::elements::label::Label;
        use lex_core::lex::ast::elements::paragraph::Paragraph;
        use lex_core::lex::ast::elements::typed_content::ContentElement;
        use lex_core::lex::ast::{ContentItem, Document};

        // Create a document with document-level annotation programmatically
        let annotation = Annotation::new(
            Label::new("test-annotation".to_string()),
            vec![],
            Vec::<ContentElement>::new(),
        );
        let doc = Document::with_annotations_and_content(
            vec![annotation],
            vec![ContentItem::Paragraph(Paragraph::from_line(
                "Regular content".to_string(),
            ))],
        );

        let mut extra_params = HashMap::new();

        // Without ast-full, annotations should be excluded from output
        let output_normal = to_treeviz_str_with_params(&doc, &extra_params);
        assert!(
            !output_normal.contains("test-annotation"),
            "Annotation label should not be visible without ast-full"
        );

        // With ast-full=true, annotations should be included
        extra_params.insert("ast-full".to_string(), "true".to_string());
        let output_full = to_treeviz_str_with_params(&doc, &extra_params);
        // The annotation icon is " (double quote character)
        assert!(
            output_full.contains("\" test-annotation"),
            "With ast-full=true, annotation with icon should appear in output. Output was:\n{output_full}"
        );
        assert!(
            output_full.contains("test-annotation"),
            "Annotation label should be visible with ast-full"
        );
    }
}
