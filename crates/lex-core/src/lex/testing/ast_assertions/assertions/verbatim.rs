//! Verbatim block assertions

use super::annotation::AnnotationAssertion;
use crate::lex::ast::elements::container::VerbatimContainer;
use crate::lex::ast::elements::verbatim::VerbatimBlockMode;
use crate::lex::ast::{ContentItem, TextContent, Verbatim};

pub struct VerbatimBlockkAssertion<'a> {
    pub(crate) verbatim_block: &'a Verbatim,
    pub(crate) context: String,
}

impl<'a> VerbatimBlockkAssertion<'a> {
    pub fn subject(self, expected: &str) -> Self {
        let actual = self.verbatim_block.subject.as_string();
        assert_eq!(
            actual, expected,
            "{}: Expected verbatim block subject to be '{}', but got '{}'",
            self.context, expected, actual
        );
        self
    }
    pub fn content_contains(self, substring: &str) -> Self {
        let actual = collect_verbatim_content(&self.verbatim_block.children);

        assert!(
            actual.contains(substring),
            "{}: Expected verbatim block content to contain '{}', but got '{}'",
            self.context,
            substring,
            actual
        );
        self
    }
    pub fn assert_marker_form(self) -> Self {
        assert!(
            self.verbatim_block.children.is_empty(),
            "{}: Expected verbatim block to be marker form (empty children), but got {} children",
            self.context,
            self.verbatim_block.children.len()
        );
        self
    }
    pub fn closing_label(self, expected: &str) -> Self {
        let actual = &self.verbatim_block.closing_data.label.value;
        assert_eq!(
            actual, expected,
            "{}: Expected closing data label to be '{}', but got '{}'",
            self.context, expected, actual
        );
        self
    }

    pub fn mode(self, expected: VerbatimBlockMode) -> Self {
        assert_eq!(
            self.verbatim_block.mode, expected,
            "{}: Expected verbatim block mode {:?}, got {:?}",
            self.context, expected, self.verbatim_block.mode
        );
        self
    }

    pub fn line_count(self, expected: usize) -> Self {
        let actual = self.verbatim_block.children.len();
        assert_eq!(
            actual, expected,
            "{}: Expected verbatim block to have {} lines, but got {}",
            self.context, expected, actual
        );
        self
    }

    pub fn line_eq(self, index: usize, expected: &str) -> Self {
        let line = self.verbatim_block.children.get(index).unwrap_or_else(|| {
            panic!(
                "{}: Verbatim line index {} out of bounds ({} lines)",
                self.context,
                index,
                self.verbatim_block.children.len()
            )
        });

        match line {
            ContentItem::VerbatimLine(line) => {
                let actual = line.content.as_string();
                assert_eq!(
                    actual, expected,
                    "{}: Expected verbatim line {} to be '{}', but got '{}'",
                    self.context, index, expected, actual
                );
            }
            other => panic!(
                "{}: Expected verbatim line at index {}, found {:?}",
                self.context, index, other
            ),
        }

        self
    }
    pub fn has_closing_parameter_with_value(self, key: &str, value: &str) -> Self {
        let found = self
            .verbatim_block
            .closing_data
            .parameters
            .iter()
            .any(|p| p.key == key && p.value == value);
        assert!(
            found,
            "{}: Expected closing data to have parameter '{}={}'",
            self.context, key, value
        );
        self
    }

    pub fn group_count(self, expected: usize) -> Self {
        let actual = self.verbatim_block.group_len();
        assert_eq!(
            actual, expected,
            "{}: Expected {} verbatim groups, found {}",
            self.context, expected, actual
        );
        self
    }

    pub fn group<F>(self, index: usize, assertion: F) -> Self
    where
        F: FnOnce(VerbatimGroupAssertion<'a>),
    {
        let group_ref = self.verbatim_block.group().nth(index).unwrap_or_else(|| {
            panic!(
                "{}: Verbatim group index {} out of bounds ({} groups)",
                self.context,
                index,
                self.verbatim_block.group_len()
            )
        });

        assertion(VerbatimGroupAssertion {
            subject: group_ref.subject,
            children: group_ref.children,
            context: format!("{}::group[{}]", self.context, index),
        });

        self
    }

    pub fn annotation_count(self, expected: usize) -> Self {
        let actual = self.verbatim_block.annotations.len();
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
            index < self.verbatim_block.annotations.len(),
            "{}: Annotation index {} out of bounds (verbatim block has {} annotations)",
            self.context,
            index,
            self.verbatim_block.annotations.len()
        );
        let annotation = &self.verbatim_block.annotations[index];
        assertion(AnnotationAssertion {
            annotation,
            context: format!("{}:annotations[{}]", self.context, index),
        });
        self
    }
}

pub struct VerbatimGroupAssertion<'a> {
    pub(crate) subject: &'a TextContent,
    pub(crate) children: &'a VerbatimContainer,
    pub(crate) context: String,
}

impl<'a> VerbatimGroupAssertion<'a> {
    pub fn subject(self, expected: &str) -> Self {
        let actual = self.subject.as_string();
        assert_eq!(
            actual, expected,
            "{}: Expected verbatim group subject to be '{}', but got '{}'",
            self.context, expected, actual
        );
        self
    }

    pub fn content_contains(self, substring: &str) -> Self {
        let actual = collect_verbatim_content(self.children);
        assert!(
            actual.contains(substring),
            "{}: Expected verbatim group content to contain '{}', but got '{}'",
            self.context,
            substring,
            actual
        );
        self
    }

    pub fn line_count(self, expected: usize) -> Self {
        let actual = self.children.len();
        assert_eq!(
            actual, expected,
            "{}: Expected verbatim group to have {} lines, but got {}",
            self.context, expected, actual
        );
        self
    }

    pub fn line_eq(self, index: usize, expected: &str) -> Self {
        let line = self.children.get(index).unwrap_or_else(|| {
            panic!(
                "{}: Verbatim group line index {} out of bounds ({} lines)",
                self.context,
                index,
                self.children.len()
            )
        });

        match line {
            ContentItem::VerbatimLine(line) => {
                let actual = line.content.as_string();
                assert_eq!(
                    actual, expected,
                    "{}: Expected verbatim group line {} to be '{}', but got '{}'",
                    self.context, index, expected, actual
                );
            }
            other => panic!(
                "{}: Expected verbatim line at index {}, found {:?}",
                self.context, index, other
            ),
        }

        self
    }

    pub fn assert_marker_form(self) -> Self {
        assert!(
            self.children.is_empty(),
            "{}: Expected group marker form to be empty, but got {} lines",
            self.context,
            self.children.len()
        );
        self
    }
}

fn collect_verbatim_content(children: &VerbatimContainer) -> String {
    children
        .iter()
        .filter_map(|child| {
            if let ContentItem::VerbatimLine(line) = child {
                Some(line.content.as_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
