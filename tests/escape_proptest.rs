//! Property-based tests for escape/unescape logic

use lex_core::lex::escape::{escape_inline, escape_quoted, unescape_inline, unescape_quoted};
use proptest::prelude::*;

/// Arbitrary string that may contain inline special characters and backslashes.
fn inline_text_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Plain ASCII text
        "[a-zA-Z0-9 ,.!?;:'-]{0,50}",
        // Text with inline markers
        "[a-zA-Z0-9 *_`#\\[\\]\\\\]{0,50}",
        // Paths with backslashes
        "[a-zA-Z]:\\\\[a-zA-Z0-9\\\\]{1,30}",
        // Dense special characters
        "[*_`#\\[\\]\\\\]{0,20}",
        // Any printable ASCII
        "[ -~]{0,80}",
    ]
}

/// Strings that represent valid "plain text" (what a user intends to display).
fn plain_content_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-zA-Z0-9 ,.!?;:'-]{1,50}",
        "[a-zA-Z0-9 *_`#\\[\\]\\\\]{1,50}",
        "[ -~]{1,80}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn roundtrip_escape_unescape(text in plain_content_strategy()) {
        let escaped = escape_inline(&text);
        let back = unescape_inline(&escaped);
        prop_assert_eq!(&back, &text, "roundtrip failed: {:?} -> {:?} -> {:?}", text, escaped, back);
    }

    #[test]
    fn unescape_never_panics(text in inline_text_strategy()) {
        let _ = unescape_inline(&text);
    }

    #[test]
    fn escape_never_panics(text in inline_text_strategy()) {
        let _ = escape_inline(&text);
    }

    #[test]
    fn escape_output_contains_no_bare_specials(text in plain_content_strategy()) {
        let escaped = escape_inline(&text);
        // Every special character in the output must be preceded by a backslash
        let chars: Vec<char> = escaped.chars().collect();
        for (i, &ch) in chars.iter().enumerate() {
            if matches!(ch, '*' | '_' | '`' | '#' | '[' | ']') {
                prop_assert!(
                    i > 0 && chars[i - 1] == '\\',
                    "bare special char {:?} at position {} in {:?}",
                    ch, i, escaped
                );
            }
        }
    }

    #[test]
    fn unescape_idempotent_on_no_backslash(text in "[a-zA-Z0-9 ,.!?;:'-]{0,50}") {
        // Text with no backslashes should pass through unchanged
        prop_assert_eq!(unescape_inline(&text), text);
    }

    #[test]
    fn escape_length_at_least_original(text in plain_content_strategy()) {
        let escaped = escape_inline(&text);
        prop_assert!(escaped.len() >= text.len(),
            "escaped ({}) shorter than original ({})", escaped.len(), text.len());
    }

    #[test]
    fn double_escape_roundtrips(text in plain_content_strategy()) {
        // escape(escape(text)) should roundtrip through two unescapes
        let once = escape_inline(&text);
        let twice = escape_inline(&once);
        let back_once = unescape_inline(&twice);
        let back_original = unescape_inline(&back_once);
        prop_assert_eq!(&back_original, &text,
            "double roundtrip failed: {:?}", text);
    }

    #[test]
    fn unescape_of_escaped_preserves_backslash_before_alpha(
        prefix in "[a-zA-Z]{1,5}",
        letter in "[a-zA-Z]",
        suffix in "[a-zA-Z]{1,5}"
    ) {
        // Backslash before alphanumeric should be preserved
        let input = format!("{prefix}\\{letter}{suffix}");
        let result = unescape_inline(&input);
        prop_assert_eq!(&result, &input,
            "backslash before alpha should be preserved");
    }
}

#[test]
fn unescape_all_inline_markers() {
    assert_eq!(unescape_inline("\\*"), "*");
    assert_eq!(unescape_inline("\\_"), "_");
    assert_eq!(unescape_inline("\\`"), "`");
    assert_eq!(unescape_inline("\\#"), "#");
    assert_eq!(unescape_inline("\\["), "[");
    assert_eq!(unescape_inline("\\]"), "]");
    assert_eq!(unescape_inline("\\\\"), "\\");
}

#[test]
fn escape_all_inline_markers() {
    assert_eq!(escape_inline("*"), "\\*");
    assert_eq!(escape_inline("_"), "\\_");
    assert_eq!(escape_inline("`"), "\\`");
    assert_eq!(escape_inline("#"), "\\#");
    assert_eq!(escape_inline("["), "\\[");
    assert_eq!(escape_inline("]"), "\\]");
    assert_eq!(escape_inline("\\"), "\\\\");
}

#[test]
fn realistic_windows_path() {
    let path = "C:\\Users\\name\\Documents";
    // Backslashes before alphanumeric are preserved
    assert_eq!(unescape_inline(path), path);
}

#[test]
fn realistic_mixed_content() {
    // User wants to display: The formula *x* uses [brackets]
    let escaped = "The formula \\*x\\* uses \\[brackets\\]";
    assert_eq!(unescape_inline(escaped), "The formula *x* uses [brackets]");
}

// --- Quoted parameter value prop tests ---

/// Content that may contain quotes and backslashes
fn quoted_content_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-zA-Z0-9 ,.!?;:'-]{0,30}",
        "[a-zA-Z0-9 \"\\\\]{0,20}",
        "[ -~]{0,40}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn roundtrip_quoted_escape_unescape(text in quoted_content_strategy()) {
        let escaped = escape_quoted(&text);
        let wrapped = format!("\"{escaped}\"");
        let back = unescape_quoted(&wrapped);
        prop_assert_eq!(&back, &text,
            "quoted roundtrip failed: {:?} -> {:?} -> {:?}", text, wrapped, back);
    }

    #[test]
    fn unescape_quoted_never_panics(text in "[ -~]{0,50}") {
        let _ = unescape_quoted(&text);
    }

    #[test]
    fn escape_quoted_never_panics(text in "[ -~]{0,50}") {
        let _ = escape_quoted(&text);
    }

    #[test]
    fn escape_quoted_no_bare_quotes(text in quoted_content_strategy()) {
        let escaped = escape_quoted(&text);
        // Every quote in the output must be preceded by a backslash
        let chars: Vec<char> = escaped.chars().collect();
        for (i, &ch) in chars.iter().enumerate() {
            if ch == '"' {
                prop_assert!(
                    i > 0 && chars[i - 1] == '\\',
                    "bare quote at position {} in {:?}", i, escaped
                );
            }
        }
    }

    #[test]
    fn escape_quoted_length_at_least_original(text in quoted_content_strategy()) {
        let escaped = escape_quoted(&text);
        prop_assert!(escaped.len() >= text.len());
    }
}
