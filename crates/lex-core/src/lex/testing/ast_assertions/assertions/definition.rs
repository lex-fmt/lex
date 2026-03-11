//! Definition assertions

use super::{
    annotation::AnnotationAssertion, summarize_items, visible_len, visible_nth, ChildrenAssertion,
};
use crate::lex::ast::traits::Container;
use crate::lex::ast::Definition;
use crate::lex::testing::ast_assertions::ContentItemAssertion;
use crate::lex::testing::matchers::TextMatch;

pub struct DefinitionAssertion<'a> {
    pub(crate) definition: &'a Definition,
    pub(crate) context: String,
}

impl<'a> DefinitionAssertion<'a> {
    pub fn subject(self, expected: &str) -> Self {
        TextMatch::Exact(expected.to_string())
            .assert(self.definition.subject.as_string(), &self.context);
        self
    }
    pub fn subject_starts_with(self, prefix: &str) -> Self {
        TextMatch::StartsWith(prefix.to_string())
            .assert(self.definition.subject.as_string(), &self.context);
        self
    }
    pub fn subject_contains(self, substring: &str) -> Self {
        TextMatch::Contains(substring.to_string())
            .assert(self.definition.subject.as_string(), &self.context);
        self
    }
    pub fn child_count(self, expected: usize) -> Self {
        let actual = visible_len(self.definition.children());
        assert_eq!(
            actual,
            expected,
            "{}: Expected {} children, found {} children: [{}]",
            self.context,
            expected,
            actual,
            summarize_items(self.definition.children())
        );
        self
    }
    pub fn child<F>(self, index: usize, assertion: F) -> Self
    where
        F: FnOnce(ContentItemAssertion<'a>),
    {
        let children = self.definition.children();
        let visible_children = visible_len(children);
        assert!(
            index < visible_children,
            "{}: Child index {} out of bounds (definition has {} children)",
            self.context,
            index,
            visible_children
        );
        let child =
            visible_nth(children, index).expect("visible child should exist at computed index");
        assertion(ContentItemAssertion {
            item: child,
            context: format!("{}:children[{}]", self.context, index),
        });
        self
    }
    pub fn children<F>(self, assertion: F) -> Self
    where
        F: FnOnce(ChildrenAssertion<'a>),
    {
        assertion(ChildrenAssertion {
            children: self.definition.children(),
            context: format!("{}:children", self.context),
        });
        self
    }

    pub fn annotation_count(self, expected: usize) -> Self {
        let actual = self.definition.annotations.len();
        assert_eq!(
            actual, expected,
            "{}: Expected {} annotations, found {} annotations",
            self.context, expected, actual
        );
        self
    }

    pub fn annotation<F>(self, index: usize, assertion: F) -> Self
    where
        F: FnOnce(AnnotationAssertion<'a>),
    {
        assert!(
            index < self.definition.annotations.len(),
            "{}: Annotation index {} out of bounds (definition has {} annotations)",
            self.context,
            index,
            self.definition.annotations.len()
        );
        let annotation = &self.definition.annotations[index];
        assertion(AnnotationAssertion {
            annotation,
            context: format!("{}:annotations[{}]", self.context, index),
        });
        self
    }
}
