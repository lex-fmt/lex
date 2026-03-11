//! Fluent assertion API for AST nodes
//!
//!     This module provides a powerful fluent API for testing AST nodes. As mentioned, the
//!     spec is in flux. This means that the lower level AST nodes are subject to change. If
//!     a test walks through the node directly, on spec changes, it will break.
//!
//!     Additionally, low level ast tests tend to be very superficial, doing things like
//!     element counts (which is bound to be wrong) and other minor checks.
//!
//!     For this reason, all AST testing is done by this powerful library. It will conveniently
//!     let you verify your choice of information from any element, including children and
//!     other nested nodes. Not only is it much faster and easier to write, but on spec changes,
//!     only one change might be needed.
//!
//! Why Manual AST Walking Tests are Insufficient
//!
//!     Traditional tests that manually walk the AST have several problems:
//!
//!         - They are verbose and hard to read. A test checking a nested session might require
//!           20+ lines of boilerplate code.
//!         - They are fragile. When AST node structures change, tests break in many places.
//!         - They tend to be superficial, only checking counts or shallow properties rather
//!           than actual content and structure.
//!         - They don't scale well. Testing deep hierarchies becomes exponentially more
//!           complex.
//!
//! How the Fluent API Helps with Spec Changes
//!
//!     The fluent API abstracts over the actual AST structure. Instead of directly accessing
//!     fields like `session.title` or `session.children`, you use semantic methods like
//!     `.label()` and `.child_count()`. When the AST structure changes, only the assertion
//!     implementation needs to change, not every test.
//!
//!     Example: If the AST changes from `session.title: String` to `session.title: TextContent`,
//!     tests using `.label()` continue to work, while tests using `session.title` directly
//!     break everywhere.
//!
//! Usage Example
//!
//!     ```rust,ignore
//!     use crate::lex::testing::{assert_ast, lexplore::Lexplore};
//!
//!     #[test]
//!     fn test_complex_document() {
//!         let doc = Lexplore::benchmark(10).parse().unwrap();
//!
//!         assert_ast(&doc)
//!             .item(0, |item| {
//!                 item.assert_session()
//!                     .label("Introduction")
//!                     .child_count(3)
//!                     .child(0, |child| {
//!                         child.assert_paragraph()
//!                             .text_starts_with("Welcome")
//!                             .text_contains("lex format")
//!                     })
//!                     .child(1, |child| {
//!                         child.assert_list()
//!                             .item_count(2)
//!                             .item(0, |item| {
//!                                 item.text_starts_with("First")
//!                             })
//!                             .item(1, |item| {
//!                                 item.text_starts_with("Second")
//!                                 .child_count(1)
//!                                 .child(0, |nested| {
//!                                     nested.assert_paragraph()
//!                                         .text_contains("nested")
//!                                 })
//!                             })
//!                     })
//!                     .child(2, |child| {
//!                         child.assert_definition()
//!                             .subject("Term")
//!                             .child_count(1)
//!                     })
//!             });
//!     }
//!     ```
//!
//!     This single test verifies the entire document structure concisely. If the spec changes,
//!     only the assertion library implementation needs updating, not hundreds of tests.

mod assertions;

pub use assertions::{
    AnnotationAssertion, ChildrenAssertion, DefinitionAssertion, DocumentAssertion,
    InlineAssertion, InlineExpectation, ListAssertion, ListItemAssertion, ParagraphAssertion,
    ReferenceExpectation, SessionAssertion, VerbatimBlockkAssertion,
};

use crate::lex::ast::traits::AstNode;
use crate::lex::ast::{ContentItem, Document};

// ============================================================================
// Entry Point
// ============================================================================

/// Create an assertion builder for a document
pub fn assert_ast(doc: &Document) -> DocumentAssertion<'_> {
    DocumentAssertion { doc }
}

// ============================================================================
// ContentItem Assertions
// ============================================================================

pub struct ContentItemAssertion<'a> {
    pub(crate) item: &'a ContentItem,
    pub(crate) context: String,
}

impl<'a> ContentItemAssertion<'a> {
    /// Assert this item is a Paragraph and return paragraph-specific assertions
    pub fn assert_paragraph(self) -> ParagraphAssertion<'a> {
        match self.item {
            ContentItem::Paragraph(p) => ParagraphAssertion {
                para: p,
                context: self.context,
            },
            _ => panic!(
                "{}: Expected Paragraph, found {}",
                self.context,
                self.item.node_type()
            ),
        }
    }

    /// Assert this item is a Session and return session-specific assertions
    pub fn assert_session(self) -> SessionAssertion<'a> {
        match self.item {
            ContentItem::Session(s) => SessionAssertion {
                session: s,
                context: self.context,
            },
            _ => panic!(
                "{}: Expected Session, found {}",
                self.context,
                self.item.node_type()
            ),
        }
    }

    /// Assert this item is a List and return list-specific assertions
    pub fn assert_list(self) -> ListAssertion<'a> {
        match self.item {
            ContentItem::List(l) => ListAssertion {
                list: l,
                context: self.context,
            },
            _ => panic!(
                "{}: Expected List, found {}",
                self.context,
                self.item.node_type()
            ),
        }
    }

    /// Assert this item is a Definition and return definition-specific assertions
    pub fn assert_definition(self) -> DefinitionAssertion<'a> {
        match self.item {
            ContentItem::Definition(d) => DefinitionAssertion {
                definition: d,
                context: self.context,
            },
            _ => panic!(
                "{}: Expected Definition, found {}",
                self.context,
                self.item.node_type()
            ),
        }
    }

    /// Assert this item is an Annotation and return annotation-specific assertions
    pub fn assert_annotation(self) -> AnnotationAssertion<'a> {
        match self.item {
            ContentItem::Annotation(a) => AnnotationAssertion {
                annotation: a,
                context: self.context,
            },
            _ => panic!(
                "{}: Expected Annotation, found {}",
                self.context,
                self.item.node_type()
            ),
        }
    }

    /// Assert this item is a VerbatimBlock and return verbatim block-specific assertions
    pub fn assert_verbatim_block(self) -> VerbatimBlockkAssertion<'a> {
        match self.item {
            ContentItem::VerbatimBlock(fb) => VerbatimBlockkAssertion {
                verbatim_block: fb,
                context: self.context,
            },
            _ => panic!(
                "{}: Expected VerbatimBlock, found {}",
                self.context,
                self.item.node_type()
            ),
        }
    }
}

// ============================================================================
// Tests for Document-level Assertions (location tests)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::range::{Position, Range};
    use crate::lex::ast::{Document, Session};

    #[test]
    fn test_root_location_starts_at() {
        let location = Range::new(0..0, Position::new(0, 0), Position::new(0, 10));
        let mut session = Session::with_title(String::new());
        session.location = location;
        let doc = Document {
            annotations: Vec::new(),
            root: session,
        };

        assert_ast(&doc).root_location_starts_at(0, 0);
    }

    #[test]
    #[should_panic(expected = "Expected root session location start line 5, found 0")]
    fn test_root_location_starts_at_fails_wrong_line() {
        let location = Range::new(0..0, Position::new(0, 0), Position::new(0, 10));
        let mut session = Session::with_title(String::new());
        session.location = location;
        let doc = Document {
            annotations: Vec::new(),
            root: session,
        };

        assert_ast(&doc).root_location_starts_at(5, 0);
    }

    #[test]
    fn test_root_location_ends_at() {
        let location = Range::new(0..0, Position::new(0, 0), Position::new(2, 15));
        let mut session = Session::with_title(String::new());
        session.location = location;
        let doc = Document {
            annotations: Vec::new(),
            root: session,
        };

        assert_ast(&doc).root_location_ends_at(2, 15);
    }

    #[test]
    #[should_panic(expected = "Expected root session location end column 10, found 15")]
    fn test_root_location_ends_at_fails_wrong_column() {
        let location = Range::new(0..0, Position::new(0, 0), Position::new(2, 15));
        let mut session = Session::with_title(String::new());
        session.location = location;
        let doc = Document {
            annotations: Vec::new(),
            root: session,
        };

        assert_ast(&doc).root_location_ends_at(2, 10);
    }

    #[test]
    fn test_root_location_contains() {
        let location = Range::new(0..0, Position::new(1, 0), Position::new(3, 10));
        let mut session = Session::with_title(String::new());
        session.location = location;
        let doc = Document {
            annotations: Vec::new(),
            root: session,
        };

        assert_ast(&doc).root_location_contains(2, 5);
    }

    #[test]
    #[should_panic(expected = "Expected root session location")]
    fn test_root_location_contains_fails() {
        let location = Range::new(0..0, Position::new(1, 0), Position::new(3, 10));
        let mut session = Session::with_title(String::new());
        session.location = location;
        let doc = Document {
            annotations: Vec::new(),
            root: session,
        };

        assert_ast(&doc).root_location_contains(5, 5);
    }

    #[test]
    fn test_root_location_excludes() {
        let location = Range::new(0..0, Position::new(1, 0), Position::new(3, 10));
        let mut session = Session::with_title(String::new());
        session.location = location;
        let doc = Document {
            annotations: Vec::new(),
            root: session,
        };

        assert_ast(&doc).root_location_excludes(5, 5);
    }

    #[test]
    #[should_panic(expected = "Expected root session location")]
    fn test_root_location_excludes_fails() {
        let location = Range::new(0..0, Position::new(1, 0), Position::new(3, 10));
        let mut session = Session::with_title(String::new());
        session.location = location;
        let doc = Document {
            annotations: Vec::new(),
            root: session,
        };

        assert_ast(&doc).root_location_excludes(2, 5);
    }

    #[test]
    fn test_location_assertions_are_fluent() {
        let location = Range::new(0..0, Position::new(0, 0), Position::new(5, 20));
        let mut session = Session::with_title(String::new());
        session.location = location;
        let doc = Document {
            annotations: Vec::new(),
            root: session,
        };

        assert_ast(&doc)
            .root_location_starts_at(0, 0)
            .root_location_ends_at(5, 20)
            .root_location_contains(2, 10)
            .root_location_excludes(10, 0)
            .item_count(0);
    }
}
