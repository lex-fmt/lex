//! Property-based tests for inline references
//!
//! Covers all 8 ReferenceType variants: ToCome, Citation, FootnoteLabeled,
//! FootnoteNumber, Session, Url, File, General — none of which had proptest
//! coverage before.

use lex_core::lex::ast::ContentItem;
use lex_core::lex::parsing::parse_document;
use lex_core::lex::testing::{InlineAssertion, InlineExpectation, ReferenceExpectation, TextMatch};
use proptest::prelude::*;

// =============================================================================
// Helpers
// =============================================================================

fn extract_first_text_line_content(source: &str) -> lex_core::lex::ast::TextContent {
    let doc = parse_document(source)
        .unwrap_or_else(|e| panic!("Failed to parse: {e}\nSource:\n{source}"));
    let para = match &doc.root.children.iter().collect::<Vec<_>>()[0] {
        ContentItem::Paragraph(p) => p,
        other => panic!("Expected Paragraph, got {other:?}\nSource:\n{source}"),
    };
    let tl = match &para.lines[0] {
        ContentItem::TextLine(tl) => tl,
        other => panic!("Expected TextLine, got {other:?}\nSource:\n{source}"),
    };
    tl.content.clone()
}

// =============================================================================
// Strategies
// =============================================================================

/// Generate valid TK identifiers (lowercase + digits, max 20 chars)
fn tk_identifier_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{0,12}"
}

/// Generate valid citation keys (alphanumeric + common academic chars)
fn citation_key_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9_-]{1,15}"
}

/// Generate valid footnote labels
fn footnote_label_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9_-]{0,10}"
}

/// Generate valid footnote numbers
fn footnote_number_strategy() -> impl Strategy<Value = u32> {
    1..999u32
}

/// Generate valid session targets (digits with optional dots/dashes)
fn session_target_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "[0-9]{1,3}",
        "[0-9]{1,2}\\.[0-9]{1,2}",
        "[0-9]{1,2}\\.[0-9]{1,2}\\.[0-9]{1,2}",
    ]
}

/// Generate URL targets
fn url_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "https://[a-z]{3,10}\\.[a-z]{2,5}",
        "https://[a-z]{3,10}\\.[a-z]{2,5}/[a-z0-9/]{1,20}",
        "http://[a-z]{3,10}\\.[a-z]{2,5}",
        "mailto:[a-z]{3,8}@[a-z]{3,8}\\.[a-z]{2,4}",
    ]
}

/// Generate file path targets
fn file_target_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "\\./[a-z]{2,8}\\.[a-z]{2,4}",
        "\\./[a-z]{2,8}/[a-z]{2,8}\\.[a-z]{2,4}",
        "/[a-z]{2,8}/[a-z]{2,8}\\.[a-z]{2,4}",
    ]
}

/// Generate general reference targets (at least one alphabetic char)
fn general_target_strategy() -> impl Strategy<Value = String> {
    "[A-Z][a-zA-Z0-9 ]{1,20}"
        .prop_map(|s| s.trim_end().to_string())
        .prop_filter("must not be empty", |s| !s.is_empty())
}

// =============================================================================
// 1. ToCome References [TK] and [TK-identifier]
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn reference_tk_bare(prefix in "[A-Z][a-z]+ [a-z]+") {
        let source = format!("{prefix} [TK] end.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "TK reference")
            .starts_with(&[
                InlineExpectation::plain_text(format!("{prefix} ")),
                InlineExpectation::reference(ReferenceExpectation::tk(None)),
                InlineExpectation::plain_text(" end."),
            ])
            .length(3);
    }

    #[test]
    fn reference_tk_with_identifier(
        prefix in "[A-Z][a-z]+",
        id in tk_identifier_strategy(),
    ) {
        let source = format!("{prefix} [TK-{id}] end.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "TK-id reference")
            .starts_with(&[
                InlineExpectation::plain_text(format!("{prefix} ")),
                InlineExpectation::reference(ReferenceExpectation::tk(
                    Some(TextMatch::Exact(id))
                )),
                InlineExpectation::plain_text(" end."),
            ])
            .length(3);
    }
}

// =============================================================================
// 2. Citation References [@key]
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn reference_citation_single_key(key in citation_key_strategy()) {
        let source = format!("See [@{key}] here.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "citation reference")
            .starts_with(&[
                InlineExpectation::plain_text("See "),
                InlineExpectation::reference(ReferenceExpectation::citation(
                    TextMatch::Exact(key)
                )),
                InlineExpectation::plain_text(" here."),
            ])
            .length(3);
    }
}

// =============================================================================
// 3. Footnote References [^label] and [42]
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn reference_footnote_labeled(label in footnote_label_strategy()) {
        let source = format!("Text [^{label}] end.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "footnote labeled")
            .starts_with(&[
                InlineExpectation::plain_text("Text "),
                InlineExpectation::reference(ReferenceExpectation::footnote_labeled(
                    TextMatch::Exact(label)
                )),
                InlineExpectation::plain_text(" end."),
            ])
            .length(3);
    }

    #[test]
    fn reference_footnote_number(num in footnote_number_strategy()) {
        let source = format!("Text [{num}] end.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "footnote number")
            .starts_with(&[
                InlineExpectation::plain_text("Text "),
                InlineExpectation::reference(ReferenceExpectation::footnote_number(num)),
                InlineExpectation::plain_text(" end."),
            ])
            .length(3);
    }
}

// =============================================================================
// 4. Session References [#target]
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn reference_session(target in session_target_strategy()) {
        let source = format!("See [#{target}] here.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "session reference")
            .starts_with(&[
                InlineExpectation::plain_text("See "),
                InlineExpectation::reference(ReferenceExpectation::session(
                    TextMatch::Exact(target)
                )),
                InlineExpectation::plain_text(" here."),
            ])
            .length(3);
    }
}

// =============================================================================
// 5. URL References
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn reference_url(url in url_strategy()) {
        let source = format!("Visit [{url}] now.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "url reference")
            .starts_with(&[
                InlineExpectation::plain_text("Visit "),
                InlineExpectation::reference(ReferenceExpectation::url(
                    TextMatch::Exact(url)
                )),
                InlineExpectation::plain_text(" now."),
            ])
            .length(3);
    }
}

// =============================================================================
// 6. File References
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn reference_file(path in file_target_strategy()) {
        let source = format!("See [{path}] here.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "file reference")
            .starts_with(&[
                InlineExpectation::plain_text("See "),
                InlineExpectation::reference(ReferenceExpectation::file(
                    TextMatch::Exact(path)
                )),
                InlineExpectation::plain_text(" here."),
            ])
            .length(3);
    }
}

// =============================================================================
// 7. General References
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn reference_general(target in general_target_strategy()) {
        let source = format!("See [{target}] here.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "general reference")
            .starts_with(&[
                InlineExpectation::plain_text("See "),
                InlineExpectation::reference(ReferenceExpectation::general(
                    TextMatch::Exact(target)
                )),
                InlineExpectation::plain_text(" here."),
            ])
            .length(3);
    }
}

// =============================================================================
// 8. References inside markup (mixed inline)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn reference_inside_bold(
        word in "[a-zA-Z]{2,8}",
        key in citation_key_strategy(),
    ) {
        // Bold containing a reference: *word [@key]*
        let source = format!("Text *{word} [@{key}]* end.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "ref inside bold")
            .starts_with(&[
                InlineExpectation::plain_text("Text "),
                InlineExpectation::strong(vec![
                    InlineExpectation::plain_text(format!("{word} ")),
                    InlineExpectation::reference(ReferenceExpectation::citation(
                        TextMatch::Exact(key)
                    )),
                ]),
                InlineExpectation::plain_text(" end."),
            ])
            .length(3);
    }

    #[test]
    fn multiple_references_in_line(
        key1 in citation_key_strategy(),
        key2 in citation_key_strategy(),
    ) {
        let source = format!("See [@{key1}] and [@{key2}] end.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "multiple refs")
            .starts_with(&[
                InlineExpectation::plain_text("See "),
                InlineExpectation::reference(ReferenceExpectation::citation(
                    TextMatch::Exact(key1)
                )),
                InlineExpectation::plain_text(" and "),
                InlineExpectation::reference(ReferenceExpectation::citation(
                    TextMatch::Exact(key2)
                )),
                InlineExpectation::plain_text(" end."),
            ])
            .length(5);
    }

    #[test]
    fn reference_with_adjacent_emphasis(
        target in general_target_strategy(),
        word in "[a-zA-Z]{2,8}",
    ) {
        let source = format!("See [{target}] and _{word}_ end.\n");
        let content = extract_first_text_line_content(&source);
        InlineAssertion::new(&content, "ref + emphasis")
            .starts_with(&[
                InlineExpectation::plain_text("See "),
                InlineExpectation::reference(ReferenceExpectation::general(
                    TextMatch::Exact(target)
                )),
                InlineExpectation::plain_text(" and "),
                InlineExpectation::emphasis_text(&word),
                InlineExpectation::plain_text(" end."),
            ])
            .length(5);
    }
}
