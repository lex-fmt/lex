//! `lexd inspect` — view internal representations of lex files.

use crate::cli_support::{absolutize_path, build_inspect_params, read_source, IncludeOptions};
use lex_babel::{
    formats::lex::formatting_rules::FormattingRules, transforms::serialize_to_lex_with_rules,
};
use lex_config::LexConfig;
use lex_core::lex::builtins;
use lex_core::lex::includes::ResolveConfig;
use lexd::transforms;
use std::path::PathBuf;

/// Handle the inspect command
pub(crate) fn handle_inspect_command(
    path: Option<&str>,
    transform: &str,
    config: &LexConfig,
    inc: &IncludeOptions,
) {
    let mut source = read_source(path);

    // When includes are enabled and we have a real file path, resolve
    // them and re-serialize the merged tree as lex source. Inspect
    // transforms then see the post-include AST. For stdin input or
    // when --no-includes is set, we leave the source unchanged.
    if inc.enabled {
        if let Some(p) = path {
            source = expand_includes_to_source(&source, p, inc);
        }
    }

    let params = build_inspect_params(config);

    let output = transforms::execute_transform(&source, transform, &params).unwrap_or_else(|e| {
        eprintln!("Execution error: {e}");
        std::process::exit(1);
    });

    print!("{output}");
}

/// Resolve `lex.include` annotations and re-serialize the result as lex
/// source so downstream transforms (which take a string) see the merged
/// content. The two-step "resolve to AST → serialize back to lex" is
/// the simplest way to wire the resolver into a string-in/string-out
/// pipeline without restructuring the transform layer.
///
/// Fast path: if the source contains no `lex.include` literal, return
/// it unchanged. The resolver's round trip would be a parse + serialize
/// no-op semantically, but the lex serializer normalizes whitespace and
/// indentation in ways that surprise downstream tools that assert on
/// exact source layout (notably `inspect ast-nodemap`). Skipping the
/// round trip when there's nothing to resolve preserves byte-identical
/// behavior for documents that don't use the feature.
///
/// Errors are printed to stderr and the process exits non-zero; they
/// surface include problems early instead of letting them propagate as
/// confusing parser/serializer errors downstream.
fn expand_includes_to_source(source: &str, entry_path: &str, inc: &IncludeOptions) -> String {
    if !source.contains("lex.include") {
        return source.to_string();
    }
    // Canonicalize entry_path so the resolver sees an absolute path
    // for cycle-detection identity and so relative includes are
    // resolved from the real on-disk parent (not a CLI-relative one).
    let entry = absolutize_path(&PathBuf::from(entry_path));
    let root = inc.resolved_root(&entry);
    let resolve_config = ResolveConfig {
        root,
        max_depth: inc.max_depth,
        max_total_includes: inc.max_total_includes,
    };
    let doc =
        match builtins::resolve_buffer(source, Some(entry), &resolve_config, inc.max_file_size) {
            Ok(doc) => doc,
            // Preserve the two distinct stderr wordings the hand-rolled path had:
            // registry-setup failures and include-resolution failures read
            // differently so the user can tell them apart.
            Err(builtins::ResolveBufferError::Registry(e)) => {
                eprintln!("Failed to register lex.* built-ins: {e}");
                std::process::exit(1);
            }
            Err(builtins::ResolveBufferError::Resolve(e)) => {
                eprintln!("Include resolution error: {e}");
                std::process::exit(1);
            }
        };

    // Re-serialize with default formatting rules; the goal is just
    // to feed downstream transforms the merged source, not to
    // produce author-grade output.
    let rules = FormattingRules::default();
    serialize_to_lex_with_rules(&doc, rules).unwrap_or_else(|e| {
        eprintln!("Failed to re-serialize merged document: {e}");
        std::process::exit(1);
    })
}
