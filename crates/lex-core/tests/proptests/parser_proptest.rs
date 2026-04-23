//! Property-based tests for the lex parser
//!
//! These tests use the proptest framework to generate random inputs and verify
//! that the parser behaves correctly (e.g. never panics) across a wide range
//! of inputs, including valid, invalid, and pathological cases.

use lex_core::lex::parsing::parse_document;
use proptest::prelude::*;

// Property-based tests using the strategy
proptest! {
    // We limit cases to avoid extremely long test runs in CI, but
    // it can be increased for deep local fuzzing
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn test_parse_document_never_panics_on_any_utf8(s in "\\PC*") {
        // The parser should never panic on any valid UTF-8 string.
        // It might return an error or parse it as plain text, but it must not crash.
        let _ = parse_document(&s);
    }
}
fn lex_text_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            "[a-zA-Z0-9]+",
            "[a-zA-Z0-9]+ [a-zA-Z0-9]+",
            "[a-zA-Z0-9]+[.,!?]",
            "",
        ],
        1..10,
    )
    .prop_map(|lines| lines.join("\n"))
}

fn list_item_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "- [a-zA-Z0-9 ]+",
        "[0-9]+\\. [a-zA-Z0-9 ]+",
        "[a-z]\\. [a-zA-Z0-9 ]+",
        "\\([0-9]+\\) [a-zA-Z0-9 ]+",
    ]
}

fn session_title_strategy() -> impl Strategy<Value = String> {
    prop_oneof!["[0-9]+\\. [a-zA-Z0-9 ]+", "[a-zA-Z0-9 ]+:", "[a-zA-Z0-9 ]+",]
}

fn lex_document_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            lex_text_strategy(),
            list_item_strategy(),
            session_title_strategy(),
        ],
        1..20,
    )
    .prop_map(|lines| lines.join("\n"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn test_parse_document_never_panics_on_valid_looking_lex(input in lex_document_strategy()) {
        let _ = parse_document(&input);
    }
}

/// Generate valid Lex verbatim blocks: Subject:\n    content\n:: label ::
fn verbatim_block_strategy() -> impl Strategy<Value = String> {
    (
        "[A-Z][a-zA-Z0-9 ]{1,15}",
        prop::collection::vec("[a-zA-Z0-9 ]+", 1..5),
        "[a-z][a-z0-9_-]{0,8}",
    )
        .prop_map(|(subject, lines, label)| {
            let subject = subject.trim_end().to_string();
            let content = lines
                .iter()
                .map(|l| format!("    {l}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{subject}:\n{content}\n:: {label} ::")
        })
}

/// Generate valid Lex annotations: :: label :: or :: label key=value ::
fn annotation_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Marker annotation
        "[a-z][a-z0-9_-]{0,8}".prop_map(|label| format!(":: {label} ::")),
        // Annotation with parameter
        ("[a-z][a-z0-9_-]{0,8}", "[a-z][a-z0-9_]{0,6}", "[a-z0-9]+",)
            .prop_map(|(label, key, value)| format!(":: {label} {key}={value} ::")),
    ]
}

/// Generate valid Lex inline text (using actual Lex inline syntax)
fn inline_text_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "This is *[a-z]+* text.",
        "This is _[a-z]+_ text.",
        "This is `[a-z]+` text.",
        "This has #[a-z]+# math.",
        "See \\[@[a-z]+\\] here.",
        "Mixed *[a-z]+* and _[a-z]+_ text.",
        "Reference \\[\\^[a-z]+\\] note.",
    ]
}

fn expanded_lex_document_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            lex_text_strategy(),
            list_item_strategy(),
            session_title_strategy(),
            verbatim_block_strategy(),
            annotation_strategy(),
            inline_text_strategy(),
        ],
        1..30,
    )
    .prop_map(|lines| lines.join("\n\n")) // Join with double newlines to ensure separation
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn test_parse_document_never_panics_on_expanded_lex(input in expanded_lex_document_strategy()) {
        let _ = parse_document(&input);
    }
}
