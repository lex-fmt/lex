//! Core token types and helpers shared across the lexer, parser, and tooling.
//!
//!     This module provides the token types used throughout the lexing and parsing pipeline.
//!     Lex opts for handling more complexity in the lexing stage in order to keep the parsing
//!     stage very simple. This implies in greater token complexity, and this is the origin of
//!     several token types.
//!
//! Token Layers
//!
//!     Even though the grammar operates mostly over lines, we have multiple layers of tokens:
//!
//!     Structural Tokens:
//!         Indent, Dedent. These are semantic tokens that represent indentation level changes,
//!         similar to open/close braces in more c-style languages. They are produced by the
//!         semantic indentation transformation from raw Indentation tokens. See
//!         [semantic_indentation](crate::lex::lexing::transformations::semantic_indentation).
//!
//!     Core Tokens:
//!         Character/word level tokens. They are produced by the logos lexer. See [core](core) module
//!         for the complete list of core tokens. Grammar: [specs/v1/grammar-core.lex].
//!
//!     Line Tokens:
//!         A group of core tokens in a single line, and used in the actual parsing. See
//!         [line](line) module. The LineType enum is the definitive set of line classifications
//!         (blank, annotation start/end, data, subject, list, subject-or-list-item, paragraph,
//!         dialog, indent, dedent). Grammar: [specs/v1/grammar-line.lex].
//!
//!     Inline Tokens:
//!         Span-based tokens that operate at the character level within text content. Unlike
//!         line-based tokens, inline tokens can start and end at arbitrary positions and can be
//!         nested within each other. See [inline](inline) module. Grammar: [specs/v1/grammar-inline.lex].
//!
//!     Line Container Tokens:
//!         A vector of line tokens or other line container tokens. This is a tree representation
//!         of each level's lines. This is created and used by the parser. See [to_line_container]
//!         module.
//!
//!     Synthetic Tokens:
//!         Tokens that are not produced by the logos lexer, but are created by the lexing pipeline
//!         to capture context information from parent to children elements so that parsing can be
//!         done in a regular single pass.
//!
//!         Context Injection: Synthetic tokens enable single-pass parsing by injecting parent context
//!         into child scopes. This avoids making the grammar context-sensitive and eliminates the need
//!         for tree walking during parsing.
//!
//!         Example - Session Preceding Blank Lines: Sessions require preceding blank lines, but for a
//!         session that is the first element in its parent, that preceding blank line belongs to the
//!         parent session's scope. A synthetic BlankLine token is injected at the start of the child
//!         scope to represent this parent context, allowing the parser to check for the required
//!         preceding blank line without looking upward in the tree.
//!
//!         Properties: Synthetic tokens are not consumed during parsing and do not become AST nodes.
//!         They exist solely to inform parsing decisions. Since they have no source text, they carry
//!         no byte range information.

pub mod core;
pub mod formatting;
pub mod inline;
pub mod line;
pub mod normalization;
pub mod testing;
pub mod to_line_container;

pub use core::Token;
pub use formatting::{detokenize, ToLexString};
pub use inline::InlineKind;
pub use line::{LineContainer, LineToken, LineType};
pub use normalization::utilities;
