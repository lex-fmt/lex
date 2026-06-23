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
//! ## Parameters
//!
//! Transforms accept parameters via CLI flags, config files, or env vars:
//!
//! - `--ast-full`: Shows complete AST including:
//!   * Document-level annotations
//!   * All node properties (labels, subjects, parameters, etc.)
//!   * Session titles, list item markers, definition subjects
//!
//! Example: `lex inspect file.lex ast-tag --ast-full`
//!
//! ## Module layout
//!
//! This file is the façade: it owns the public surface (`AVAILABLE_TRANSFORMS`,
//! `execute_transform`) and dispatches to the per-stage rendering submodules:
//!
//! - [`ast_json`] — AST → JSON (`ast-json`)
//! - [`parity`] — AST → parity skeleton (`parity`)
//! - [`tokens`] — core / line token renderings (`token-*`)
//! - [`ir`] — IR → JSON (`ir-json`)
//! - [`semantic`] — semantic-token renderings (`semantic-tokens*`)

mod ast_json;
mod ir;
mod parity;
mod semantic;
mod tokens;

#[cfg(test)]
mod tests;

use lex_babel::formats::{
    linetreeviz::to_linetreeviz_str_with_params, nodemap::to_nodemap_str_with_params,
    tag::serialize_document_with_params as serialize_ast_tag_with_params,
    treeviz::to_treeviz_str_with_params,
};
use lex_core::lex::lexing::transformations::line_token_grouping::GroupedTokens;
use lex_core::lex::lexing::transformations::LineTokenGroupingMapper;
use lex_core::lex::loader::DocumentLoader;
use lex_core::lex::token::LineToken;
use lex_core::lex::transforms::standard::{CORE_TOKENIZATION, LEXING, TO_IR};
use std::collections::HashMap;

use ast_json::ast_to_json;
use ir::ir_to_json;
use parity::ast_to_parity;
use semantic::{semantic_tokens_to_json, semantic_tokens_to_simple};
use tokens::{
    line_tokens_to_json, line_tokens_to_pprint, line_tokens_to_simple, tokens_to_json,
    tokens_to_pprint, tokens_to_simple,
};

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
    "semantic-tokens",
    "semantic-tokens-json",
    "parity",
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
            // Supports: --ast-full
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
        "semantic-tokens" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(semantic_tokens_to_simple(&doc, source))
        }
        "semantic-tokens-json" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(
                serde_json::to_string_pretty(&semantic_tokens_to_json(&doc, source))
                    .map_err(|e| format!("JSON serialization failed: {e}"))?,
            )
        }
        "parity" => {
            let doc = loader
                .parse()
                .map_err(|e| format!("Transform failed: {e}"))?;
            Ok(ast_to_parity(&doc))
        }
        _ => Err(format!("Unknown transform: {transform_name}")),
    }
}
