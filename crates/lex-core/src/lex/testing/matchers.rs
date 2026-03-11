//! Text matching utilities for AST assertions

/// Text matching strategies for assertions
#[derive(Debug, Clone)]
pub enum TextMatch {
    /// Exact text match
    Exact(String),
    /// Text starts with prefix
    StartsWith(String),
    /// Text contains substring
    Contains(String),
}

impl TextMatch {
    /// Check if the actual text matches this pattern (returns bool)
    pub fn matches(&self, actual: &str) -> bool {
        match self {
            TextMatch::Exact(expected) => actual == expected,
            TextMatch::StartsWith(prefix) => actual.starts_with(prefix),
            TextMatch::Contains(substring) => actual.contains(substring),
        }
    }

    /// Assert that the actual text matches this pattern
    pub fn assert(&self, actual: &str, context: &str) {
        match self {
            TextMatch::Exact(expected) => {
                assert_eq!(
                    actual, expected,
                    "{context}: Expected text to be '{expected}', but got '{actual}'"
                );
            }
            TextMatch::StartsWith(prefix) => {
                assert!(
                    actual.starts_with(prefix),
                    "{context}: Expected text to start with '{prefix}', but got '{actual}'"
                );
            }
            TextMatch::Contains(substring) => {
                assert!(
                    actual.contains(substring),
                    "{context}: Expected text to contain '{substring}', but got '{actual}'"
                );
            }
        }
    }
}
