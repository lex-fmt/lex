//! Ready-to-insert snippets for Lex documents.
//!
//! Editors need to insert structured Lex content (images, code blocks, etc.) without
//! reimplementing serialization logic. This module provides snippet generators that:
//!
//! - Produce correctly formatted Lex syntax
//! - Respect the active [`FormattingRules`](crate::formats::lex::formatting_rules::FormattingRules)
//! - Handle path normalization (absolute → relative)
//! - Return cursor position hints for editor UX
//!
//! # Available Snippets
//!
//! - **Asset snippets** ([`asset`]): Insert media references (`doc.image`, `doc.video`, etc.)
//!   with automatic kind detection from file extension.
//!
//! - **Verbatim snippets** ([`verbatim`]): Insert code blocks with optional source file
//!   linking and language inference from extension.
//!
//! # Example
//!
//! ```ignore
//! use lex_babel::templates::asset::{AssetSnippetRequest, build_asset_snippet};
//!
//! let request = AssetSnippetRequest::new(Path::new("./diagram.png"), &rules);
//! let snippet = build_asset_snippet(&request);
//! // snippet.text contains: ":: doc.image src=\"./diagram.png\"\n"
//! ```

mod util;

pub mod asset;
pub mod verbatim;

pub use asset::{build_asset_snippet, AssetKind, AssetSnippet, AssetSnippetRequest};
pub(crate) use util::normalize_path;
pub use verbatim::{build_verbatim_snippet, VerbatimSnippet, VerbatimSnippetRequest};
