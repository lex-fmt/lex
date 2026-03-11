//! Document-level assertions

use super::{annotation::AnnotationAssertion, summarize_items, visible_len, visible_nth};
use crate::lex::ast::Document;
use crate::lex::testing::ast_assertions::ContentItemAssertion;
use crate::lex::testing::TextMatch;

pub struct DocumentAssertion<'a> {
    pub(crate) doc: &'a Document,
}

impl<'a> DocumentAssertion<'a> {
    // ===== Title assertions =====

    /// Assert the document title matches exactly
    pub fn title(self, expected: &str) -> Self {
        let actual = self.doc.title();
        assert_eq!(
            actual, expected,
            "Expected document title \"{expected}\", found \"{actual}\""
        );
        self
    }

    /// Assert the document title matches a pattern
    pub fn title_matches(self, matcher: TextMatch) -> Self {
        let actual = self.doc.title();
        assert!(
            matcher.matches(actual),
            "Document title \"{actual}\" does not match pattern {matcher:?}"
        );
        self
    }

    /// Assert the document has no title (empty string)
    pub fn title_is_empty(self) -> Self {
        let actual = self.doc.title();
        assert!(
            actual.is_empty(),
            "Expected empty document title, found \"{actual}\""
        );
        self
    }

    /// Assert the document has a non-empty title
    pub fn has_title(self) -> Self {
        let actual = self.doc.title();
        assert!(
            !actual.is_empty(),
            "Expected document to have a title, but title is empty"
        );
        self
    }

    // ===== Item assertions =====

    /// Assert the number of items in the document
    pub fn item_count(self, expected: usize) -> Self {
        let actual = visible_len(&self.doc.root.children);
        assert_eq!(
            actual,
            expected,
            "Expected {} items, found {} items: [{}]",
            expected,
            actual,
            summarize_items(&self.doc.root.children)
        );
        self
    }

    /// Assert on a specific item by index
    pub fn item<F>(self, index: usize, assertion: F) -> Self
    where
        F: FnOnce(ContentItemAssertion<'a>),
    {
        let visible_children = visible_len(&self.doc.root.children);
        assert!(
            index < visible_children,
            "Item index {index} out of bounds (document has {visible_children} items)"
        );

        let item = visible_nth(&self.doc.root.children, index)
            .expect("visible child should exist at computed index");
        assertion(ContentItemAssertion {
            item,
            context: format!("items[{index}]"),
        });
        self
    }

    /// Assert the root session location starts at the given line and column
    pub fn root_location_starts_at(self, expected_line: usize, expected_column: usize) -> Self {
        let actual = self.doc.root.location.clone();
        assert_eq!(
            actual.clone().start.line,
            expected_line,
            "Expected root session location start line {}, found {}",
            expected_line,
            actual.clone().start.line
        );
        assert_eq!(
            actual.clone().start.column,
            expected_column,
            "Expected root session location start column {}, found {}",
            expected_column,
            actual.clone().start.column
        );
        self
    }

    /// Assert the root session location ends at the given line and column
    pub fn root_location_ends_at(self, expected_line: usize, expected_column: usize) -> Self {
        let actual = self.doc.root.location.clone();
        assert_eq!(
            actual.end.line, expected_line,
            "Expected root session location end line {}, found {}",
            expected_line, actual.end.line
        );
        assert_eq!(
            actual.end.column, expected_column,
            "Expected root session location end column {}, found {}",
            expected_column, actual.end.column
        );
        self
    }

    /// Assert the root session location contains the given position
    pub fn root_location_contains(self, line: usize, column: usize) -> Self {
        use crate::lex::ast::range::Position;

        let pos = Position::new(line, column);
        let location = self.doc.root.location.clone();
        assert!(
            location.contains(pos),
            "Expected root session location {location} to contain position {line}:{column}"
        );
        self
    }

    /// Assert the root session location does NOT contain the given position
    pub fn root_location_excludes(self, line: usize, column: usize) -> Self {
        use crate::lex::ast::range::Position;

        let pos = Position::new(line, column);
        let location = self.doc.root.location.clone();
        assert!(
            !location.contains(pos),
            "Expected root session location {location} to NOT contain position {line}:{column}"
        );
        self
    }

    pub fn annotation_count(self, expected: usize) -> Self {
        let actual = self.doc.annotations.len();
        assert_eq!(
            actual, expected,
            "Expected {expected} document annotations, found {actual}"
        );
        self
    }

    pub fn annotation<F>(self, index: usize, assertion: F) -> Self
    where
        F: FnOnce(AnnotationAssertion<'a>),
    {
        assert!(
            index < self.doc.annotations.len(),
            "Annotation index {} out of bounds (document has {} annotations)",
            index,
            self.doc.annotations.len()
        );
        let annotation = &self.doc.annotations[index];
        assertion(AnnotationAssertion {
            annotation,
            context: format!("document:annotations[{index}]"),
        });
        self
    }
}
