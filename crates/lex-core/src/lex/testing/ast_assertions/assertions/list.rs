//! List and ListItem assertions

use super::{
    annotation::AnnotationAssertion, summarize_items, visible_len, visible_nth, ChildrenAssertion,
};
use crate::lex::ast::traits::{AstNode, Container};
use crate::lex::ast::{ContentItem, List, ListItem};
use crate::lex::testing::ast_assertions::ContentItemAssertion;
use crate::lex::testing::matchers::TextMatch;

pub struct ListAssertion<'a> {
    pub(crate) list: &'a List,
    pub(crate) context: String,
}

impl<'a> ListAssertion<'a> {
    pub fn item_count(self, expected: usize) -> Self {
        let actual = visible_len(&self.list.items);
        assert_eq!(
            actual, expected,
            "{}: Expected {} list items, found {} list items",
            self.context, expected, actual
        );
        self
    }
    pub fn item<F>(self, index: usize, assertion: F) -> Self
    where
        F: FnOnce(ListItemAssertion<'a>),
    {
        let visible_items = visible_len(&self.list.items);
        assert!(
            index < visible_items,
            "{}: Item index {} out of bounds (list has {} items)",
            self.context,
            index,
            visible_items
        );
        let content_item = visible_nth(&self.list.items, index)
            .expect("visible list item should exist at computed index");
        let item = if let ContentItem::ListItem(li) = content_item {
            li
        } else {
            panic!(
                "{}: Expected ListItem at index {}, but found {:?}",
                self.context,
                index,
                content_item.node_type()
            );
        };
        assertion(ListItemAssertion {
            item,
            context: format!("{}:items[{}]", self.context, index),
        });
        self
    }

    pub fn annotation_count(self, expected: usize) -> Self {
        let actual = self.list.annotations.len();
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
            index < self.list.annotations.len(),
            "{}: Annotation index {} out of bounds (list has {} annotations)",
            self.context,
            index,
            self.list.annotations.len()
        );
        let annotation = &self.list.annotations[index];
        assertion(AnnotationAssertion {
            annotation,
            context: format!("{}:annotations[{}]", self.context, index),
        });
        self
    }
}

pub struct ListItemAssertion<'a> {
    pub(crate) item: &'a ListItem,
    pub(crate) context: String,
}

impl<'a> ListItemAssertion<'a> {
    pub fn text(self, expected: &str) -> Self {
        TextMatch::Exact(expected.to_string()).assert(self.item.text(), &self.context);
        self
    }
    pub fn marker(self, expected: &str) -> Self {
        TextMatch::Exact(expected.to_string()).assert(self.item.marker(), &self.context);
        self
    }
    pub fn text_starts_with(self, prefix: &str) -> Self {
        TextMatch::StartsWith(prefix.to_string()).assert(self.item.text(), &self.context);
        self
    }
    pub fn text_contains(self, substring: &str) -> Self {
        TextMatch::Contains(substring.to_string()).assert(self.item.text(), &self.context);
        self
    }
    pub fn child_count(self, expected: usize) -> Self {
        let actual = visible_len(self.item.children());
        assert_eq!(
            actual,
            expected,
            "{}: Expected {} children, found {} children: [{}]",
            self.context,
            expected,
            actual,
            summarize_items(self.item.children())
        );
        self
    }
    pub fn child<F>(self, index: usize, assertion: F) -> Self
    where
        F: FnOnce(ContentItemAssertion<'a>),
    {
        let children = self.item.children();
        let visible_children = visible_len(children);
        assert!(
            index < visible_children,
            "{}: Child index {} out of bounds (list item has {} children)",
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
            children: self.item.children(),
            context: format!("{}:children", self.context),
        });
        self
    }

    pub fn annotation_count(self, expected: usize) -> Self {
        let actual = self.item.annotations.len();
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
            index < self.item.annotations.len(),
            "{}: Annotation index {} out of bounds (list item has {} annotations)",
            self.context,
            index,
            self.item.annotations.len()
        );
        let annotation = &self.item.annotations[index];
        assertion(AnnotationAssertion {
            annotation,
            context: format!("{}:annotations[{}]", self.context, index),
        });
        self
    }
}
