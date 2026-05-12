// Tower-lsp-specific features that stay in this crate.
pub mod commands;
pub mod extract;

// Re-exported sync features. These live in `lex-lsp-core` so they can be
// shared with `lex-wasm`; we re-export them here so server.rs and the rest
// of this crate keep using `crate::features::...` paths unchanged.
pub use lex_lsp_core::{
    available_actions, document_links, footnotes, formatting, table_format, table_navigation,
};

// Re-export analysis features from lex-analysis (unchanged from before).
pub use lex_analysis::{
    document_symbols, folding_ranges, go_to_definition, hover, references, semantic_tokens,
};
