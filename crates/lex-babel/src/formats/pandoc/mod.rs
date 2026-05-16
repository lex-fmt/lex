//! Pandoc JSON format implementation
//!
//! # Status: Planned, not started
//!
//! There is currently **no** Pandoc implementation in this crate — no
//! struct, no `Format` impl, no `pandoc_ast` dependency in `Cargo.toml`,
//! no registration in `crates/lex-babel/src/registry.rs`. This file
//! exists only as planning material; the element-mapping table below
//! is preserved as design notes, not as documentation of an in-progress
//! implementation.
//!
//! When Pandoc work starts, it will be the primary bridge for DOCX,
//! EPUB, RST, Org, and most other formats not worth a bespoke
//! implementation. See the full interop tiering at
//! `comms/docs/interop-scope.lex` in-repo, or
//! <https://github.com/lex-fmt/comms/blob/main/docs/interop-scope.lex>
//! on the web.
//!
//! # Planned strategy
//!
//! Bidirectional conversion via Pandoc's JSON AST.
//!
//! Pandoc is a universal document converter that uses a JSON representation of its
//! internal AST. This format enables Lex to integrate with Pandoc's extensive format
//! ecosystem, allowing conversion to/from formats like DOCX, EPUB, LaTeX, and more.
//!
//! Library
//!
//!     As our goal is to avoid parsing, serializing, and shelling out, we will use the
//!     pandoc_ast crate. This crate is actively maintained and focuses on filters/adapters,
//!     which is exactly what we need. We will use the MutVisitor trait for the conversion.
//!
//! Data Model
//!
//! Pandoc's AST is similar to Lex but with some key differences:
//!
//! | Lex Element | Pandoc Element | Notes |
//! |-------------|----------------|-------|
//! | Session | Header + Div | Pandoc uses headers for structure, divs for grouping |
//! | Paragraph | Para | Direct mapping |
//! | List | BulletList / OrderedList | Based on list type |
//! | ListItem | List item blocks | Pandoc list items can contain block content |
//! | Definition | DefinitionList | Direct mapping to Pandoc's definition lists |
//! | VerbatimBlock | CodeBlock | With optional language attribute |
//! | VerbatimLine | Code (inline) | Inline code span |
//! | Annotation | Div with attributes | Custom attributes for metadata |
//!
//!
