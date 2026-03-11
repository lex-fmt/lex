//! Assembling module
//!
//!     The assembling stage processes parsed AST nodes to attach metadata and perform
//!     post-parsing transformations. Unlike the parsing stage which converts tokens to AST,
//!     assembling stages operate on the AST itself.
//!
//!     The builder returns the root session tree, so assembling first wraps it in a
//!     [`Document`](crate::lex::ast::Document). Annotations, which are metadata, are always
//!     attached to AST nodes so they can be very targeted. Only with the full document in
//!     place can we attach annotations to their correct target nodes.
//!
//!     This is harder than it seems. Keeping Lex ethos of not enforcing structure, this needs
//!     to deal with several ambiguous cases, including some complex logic for calculating
//!     "human understanding" distance between elements.
//!
//! Current stages:
//!
//!     - `attach_root`: Wraps the built session tree in a [`Document`].
//!     - `attach_annotations`: Attaches annotations from content to AST nodes as metadata.
//!       See [attach_annotations](stages::attach_annotations) for details.
//!
//!     Note on Document Title:
//!     The document title is extracted during the AST building phase (in `AstTreeBuilder`),
//!     just before the assembling stages begin. It promotes the first paragraph (if followed
//!     by blank lines) to be the document title.

pub mod stages;

pub use stages::{AttachAnnotations, AttachRoot};
