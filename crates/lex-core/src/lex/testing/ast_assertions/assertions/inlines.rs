//! Inline content assertions used by parser tests.

use crate::lex::ast::TextContent;
use crate::lex::inlines::{InlineContent, InlineNode, ReferenceInline, ReferenceType};
use crate::lex::testing::matchers::TextMatch;

#[allow(dead_code)]
pub struct InlineAssertion {
    nodes: InlineContent,
    context: String,
}

#[allow(dead_code)]
impl InlineAssertion {
    pub fn new(content: &TextContent, context: impl Into<String>) -> Self {
        Self {
            nodes: content.inline_items(),
            context: context.into(),
        }
    }

    /// Assert that the inline list starts with the provided expectations.
    ///
    /// This mirrors the workflow described in the inline proposal: tests only
    /// need to check the prefix of the inline list for quick sanity checks.
    pub fn starts_with(self, expectations: &[InlineExpectation]) -> Self {
        assert!(
            self.nodes.len() >= expectations.len(),
            "{}: Inline list shorter than expected (have {}, need {})",
            self.context,
            self.nodes.len(),
            expectations.len()
        );
        for (idx, expectation) in expectations.iter().enumerate() {
            let actual = &self.nodes[idx];
            expectation.assert(actual, &format!("{}:inline[{}]", self.context, idx));
        }
        self
    }

    /// Assert the total amount of inline nodes.
    pub fn length(self, expected: usize) -> Self {
        assert_eq!(
            self.nodes.len(),
            expected,
            "{}: Expected {} inline nodes, found {}",
            self.context,
            expected,
            self.nodes.len()
        );
        self
    }

    /// Exposes the raw inline nodes for custom assertions.
    pub fn nodes(&self) -> &[InlineNode] {
        &self.nodes
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct InlineExpectation {
    kind: InlineExpectationKind,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum InlineExpectationKind {
    Plain(TextMatch),
    Strong(Vec<InlineExpectation>),
    Emphasis(Vec<InlineExpectation>),
    Code(TextMatch),
    Math(TextMatch),
    Reference(ReferenceExpectation),
}

#[allow(dead_code)]
impl InlineExpectation {
    pub fn plain_text(text: impl Into<String>) -> Self {
        Self {
            kind: InlineExpectationKind::Plain(TextMatch::Exact(text.into())),
        }
    }

    pub fn plain(match_kind: TextMatch) -> Self {
        Self {
            kind: InlineExpectationKind::Plain(match_kind),
        }
    }

    pub fn strong(children: Vec<InlineExpectation>) -> Self {
        Self {
            kind: InlineExpectationKind::Strong(children),
        }
    }

    pub fn strong_text(text: impl Into<String>) -> Self {
        Self::strong(vec![InlineExpectation::plain_text(text.into())])
    }

    pub fn emphasis(children: Vec<InlineExpectation>) -> Self {
        Self {
            kind: InlineExpectationKind::Emphasis(children),
        }
    }

    pub fn emphasis_text(text: impl Into<String>) -> Self {
        Self::emphasis(vec![InlineExpectation::plain_text(text.into())])
    }

    pub fn code_text(text: impl Into<String>) -> Self {
        Self {
            kind: InlineExpectationKind::Code(TextMatch::Exact(text.into())),
        }
    }

    pub fn math_text(text: impl Into<String>) -> Self {
        Self {
            kind: InlineExpectationKind::Math(TextMatch::Exact(text.into())),
        }
    }

    pub fn reference(expectation: ReferenceExpectation) -> Self {
        Self {
            kind: InlineExpectationKind::Reference(expectation),
        }
    }

    fn assert(&self, actual: &InlineNode, context: &str) {
        match (&self.kind, actual) {
            (InlineExpectationKind::Plain(matcher), InlineNode::Plain { text, .. }) => {
                matcher.assert(text, context);
            }
            (
                InlineExpectationKind::Strong(expect_children),
                InlineNode::Strong { content, .. },
            ) => {
                assert_inline_children(content, expect_children, context);
            }
            (
                InlineExpectationKind::Emphasis(expect_children),
                InlineNode::Emphasis { content, .. },
            ) => {
                assert_inline_children(content, expect_children, context);
            }
            (InlineExpectationKind::Code(matcher), InlineNode::Code { text, .. }) => {
                matcher.assert(text, context);
            }
            (InlineExpectationKind::Math(matcher), InlineNode::Math { text, .. }) => {
                matcher.assert(text, context);
            }
            (InlineExpectationKind::Reference(expectation), InlineNode::Reference { data, .. }) => {
                expectation.assert(data, context);
            }
            (expected, got) => panic!("{context}: Expected inline {expected:?}, got {got:?}"),
        }
    }
}

#[allow(dead_code)]
fn assert_inline_children(actual: &InlineContent, expected: &[InlineExpectation], context: &str) {
    assert!(
        actual.len() >= expected.len(),
        "{}: Inline child list shorter than expected (have {}, need {})",
        context,
        actual.len(),
        expected.len()
    );
    for (idx, expectation) in expected.iter().enumerate() {
        let child_context = format!("{context}:child[{idx}]");
        expectation.assert(&actual[idx], &child_context);
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ReferenceExpectation {
    expected: ReferenceTypeExpectation,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ReferenceTypeExpectation {
    Url(TextMatch),
    File(TextMatch),
    Citation {
        keys: Vec<TextMatch>,
        locator: Option<TextMatch>,
    },
    Tk(Option<TextMatch>),
    FootnoteLabeled(TextMatch),
    FootnoteNumber(u32),
    Session(TextMatch),
    General(TextMatch),
    NotSure,
}

#[allow(dead_code)]
impl ReferenceExpectation {
    pub fn url(target: TextMatch) -> Self {
        Self {
            expected: ReferenceTypeExpectation::Url(target),
        }
    }

    pub fn file(target: TextMatch) -> Self {
        Self {
            expected: ReferenceTypeExpectation::File(target),
        }
    }

    pub fn citation(target: TextMatch) -> Self {
        Self {
            expected: ReferenceTypeExpectation::Citation {
                keys: vec![target],
                locator: None,
            },
        }
    }

    pub fn citation_with_locator(keys: Vec<TextMatch>, locator: Option<TextMatch>) -> Self {
        Self {
            expected: ReferenceTypeExpectation::Citation { keys, locator },
        }
    }

    pub fn tk(identifier: Option<TextMatch>) -> Self {
        Self {
            expected: ReferenceTypeExpectation::Tk(identifier),
        }
    }

    pub fn footnote_labeled(label: TextMatch) -> Self {
        Self {
            expected: ReferenceTypeExpectation::FootnoteLabeled(label),
        }
    }

    pub fn footnote_number(number: u32) -> Self {
        Self {
            expected: ReferenceTypeExpectation::FootnoteNumber(number),
        }
    }

    pub fn session(target: TextMatch) -> Self {
        Self {
            expected: ReferenceTypeExpectation::Session(target),
        }
    }

    pub fn general(target: TextMatch) -> Self {
        Self {
            expected: ReferenceTypeExpectation::General(target),
        }
    }

    pub fn not_sure() -> Self {
        Self {
            expected: ReferenceTypeExpectation::NotSure,
        }
    }

    fn assert(&self, actual: &ReferenceInline, context: &str) {
        match (&self.expected, &actual.reference_type) {
            (ReferenceTypeExpectation::Url(expected), ReferenceType::Url { target })
            | (ReferenceTypeExpectation::File(expected), ReferenceType::File { target })
            | (ReferenceTypeExpectation::Session(expected), ReferenceType::Session { target })
            | (ReferenceTypeExpectation::General(expected), ReferenceType::General { target }) => {
                expected.assert(target, context);
            }
            (
                ReferenceTypeExpectation::Citation { keys, locator },
                ReferenceType::Citation(data),
            ) => {
                assert_eq!(
                    keys.len(),
                    data.keys.len(),
                    "{}: Expected {} citation keys, got {}",
                    context,
                    keys.len(),
                    data.keys.len()
                );
                for (idx, matcher) in keys.iter().enumerate() {
                    matcher.assert(&data.keys[idx], &format!("{context}:key[{idx}]"));
                }
                match (locator, &data.locator) {
                    (None, None) => {}
                    (Some(expected_locator), Some(actual_locator)) => {
                        expected_locator.assert(&actual_locator.raw, context);
                    }
                    (None, Some(_)) => {}
                    (Some(_), None) => {
                        panic!("{context}: Expected citation locator, but none present")
                    }
                }
            }
            (
                ReferenceTypeExpectation::Tk(expected_identifier),
                ReferenceType::ToCome { identifier },
            ) => match (expected_identifier, identifier) {
                (None, None) => {}
                (Some(matcher), Some(value)) => matcher.assert(value, context),
                (None, Some(value)) => {
                    panic!("{context}: Expected TK without identifier, got {value}")
                }
                (Some(_), None) => {
                    panic!("{context}: Expected TK with identifier, but none present")
                }
            },
            (
                ReferenceTypeExpectation::FootnoteLabeled(expected),
                ReferenceType::FootnoteLabeled { label },
            ) => expected.assert(label, context),
            (
                ReferenceTypeExpectation::FootnoteNumber(expected_number),
                ReferenceType::FootnoteNumber { number },
            ) => assert_eq!(
                expected_number, number,
                "{context}: Expected footnote number {expected_number}, got {number}"
            ),
            (ReferenceTypeExpectation::NotSure, ReferenceType::NotSure) => {}
            (expected, got) => panic!("{context}: Expected reference {expected:?}, got {got:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asserts_inline_prefix() {
        let content = TextContent::from_string("Welcome to *the* party".into(), None);
        InlineAssertion::new(&content, "paragraph.lines[0]")
            .starts_with(&[
                InlineExpectation::plain_text("Welcome to "),
                InlineExpectation::strong_text("the"),
                InlineExpectation::plain_text(" party"),
            ])
            .length(3);
    }

    #[test]
    #[should_panic(expected = "paragraph.lines[0]:inline[0]")]
    fn detects_mismatched_inline() {
        let content = TextContent::from_string("*value*".into(), None);
        InlineAssertion::new(&content, "paragraph.lines[0]")
            .starts_with(&[InlineExpectation::plain_text("value")]);
    }

    #[test]
    fn matches_reference_inline() {
        let content = TextContent::from_string("See [https://example.com]".into(), None);
        InlineAssertion::new(&content, "paragraph.lines[0]").starts_with(&[
            InlineExpectation::plain_text("See "),
            InlineExpectation::reference(ReferenceExpectation::url(TextMatch::Exact(
                "https://example.com".into(),
            ))),
        ]);
    }

    #[test]
    fn matches_citation_inline() {
        let content = TextContent::from_string("See [@doe2024, p.45-46]".into(), None);
        InlineAssertion::new(&content, "paragraph.lines[0]").starts_with(&[
            InlineExpectation::plain_text("See "),
            InlineExpectation::reference(ReferenceExpectation::citation_with_locator(
                vec![TextMatch::Exact("doe2024".into())],
                Some(TextMatch::Exact("p.45-46".into())),
            )),
        ]);
    }
}
