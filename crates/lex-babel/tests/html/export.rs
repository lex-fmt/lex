//! Export tests for HTML format (Lex → HTML)
//!
//! These tests verify that Lex documents are correctly converted to HTML
//! by checking the resulting HTML structure.

use insta::assert_snapshot;
use lex_babel::format::Format;
use lex_babel::formats::html::{HtmlFormat, HtmlTheme};
use lex_core::lex::transforms::standard::STRING_TO_AST;
use once_cell::sync::Lazy;
use regex::Regex;

/// Helper to convert Lex source to HTML
fn lex_to_html(lex_src: &str, theme: HtmlTheme) -> String {
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let html_format = HtmlFormat::new(theme);
    html_format.serialize(&lex_doc).unwrap()
}

// ============================================================================
// BASIC ELEMENT TESTS
// ============================================================================

#[test]
fn test_paragraph_simple() {
    let lex_src = "This is a simple paragraph.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("<div class=\"lex-document\">"));
    assert!(html.contains("<p class=\"lex-paragraph\">"));
    assert!(html.contains("This is a simple paragraph."));
}

#[test]
fn test_heading_simple() {
    let lex_src = "1. Introduction\n\n    Some content.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<section class=\"lex-session lex-session-2\">"));
    assert!(html.contains("<h2>"));
    assert!(html.contains("Introduction"));
    assert!(html.contains("<p class=\"lex-paragraph\">"));
    assert!(html.contains("Some content."));
}

#[test]
fn test_multiple_heading_levels() {
    let lex_src = "1. Level 1\n\n    1.1. Level 2\n\n        Content here.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<section class=\"lex-session lex-session-2\">"));
    assert!(html.contains("<section class=\"lex-session lex-session-3\">"));
    assert!(html.contains("<h2>"));
    assert!(html.contains("<h3>"));
}

#[test]
fn test_unordered_list() {
    let lex_src = "- Item 1\n- Item 2\n- Item 3\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<ul class=\"lex-list\">"));
    assert!(html.contains("<li class=\"lex-list-item\">"));
    assert!(html.contains("Item 1"));
    assert!(html.contains("Item 2"));
    assert!(html.contains("Item 3"));
}

#[test]
fn test_ordered_list() {
    let lex_src = "1) First item\n2) Second item\n3) Third item\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<ol class=\"lex-list\">"));
    assert!(html.contains("<li class=\"lex-list-item\">"));
    assert!(html.contains("First item"));
    assert!(html.contains("Second item"));
}

#[test]
fn test_bold_text() {
    let lex_src = "This is *bold text* in a paragraph.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<strong>"));
    assert!(html.contains("bold text"));
    assert!(html.contains("</strong>"));
}

#[test]
fn test_italic_text() {
    let lex_src = "This is _italic text_ in a paragraph.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<em>"));
    assert!(html.contains("italic text"));
    assert!(html.contains("</em>"));
}

#[test]
fn test_code_inline() {
    let lex_src = "This is `inline code` in a paragraph.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<code>"));
    assert!(html.contains("inline code"));
    assert!(html.contains("</code>"));
}

#[test]
fn test_code_block() {
    let lex_src =
        "Code Example:\n\n    function hello() {\n        return \"world\";\n    }\n\n:: rust ::\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<pre class=\"lex-verbatim\" data-language=\"rust\">"));
    assert!(html.contains("<code class=\"language-rust\">"));
    assert!(html.contains("function hello()"));
    assert!(html.contains("return \"world\""));
    // highlight.js CDN should be injected
    assert!(html.contains("highlight.min.js"));
    assert!(html.contains("hljs.highlightAll()"));
}

#[test]
fn test_definition_list() {
    let lex_src = "Term 1:\n    Definition 1\n\nTerm 2:\n    Definition 2\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<dl class=\"lex-definition\">"));
    assert!(html.contains("<dt>"));
    assert!(html.contains("<dd>"));
    assert!(html.contains("Term 1"));
    assert!(html.contains("Definition 1"));
}

#[test]
fn test_math_inline() {
    let lex_src = "The formula is #E = mc^2# here.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<span class=\"lex-math\">"));
    assert!(html.contains("$E = mc^2$")); // Still outputs $ in HTML
}

#[test]
fn test_reference() {
    let lex_src = "Visit [example.com] for more info.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<a href=\"example.com\">"));
}

// ============================================================================
// ISSUE B: Citation href Format Tests
// ============================================================================

#[test]
fn test_citation_href_format() {
    let lex_src = "According to [@smith2023], this is correct.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    // Citations should link to #ref-* anchors, not @*
    assert!(
        html.contains("<a href=\"#ref-smith2023\">"),
        "Citation should use #ref-smith2023, not @smith2023"
    );
    assert!(
        !html.contains("<a href=\"@smith2023\">"),
        "Citation should not use @ in href"
    );
}

#[test]
fn test_multiple_citations() {
    let lex_src = "Research from [@jones2020] and [@brown2021] supports this.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<a href=\"#ref-jones2020\">"));
    assert!(html.contains("<a href=\"#ref-brown2021\">"));
}

#[test]
fn test_url_reference_unchanged() {
    let lex_src = "Visit [https://example.com] for details.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    // URLs should remain as-is
    assert!(html.contains("<a href=\"https://example.com\">"));
}

#[test]
fn test_anchor_reference_unchanged() {
    let lex_src = "See [#section-3] above.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    // Anchors should remain as-is
    assert!(html.contains("<a href=\"#section-3\">"));
}

// TODO: Annotations are not yet fully supported in HTML export
// Document-level annotations aren't converted to IR/Events
// #[test]
// fn test_annotation() {
//     let lex_src = ":: note priority=high ::\n    Important paragraph.\n::\n";
//     let html = lex_to_html(lex_src, HtmlTheme::Modern);
//
//     assert!(html.contains("<!-- lex:note"));
//     assert!(html.contains("priority=high"));
//     assert!(html.contains("<!-- /lex:note -->"));
// }

// ============================================================================
// SYNTAX HIGHLIGHTING TESTS
// ============================================================================

#[test]
fn test_highlight_js_injected() {
    let lex_src = "Hello world.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("highlight.min.js"),
        "highlight.js script should be included"
    );
    assert!(
        html.contains("hljs.highlightAll()"),
        "hljs.highlightAll() should be called"
    );
    assert!(
        html.contains("github.min.css"),
        "highlight.js github theme should be linked"
    );
}

#[test]
fn test_code_block_language_class() {
    let lex_src = "Example:\n\n    print(\"hello\")\n\n:: python ::\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("<code class=\"language-python\">"),
        "code should have language-python class"
    );
    assert!(
        html.contains("data-language=\"python\""),
        "pre should keep data-language attribute"
    );
}

#[test]
fn test_code_block_language_alias_js() {
    let lex_src = "Example:\n\n    console.log(\"hello\")\n\n:: js ::\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("<code class=\"language-javascript\">"),
        "js should be normalized to javascript"
    );
}

#[test]
fn test_code_block_language_alias_py() {
    let lex_src = "Example:\n\n    print(\"hello\")\n\n:: py ::\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("<code class=\"language-python\">"),
        "py should be normalized to python"
    );
}

#[test]
fn test_code_block_language_alias_ts() {
    let lex_src = "Example:\n\n    const x: number = 1\n\n:: ts ::\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("<code class=\"language-typescript\">"),
        "ts should be normalized to typescript"
    );
}

#[test]
fn test_code_block_no_language() {
    let lex_src = "Example:\n\n    some code here\n\n:: ::\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    // No language class on code when no language specified
    assert!(
        html.contains("<code>") || html.contains("<code "),
        "code element should exist"
    );
    assert!(
        !html.contains("language-"),
        "no language class when language is unspecified"
    );
}

// ============================================================================
// CSS AND THEMING TESTS
// ============================================================================

#[test]
fn test_css_embedded_modern() {
    let lex_src = "Test document.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<style"));
    assert!(html.contains("Lex HTML Export - Baseline Styles"));
}

#[test]
fn test_css_embedded_fancy_serif() {
    let lex_src = "Test document.\n";
    let html = lex_to_html(lex_src, HtmlTheme::FancySerif);

    assert!(html.contains("<style"));
    assert!(html.contains("Lex HTML Export - Fancy Serif Theme"));
}

#[test]
fn test_viewport_meta_tag() {
    let lex_src = "Mobile test.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<meta name=\"viewport\""));
    assert!(html.contains("width=device-width"));
}

// ============================================================================
// TRIFECTA TESTS - Document Structure
// ============================================================================

fn snapshot_without_styles(html: &str) -> String {
    static STYLE_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new("(?is)<style[^>]*?>.*?</style>").expect("valid regex for stripping style blocks")
    });
    STYLE_REGEX
        .replace_all(html, "<style data-lex-snapshot=\"removed\"></style>")
        .into_owned()
}

#[test]
fn test_trifecta_010_paragraphs_sessions_flat_single() {
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/trifecta/010-paragraphs-sessions-flat-single.lex",
    )
    .expect("trifecta 010 file should exist");

    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    // Verify basic structure
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("<div class=\"lex-document\">"));

    // Snapshot test for full output
    assert_snapshot!(snapshot_without_styles(&html));
}

#[test]
fn test_trifecta_020_paragraphs_sessions_flat_multiple() {
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/trifecta/020-paragraphs-sessions-flat-multiple.lex",
    )
    .expect("trifecta 020 file should exist");

    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    // Verify multiple sessions exist
    assert!(html.contains("<section class=\"lex-session lex-session-2\">"));

    // Snapshot test
    assert_snapshot!(snapshot_without_styles(&html));
}

#[test]
fn test_trifecta_060_nesting() {
    let lex_src = std::fs::read_to_string("../../comms/specs/trifecta/060-trifecta-nesting.lex")
        .expect("trifecta 060 file should exist");

    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    // Verify nested sessions
    assert!(html.contains("<section class=\"lex-session lex-session-2\">"));
    assert!(html.contains("<section class=\"lex-session lex-session-3\">"));

    // Snapshot test
    assert_snapshot!(snapshot_without_styles(&html));
}

// ============================================================================
// DOCUMENT TITLE TESTS
// ============================================================================

#[test]
fn test_document_title_from_lex_document() {
    // Use spec file: document with explicit title
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-01-title-explicit.lex",
    )
    .expect("document-01 spec file should exist");
    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    assert!(html.contains("<title>My Document Title</title>"));
}

#[test]
fn test_document_title_first_paragraph() {
    // Use spec file: first paragraph followed by blank line becomes document title
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-06-title-empty.lex",
    )
    .expect("document-06 spec file should exist");
    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    // First paragraph "Just a paragraph with no title." becomes the document title
    assert!(html.contains("<title>Just a paragraph with no title.</title>"));
}

#[test]
fn test_document_title_session_without_title() {
    // Use spec file: document starts with session (no explicit document title)
    // Session hoisting is not currently implemented, falls back to "Lex Document"
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-05-title-session-hoist.lex",
    )
    .expect("document-05 spec file should exist");
    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    // Document should fallback to default title (session hoisting not implemented)
    assert!(html.contains("<title>Lex Document</title>"));
}

// ============================================================================
// LIST DECORATION STYLE TESTS
// ============================================================================

#[test]
fn test_alphabetical_list_html_type() {
    let lex_src = "a. First item\nb. Second item\nc. Third item\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("<ol") && html.contains("type=\"a\""),
        "Lowercase alpha list should have type=\"a\": {html}"
    );
}

#[test]
fn test_roman_numeral_list_html_type() {
    let lex_src = "I. First item\nII. Second item\nIII. Third item\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("<ol") && html.contains("type=\"I\""),
        "Uppercase roman list should have type=\"I\": {html}"
    );
}

#[test]
fn test_numeric_list_no_type_attr() {
    let lex_src = "1. First item\n2. Second item\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<ol"), "Should be an ordered list");
    assert!(
        !html.contains("type="),
        "Numeric lists should not have a type attribute: {html}"
    );
}

#[test]
fn test_bullet_list_is_ul() {
    let lex_src = "- First item\n- Second item\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(html.contains("<ul"), "Bullet list should use <ul>");
    assert!(!html.contains("<ol"), "Bullet list should not use <ol>");
}

// ============================================================================
// BEYOND-H6 DEEP SESSION TESTS
// ============================================================================

#[test]
fn test_deep_session_beyond_h6_gets_class() {
    // Create a document with 7 levels of session nesting.
    // Doc title = H1, so root session = H2, and 6 levels deep = H8 → clamped to H6.
    // Levels > 6 should get class="lex-level-N" for lossless identification.
    let lex_src = concat!(
        "1. Level One\n\n",
        "    1.1. Level Two\n\n",
        "        1.1.1. Level Three\n\n",
        "            1.1.1.1. Level Four\n\n",
        "                1.1.1.1.1. Level Five\n\n",
        "                    1.1.1.1.1.1. Level Six\n\n",
        "                        Deep content.\n",
    );
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    // Levels 2-6 should use standard h2-h6 without lex-level class
    assert!(html.contains("<h2>"), "Level 1 session should be h2");

    // Level 7 (H7 clamped to H6) should have lex-level-7 class
    // Note: doc title occupies H1, so 6 nested sessions = levels 2..7
    // Level 7 is the first to exceed H6
    assert!(
        html.contains("lex-level-7"),
        "Session at level 7 must have class lex-level-7 for lossless depth: {html}"
    );

    // The section wrapper already has lex-session-N for all levels
    assert!(
        html.contains("lex-session-7"),
        "Section wrapper should have lex-session-7 class"
    );
}

// ============================================================================
// KITCHENSINK TEST
// ============================================================================

#[test]
fn test_kitchensink() {
    let lex_src = std::fs::read_to_string("../../comms/specs/benchmark/010-kitchensink.lex")
        .expect("kitchensink file should exist");

    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    // Verify complete HTML document
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("<html lang=\"en\">"));
    assert!(html.contains("</html>"));

    // Verify all major element types are present
    assert!(html.contains("<p class=\"lex-paragraph\">"));
    assert!(html.contains("<section class=\"lex-session"));
    assert!(html.contains("<ul class=\"lex-list\">"));
    assert!(html.contains("<pre class=\"lex-verbatim\""));
    assert!(html.contains("<strong>"));
    assert!(html.contains("<em>"));
    assert!(html.contains("<code>"));
    assert!(html.contains("<dl class=\"lex-definition\">"));

    // Snapshot test for the complete output
    assert_snapshot!(snapshot_without_styles(&html));
}
