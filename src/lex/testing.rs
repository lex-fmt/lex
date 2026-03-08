//! Testing utilities for AST assertions
//!
//!     This module provides comprehensive testing tools and guidelines for the lex parser.
//!     Testing the parser must follow strict rules to ensure reliability and maintainability.
//!
//! Why Testing is Different
//!
//!     Lex is a novel format, for which there is no established body of source text nor a
//!     reference parser to compare against. Adding insult to injury, the format is still
//!     evolving, so specs change, and in some ways it looks like markdown just enough to
//!     create confusion.
//!
//!     The corollary here being that getting correct Lex source text is not trivial, and if
//!     you make one up, the odds of it being slightly off are high. If one tests the parser
//!     against an illegal source string, all goes to waste: we will have a parser tuned to
//!     the wrong thing. Worst of all, as each test might produce its slight variation, we
//!     will have an unpredictable, complex and wrong parser. If that was not enough, come a
//!     change in the spec, and now we must hunt down and review hundreds of ad-hoc strings
//!     in test files.
//!
//!     This is why all testing must follow two strict rules:
//!
//!         1. Always use verified sample files from the spec (via [Lexplore](lexplore))
//!         2. Always use comprehensive AST assertions (via [assert_ast](fn@assert_ast))
//!
//! Rule 1: Always Use Lexplore for Test Content
//!
//!     Why this matters:
//!
//!         lex is a novel format that's still evolving. People regularly get small details
//!         wrong, leading to false positives in tests. When lex changes, we need to verify
//!         and update all source files. If lex content is scattered across many test files,
//!         this becomes a maintenance nightmare.
//!
//!     The solution:
//!
//!         Use the `Lexplore` library to access verified, curated lex sample files. This
//!         ensures only vetted sources are used and makes writing tests much easier.
//!
//!     Examples:
//!
//!     ```rust,ignore
//!     use crate::lex::testing::lexplore::Lexplore;
//!     use crate::lex::parsing::parse_document;
//!
//!     // CORRECT: Use verified sample files
//!     let doc = Lexplore::paragraph(1).parse().unwrap();
//!     let paragraph = doc.root.expect_paragraph();
//!
//!     // OR load source and parse separately
//!     let source = Lexplore::paragraph(1).source();
//!     let doc = parse_document(&source).unwrap();
//!
//!     // OR use tokenization
//!     let tokens = Lexplore::list(1).tokenize().unwrap();
//!
//!     // OR load documents (benchmark, trifecta)
//!     let doc = Lexplore::benchmark(10).parse().unwrap();
//!     let doc = Lexplore::trifecta(0).parse().unwrap();
//!
//!     // OR get the AST node directly
//!     let paragraph = Lexplore::get_paragraph(1);
//!     let list = Lexplore::get_list(1);
//!     let session = Lexplore::get_session(1);
//!
//!     // WRONG: Don't write lex content directly in tests
//!     let doc = parse_document("Some paragraph\n\nAnother paragraph\n\n").unwrap();
//!     ```
//!
//!     Available sources:
//!
//!         - Elements: `Lexplore::paragraph(1)`, `Lexplore::list(1)`, etc. - Individual elements
//!         - Documents: `Lexplore::benchmark(0)`, `Lexplore::trifecta(0)` - Full documents
//!         - Direct access: `Lexplore::get_paragraph(1)` - Returns the AST node directly
//!
//!     The sample files are organized:
//!
//!         - By elements:
//!             - Isolated elements (only the element itself): Individual test cases
//!             - In Document: mixed with other elements: Integration test cases
//!         - Benchmark: full documents that are used to test the parser
//!         - Trifecta: a mix of sessions, paragraphs and lists, the structural elements
//!
//!     See the [Lexplore documentation](lexplore) for complete API details.
//!
//! Rule 2: Always Use assert_ast for AST Verification
//!
//! Why this matters:
//!
//! What we want for every document test is to ensure that the AST shape is correct
//! per the grammar, that all attributes are correct (children, content, etc.).
//! Asserting generalities like node counts is useless - it's not informative.
//! We want assurance on the AST shape and content.
//!
//! This is also very hard to write, time-consuming, and when the lex spec changes,
//! very hard to update.
//!
//! The solution:
//!
//! Use the `assert_ast` library with its fluent API. It allows testing entire
//! hierarchies of nodes at once with 10-20x less code.
//!
//! ### The Problem with Manual Testing
//!
//! Testing a nested session traditionally looks like this:
//!
//! ```rust-example
//! use crate::lex::ast::ContentItem;
//!
//! match &doc.content[0] {
//!     ContentItem::Session(s) => {
//!         assert_eq!(s.title, "Introduction");
//!         assert_eq!(s.children.len(), 2);
//!         match &s.content[0] {
//!             ContentItem::Paragraph(p) => {
//!                 assert_eq!(p.lines.len(), 1);
//!                 assert!(p.lines[0].starts_with("Hello"));
//!             }
//!             _ => panic!("Expected paragraph"),
//!         }
//!         // ... repeat for second child
//!     }
//!     _ => panic!("Expected session"),
//! }
//! ```
//!
//! 20+ lines of boilerplate. Hard to see what's actually being tested.

//! ### The Solution: Fluent Assertion API

//! With the `assert_ast` fluent API, the same test becomes:

//! ```rust-example
//! use crate::lex::testing::assert_ast;
//!
//! assert_ast(&doc)
//!     .item(0, |item| {
//!         item.assert_session()
//!             .label("Introduction")
//!             .child_count(2)
//!             .child(0, |child| {
//!                 child.assert_paragraph()
//!                     .text_starts_with("Hello")
//!             })
//!     });
//! ```

//! Concise, readable, and maintainable.

//! ## Available Node Types

//! The assertion API supports all AST node types:
//! - `ParagraphAssertion` - Text content nodes
//! - `SessionAssertion` - Titled container nodes  
//! - `ListAssertion` / `ListItemAssertion` - List structures
//! - `DefinitionAssertion` - Subject-definition pairs
//! - `AnnotationAssertion` - Metadata with parameters
//! - `VerbatimBlockkAssertion` - Raw content blocks

//!   Each assertion type provides type-specific methods (e.g., `label()` for
//!   sessions, `subject()` for definitions, `parameter_count()` for annotations).

//! ## Extending the Assertion API

//! To add support for a new container node type:
//!
//! 1. Implement the traits in `ast.rs`:
//!    ```rust-example
//!    use crate::lex::ast::{Container, ContentItem};
//!
//!    struct NewNode { content: Vec<ContentItem>, label: String }
//!
//!    impl Container for NewNode {
//!        fn label(&self) -> &str { &self.label }
//!        fn children(&self) -> &[ContentItem] { &self.content }
//!        fn children_mut(&mut self) -> &mut Vec<ContentItem> { &mut self.content }
//!    }
//!    ```
//!
//! 2. Add to ContentItem enum and implement helper methods
//!
//! 3. Add assertion type in `testing_assertions.rs`:
//!    ```rust-example
//!    pub struct NewNodeAssertion<'a> { /* ... */ }
//!
//!    impl NewNodeAssertion<'_> {
//!        pub fn custom_field(self, expected: &str) -> Self { /* ... */ }
//!        pub fn child_count(self, expected: usize) -> Self { /* ... */ }
//!    }
//!    ```
//!
//! 4. Add to ContentItemAssertion and export in `testing.rs`:
//!    ```rust-example
//!    pub fn assert_new_node(self) -> NewNodeAssertion<'a> { /* ... */ }
//!    ```

mod ast_assertions;
pub mod lexplore;
mod matchers;
pub mod text_diff;

pub use ast_assertions::{
    assert_ast, AnnotationAssertion, ChildrenAssertion, ContentItemAssertion, DefinitionAssertion,
    DocumentAssertion, InlineAssertion, InlineExpectation, ListAssertion, ListItemAssertion,
    ParagraphAssertion, ReferenceExpectation, SessionAssertion, VerbatimBlockkAssertion,
};
pub use matchers::TextMatch;

// Public submodule path: crate::lex::testing::factories
pub mod factories {
    pub use crate::lex::token::testing::*;
}

/// Get a path relative to the crate root for testing purposes.
///
/// `CARGO_MANIFEST_DIR` points to the crate directory where specs/ lives.
///
/// # Example
/// ```rust,ignore
/// let path = workspace_path("comms/specs/elements/paragraph.docs/paragraph-01-flat-oneline.lex");
/// let content = std::fs::read_to_string(path).unwrap();
/// ```
pub fn workspace_path(relative_path: &str) -> std::path::PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::path::Path::new(manifest_dir).join(relative_path)
}

/// Parse a Lex document without running the annotation attachment stage.
///
/// This is useful for tests that need annotations to remain in the content tree
/// rather than being attached as metadata. Common use cases:
/// - Testing annotation parsing in isolation
/// - Testing the attachment logic itself
/// - Element tests that expect annotations as content items
///
/// # Example
/// ```rust,ignore
/// use crate::lex::testing::parse_without_annotation_attachment;
///
/// let source = ":: note ::\nSome paragraph\n";
/// let doc = parse_without_annotation_attachment(source).unwrap();
///
/// // Annotation is still in content tree, not attached as metadata
/// assert!(doc.root.children.iter().any(|item| matches!(item, ContentItem::Annotation(_))));
/// ```
pub fn parse_without_annotation_attachment(
    source: &str,
) -> Result<crate::lex::ast::Document, String> {
    use crate::lex::assembling::AttachRoot;
    use crate::lex::parsing::engine::parse_from_flat_tokens;
    use crate::lex::transforms::stages::ParseInlines;
    use crate::lex::transforms::standard::LEXING;
    use crate::lex::transforms::Runnable;

    let source = if !source.is_empty() && !source.ends_with('\n') {
        format!("{source}\n")
    } else {
        source.to_string()
    };
    let tokens = LEXING.run(source.clone()).map_err(|e| e.to_string())?;
    let root = parse_from_flat_tokens(tokens, &source).map_err(|e| e.to_string())?;
    let root = ParseInlines::new().run(root).map_err(|e| e.to_string())?;
    // Assemble the root session into a Document but skip metadata attachment
    AttachRoot::new().run(root).map_err(|e| e.to_string())
}
