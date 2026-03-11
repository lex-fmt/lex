//! Session assertions

use super::{
    annotation::AnnotationAssertion, summarize_items, visible_len, visible_nth, ChildrenAssertion,
};
use crate::lex::ast::traits::Container;
use crate::lex::ast::Session;
use crate::lex::testing::ast_assertions::ContentItemAssertion;

pub struct SessionAssertion<'a> {
    pub(crate) session: &'a Session,
    pub(crate) context: String,
}

impl<'a> SessionAssertion<'a> {
    pub fn label(self, expected: &str) -> Self {
        let actual = self.session.label();
        assert_eq!(
            actual, expected,
            "{}: Expected session label to be '{}', but got '{}'",
            self.context, expected, actual
        );
        self
    }
    pub fn label_starts_with(self, prefix: &str) -> Self {
        let actual = self.session.label();
        assert!(
            actual.starts_with(prefix),
            "{}: Expected session label to start with '{}', but got '{}'",
            self.context,
            prefix,
            actual
        );
        self
    }
    pub fn label_contains(self, substring: &str) -> Self {
        let actual = self.session.label();
        assert!(
            actual.contains(substring),
            "{}: Expected session label to contain '{}', but got '{}'",
            self.context,
            substring,
            actual
        );
        self
    }
    pub fn child_count(self, expected: usize) -> Self {
        let actual = visible_len(self.session.children());
        assert_eq!(
            actual,
            expected,
            "{}: Expected {} children, found {} children: [{}]",
            self.context,
            expected,
            actual,
            summarize_items(self.session.children())
        );
        self
    }
    pub fn child<F>(self, index: usize, assertion: F) -> Self
    where
        F: FnOnce(ContentItemAssertion<'a>),
    {
        let children = self.session.children();
        let visible_children = visible_len(children);
        assert!(
            index < visible_children,
            "{}: Child index {} out of bounds (session has {} children)",
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
            children: self.session.children(),
            context: format!("{}:children", self.context),
        });
        self
    }

    pub fn annotation_count(self, expected: usize) -> Self {
        let actual = self.session.annotations.len();
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
            index < self.session.annotations.len(),
            "{}: Annotation index {} out of bounds (session has {} annotations)",
            self.context,
            index,
            self.session.annotations.len()
        );
        let annotation = &self.session.annotations[index];
        assertion(AnnotationAssertion {
            annotation,
            context: format!("{}:annotations[{}]", self.context, index),
        });
        self
    }
}
