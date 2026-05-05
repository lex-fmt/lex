//! Sync core for Lex language-server features.
//!
//! Both the stdio LSP server (`lexd-lsp`) and the WASM bindings (`lex-wasm`)
//! call into this crate. Anything that's pure logic over the AST and doesn't
//! need an async runtime lives here; tower-lsp/tokio glue stays in `lexd-lsp`.

pub mod available_actions;
pub mod document_links;
pub mod footnotes;
pub mod formatting;
pub mod table_format;
pub mod table_navigation;
