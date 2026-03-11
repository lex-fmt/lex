//! Line-based text diffing utilities for testing
//!
//! Provides utilities for comparing text strings line-by-line with clear
//! reporting of differences. This is especially useful for testing formatters
//! and serializers where exact text output matters.

/// Assert that two strings are equal, with line-by-line diff on failure
///
/// This function compares two strings line by line and provides detailed
/// error messages showing exactly which lines differ.
///
/// # Panics
///
/// Panics if the strings are not equal, with a detailed diff showing:
/// - Line numbers where differences occur
/// - Expected vs actual content for each differing line
/// - Lines that were added or removed
///
/// # Example
///
/// ```
/// use lex_parser::lex::testing::text_diff::assert_text_eq;
///
/// let expected = "line 1\nline 2\nline 3";
/// let actual = "line 1\nline 2\nline 3";
/// assert_text_eq(expected, actual); // passes
/// ```
pub fn assert_text_eq(expected: &str, actual: &str) {
    if expected == actual {
        return;
    }

    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    let mut diff_lines = Vec::new();
    let max_lines = expected_lines.len().max(actual_lines.len());

    for i in 0..max_lines {
        let expected_line = expected_lines.get(i);
        let actual_line = actual_lines.get(i);

        match (expected_line, actual_line) {
            (Some(exp), Some(act)) if exp == act => {
                // Lines match, no diff
            }
            (Some(exp), Some(act)) => {
                diff_lines.push(format!("Line {}: MISMATCH", i + 1));
                diff_lines.push(format!("  Expected: {exp:?}"));
                diff_lines.push(format!("  Actual:   {act:?}"));
            }
            (Some(exp), None) => {
                diff_lines.push(format!("Line {}: MISSING in actual", i + 1));
                diff_lines.push(format!("  Expected: {exp:?}"));
            }
            (None, Some(act)) => {
                diff_lines.push(format!("Line {}: EXTRA in actual", i + 1));
                diff_lines.push(format!("  Actual:   {act:?}"));
            }
            (None, None) => unreachable!(),
        }
    }

    if !diff_lines.is_empty() {
        panic!(
            "\n\nText comparison failed:\n{}\n\nExpected ({} lines):\n{}\n\nActual ({} lines):\n{}\n",
            diff_lines.join("\n"),
            expected_lines.len(),
            expected,
            actual_lines.len(),
            actual
        );
    }
}

/// Compare two strings and return a diff report without panicking
///
/// Returns `None` if the strings are equal, or `Some(diff_report)` if they differ.
pub fn diff_text(expected: &str, actual: &str) -> Option<String> {
    if expected == actual {
        return None;
    }

    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    let mut diff_lines = Vec::new();
    let max_lines = expected_lines.len().max(actual_lines.len());

    for i in 0..max_lines {
        let expected_line = expected_lines.get(i);
        let actual_line = actual_lines.get(i);

        match (expected_line, actual_line) {
            (Some(exp), Some(act)) if exp == act => {
                // Lines match
            }
            (Some(exp), Some(act)) => {
                diff_lines.push(format!("Line {}: MISMATCH", i + 1));
                diff_lines.push(format!("  Expected: {exp:?}"));
                diff_lines.push(format!("  Actual:   {act:?}"));
            }
            (Some(exp), None) => {
                diff_lines.push(format!("Line {}: MISSING in actual", i + 1));
                diff_lines.push(format!("  Expected: {exp:?}"));
            }
            (None, Some(act)) => {
                diff_lines.push(format!("Line {}: EXTRA in actual", i + 1));
                diff_lines.push(format!("  Actual:   {act:?}"));
            }
            (None, None) => unreachable!(),
        }
    }

    if diff_lines.is_empty() {
        None
    } else {
        Some(format!(
            "Text differs:\n{}\n\nExpected ({} lines):\n{}\n\nActual ({} lines):\n{}",
            diff_lines.join("\n"),
            expected_lines.len(),
            expected,
            actual_lines.len(),
            actual
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assert_text_eq_identical() {
        assert_text_eq("hello\nworld", "hello\nworld");
    }

    #[test]
    fn test_assert_text_eq_empty() {
        assert_text_eq("", "");
    }

    #[test]
    #[should_panic(expected = "Text comparison failed")]
    fn test_assert_text_eq_different() {
        assert_text_eq("hello\nworld", "hello\nplanet");
    }

    #[test]
    #[should_panic(expected = "MISSING in actual")]
    fn test_assert_text_eq_missing_line() {
        assert_text_eq("hello\nworld\nfoo", "hello\nworld");
    }

    #[test]
    #[should_panic(expected = "EXTRA in actual")]
    fn test_assert_text_eq_extra_line() {
        assert_text_eq("hello\nworld", "hello\nworld\nfoo");
    }

    #[test]
    fn test_diff_text_identical() {
        assert_eq!(diff_text("hello\nworld", "hello\nworld"), None);
    }

    #[test]
    fn test_diff_text_different() {
        let result = diff_text("hello\nworld", "hello\nplanet");
        assert!(result.is_some());
        let diff = result.unwrap();
        assert!(diff.contains("Line 2: MISMATCH"));
        assert!(diff.contains("world"));
        assert!(diff.contains("planet"));
    }
}
