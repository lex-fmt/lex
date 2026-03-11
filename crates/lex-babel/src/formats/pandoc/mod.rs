//! Pandoc JSON format implementation
//!
//! Strategy: Bidirectional conversion via Pandoc's JSON AST
//!
//! # Overview
//!
//! Pandoc is a universal document converter that uses a JSON representation of its
//! internal AST. This format enables Lex to integrate with Pandoc's extensive format
//! ecosystem, allowing conversion to/from formats like DOCX, PDF, EPUB, LaTeX, and more.
//!
//! Library
//!
//!     As our goal is to avoid parsing , serializing and shelling out, we wil use the pandoc_ast crate.
//1 .   This create is actively maitained, and focus on filters/ adapters, which is exactly what we need. We will use the MutVisitor trait for the conversion.
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
