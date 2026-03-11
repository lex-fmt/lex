//! Paragraph assertions

use super::annotation::AnnotationAssertion;
use crate::lex::ast::Paragraph;
use crate::lex::testing::matchers::TextMatch;

pub struct ParagraphAssertion<'a> {
    pub(crate) para: &'a Paragraph,
    pub(crate) context: String,
}

impl<'a> ParagraphAssertion<'a> {
    pub fn text(self, expected: &str) -> Self {
        TextMatch::Exact(expected.to_string()).assert(&self.para.text(), &self.context);
        self
    }
    pub fn text_starts_with(self, prefix: &str) -> Self {
        TextMatch::StartsWith(prefix.to_string()).assert(&self.para.text(), &self.context);
        self
    }
    pub fn text_contains(self, substring: &str) -> Self {
        TextMatch::Contains(substring.to_string()).assert(&self.para.text(), &self.context);
        self
    }
    pub fn line_count(self, expected: usize) -> Self {
        let actual = self.para.lines.len();
        assert_eq!(
            actual, expected,
            "{}: Expected {} lines, found {} lines",
            self.context, expected, actual
        );
        self
    }

    pub fn annotation_count(self, expected: usize) -> Self {
        let actual = self.para.annotations.len();
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
            index < self.para.annotations.len(),
            "{}: Annotation index {} out of bounds (paragraph has {} annotations)",
            self.context,
            index,
            self.para.annotations.len()
        );
        let annotation = &self.para.annotations[index];
        assertion(AnnotationAssertion {
            annotation,
            context: format!("{}:annotations[{}]", self.context, index),
        });
        self
    }
}
