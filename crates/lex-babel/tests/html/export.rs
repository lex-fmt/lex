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
fn test_document_untitled_marker_is_title_less() {
    // document-06 leads with `:: doc.untitled ::`, the ADR-0002 no-title marker
    // (#783): the parser honors it, suppressing title promotion, so the first
    // paragraph stays body and no document title is emitted. HTML falls back to
    // the default `<title>`; the first paragraph renders as a `<p>`, not a heading.
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-06-title-untitled.lex",
    )
    .expect("document-06 spec file should exist");
    let html = lex_to_html(&lex_src, HtmlTheme::Modern);
    assert!(
        !html.contains("<title>Just a paragraph with no title.</title>"),
        "the first paragraph must not become the document title; got: {html}"
    );
    assert!(
        html.contains("<p class=\"lex-paragraph\">Just a paragraph with no title.</p>"),
        "the first paragraph must render as body, got: {html}"
    );
}

#[test]
fn test_document_title_session_without_title() {
    // Use spec file: document starts with session (no explicit document title)
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-05-title-session-none.lex",
    )
    .expect("document-05 spec file should exist");
    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    // Document should fallback to default title (no document title)
    assert!(html.contains("<title>Lex Document</title>"));
}

#[test]
fn test_document_title_rendered_in_body() {
    // Regression test for #601: title was set in <head><title> only,
    // body had no <h1> for the document title.
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-01-title-explicit.lex",
    )
    .expect("document-01 spec file should exist");
    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("<header class=\"lex-doc-header\">"),
        "body should include a doc-header element: {html}"
    );
    assert!(
        html.contains("<h1 class=\"lex-doc-title\">My Document Title</h1>"),
        "body should include an h1 with the title: {html}"
    );
}

#[test]
fn test_document_title_no_body_header_when_no_title() {
    // Documents without a title shouldn't emit an empty <header>.
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-05-title-session-none.lex",
    )
    .expect("document-05 spec file should exist");
    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    assert!(
        !html.contains("lex-doc-header"),
        "no doc-header should be present when document has no title: {html}"
    );
}

#[test]
fn test_katex_injected_when_math_present() {
    // Regression test for #602: math spans previously rendered as raw
    // `$...$` text because no math renderer was loaded.
    let lex_src = "1. Math\n\n    Inline: #\\log_2# present.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("katex.min.js"),
        "KaTeX script should be loaded when math is present: {html}"
    );
    assert!(
        html.contains("auto-render.min.js"),
        "KaTeX auto-render should be loaded: {html}"
    );
    assert!(
        html.contains("renderMathInElement"),
        "KaTeX auto-render onload trigger expected: {html}"
    );
}

#[test]
fn test_highlight_js_tags_have_sri_integrity_hashes() {
    // Follow-up to #611 (Copilot review): every CDN-loaded third-party
    // asset should carry an SRI hash, not just KaTeX. Hashes are for
    // highlight.js 11.11.1 and were computed from the official cdn-release
    // tarball at github.com/highlightjs/cdn-release/releases/tag/11.11.1.
    let lex_src = "Plain document for the highlight.js tag check.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    // styles/github.min.css
    assert!(
        html.contains(
            "integrity=\"sha384-eFTL69TLRZTkNfYZOLM+G04821K1qZao/4QLJbet1pP4tcF+fdXq/9CdqAbWRl/L\""
        ),
        "highlight.js github.min.css must carry verified SRI hash: {html}"
    );
    // highlight.min.js
    assert!(
        html.contains(
            "integrity=\"sha384-RH2xi4eIQ/gjtbs9fUXM68sLSi99C7ZWBRX1vDrVv6GQXRibxXLbwO2NGZB74MbU\""
        ),
        "highlight.min.js must carry verified SRI hash: {html}"
    );
}

#[test]
fn test_katex_tags_have_sri_integrity_hashes() {
    // Regression test for #611: CDN-loaded KaTeX assets must carry
    // Subresource Integrity (SRI) hashes so a compromised CDN cannot
    // substitute malicious bytes. Hashes are for KaTeX 0.16.11 and were
    // computed from the actual release tarball at
    // github.com/KaTeX/KaTeX/releases/tag/v0.16.11.
    let lex_src = "1. Math\n\n    Inline: #\\log_2# present.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    // katex.min.css
    assert!(
        html.contains(
            "integrity=\"sha384-nB0miv6/jRmo5UMMR1wu3Gz6NLsoTkbqJghGIsx//Rlm+ZU03BU6SQNC66uf4l5+\""
        ),
        "katex.min.css must carry verified SRI hash: {html}"
    );
    // katex.min.js
    assert!(
        html.contains(
            "integrity=\"sha384-7zkQWkzuo3B5mTepMUcHkMB5jZaolc2xDwL6VFqjFALcbeS9Ggm/Yr2r3Dy4lfFg\""
        ),
        "katex.min.js must carry verified SRI hash: {html}"
    );
    // contrib/auto-render.min.js
    assert!(
        html.contains(
            "integrity=\"sha384-43gviWU0YVjaDtb/GhzOouOXtZMP/7XUzwPTstBeZFe/+rCMvRwr4yROQP43s0Xk\""
        ),
        "auto-render.min.js must carry verified SRI hash: {html}"
    );
}

#[test]
fn test_katex_not_injected_when_no_math() {
    // Math-free documents shouldn't pay the KaTeX cost.
    let lex_src = "Just a paragraph with no math at all.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        !html.contains("katex"),
        "KaTeX must not be loaded for math-free documents: {html}"
    );
}

#[test]
fn test_katex_not_injected_for_verbatim_containing_math_class_text() {
    // Regression test for a false-positive flagged in PR review: when math
    // detection was a substring scan over the serialized HTML, a verbatim
    // block whose author-written text happened to include `class="lex-math"`
    // would falsely trigger KaTeX injection. Math is now tracked during DOM
    // construction, so the substring is irrelevant.
    let lex_src = concat!(
        "1. Verbatim\n\n",
        "    Here's an HTML sample.\n\n",
        "    ===\n",
        "    <span class=\"lex-math\">$x$</span>\n",
        "    ===\n",
    );
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        !html.contains("katex.min.js"),
        "KaTeX must not load when the only `lex-math` occurrence is inside verbatim text: {html}"
    );
}

#[test]
fn test_consecutive_definitions_share_one_dl() {
    // Regression test for #603: each `Definition` node used to emit its own
    // `<dl>`. Consecutive sibling Definitions should now share one `<dl>`.
    let lex_src = concat!(
        "term-a:\n",
        "    def a\n\n",
        "term-b:\n",
        "    def b\n\n",
        "term-c:\n",
        "    def c\n",
    );
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    let dl_count = html.matches("<dl class=\"lex-definition\">").count();
    let dt_count = html.matches("<dt>").count();
    assert_eq!(
        dl_count, 1,
        "three consecutive defs should share one <dl>, got {dl_count}: {html}"
    );
    assert_eq!(dt_count, 3, "three terms expected, got {dt_count}");
}

#[test]
fn test_non_definition_between_definitions_breaks_dl_grouping() {
    // A paragraph between two Definition groups should close the first
    // `<dl>` and open a new one for the second group.
    let lex_src = concat!(
        "term-a:\n",
        "    def a\n\n",
        "Just a paragraph between def groups.\n\n",
        "term-b:\n",
        "    def b\n",
    );
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    let dl_count = html.matches("<dl class=\"lex-definition\">").count();
    assert_eq!(
        dl_count, 2,
        "a paragraph between def groups should yield two <dl>s, got {dl_count}: {html}"
    );
}

#[test]
fn test_section_body_emits_no_redundant_content_wrapper() {
    // Regression test for #610 (option 1): after a heading, the
    // semantic content area is the `<section>` element itself. Wrapping
    // its body in a further `<div class="lex-content">` is DOM bloat —
    // two stacked containers carrying the same "this is content" cue.
    // The fix suppresses `StartContent` when the immediate parent is a
    // `<section>`, mirroring the existing skip for `<dd>` (#604).
    let lex_src = concat!(
        "1. Primary Session\n\n",
        "    A simple paragraph inside the session.\n",
    );
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        !html.contains("<section class=\"lex-session lex-session-2\"><h2>Primary Session</h2><div class=\"lex-content\">"),
        "section body should not open with <div class=\"lex-content\">: {html}"
    );
    // The paragraph should be a direct child of the section.
    assert!(
        html.contains("</h2><p class=\"lex-paragraph\">A simple paragraph inside the session.</p>"),
        "section body should emit <p> directly under the heading: {html}"
    );
}

#[test]
fn test_nested_sections_share_no_redundant_wrappers() {
    // A nested-session document collapses two layers of wrappers — the
    // outer section's body wrapper AND the inner section's body wrapper —
    // both of which are redundant under #610.
    let lex_src = concat!(
        "1. Outer\n\n",
        "    Outer paragraph.\n\n",
        "    1.1. Inner\n\n",
        "        Inner paragraph.\n",
    );
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    // Zero `<div class="lex-content">` wrappers should appear in the
    // rendered body: the body of every session is now the section
    // element itself. We scope the match to the post-`<body>` segment
    // so the literal CSS-comment string `<div class="lex-content">`
    // embedded in baseline.css doesn't false-positive.
    let body_start = html.find("<body>").expect("body tag present");
    let body = &html[body_start..];
    let wrapper_count = body.matches("<div class=\"lex-content\">").count();
    assert_eq!(
        wrapper_count, 0,
        "expected zero .lex-content wrappers in nested sessions, got {wrapper_count}: {body}"
    );
}

#[test]
fn test_dd_body_emits_p_directly_without_content_wrapper() {
    // Regression test for #604: `<dd>` body used to wrap its content in
    // `<div class="lex-content"><p class="lex-paragraph">…</p></div>`. The
    // inner div adds no semantic value inside `<dd>` (the dd is already the
    // content container). The fix skips the wrapper so a simple definition
    // body emits as `<dd><p>…</p></dd>`.
    let lex_src = "term:\n    A simple definition body.\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("<dd><p>A simple definition body.</p></dd>"),
        "dd should emit <p> directly, no inner div.lex-content: {html}"
    );
    assert!(
        !html.contains("<dd><div class=\"lex-content\">"),
        "dd should not wrap its content in <div class=\"lex-content\">: {html}"
    );
}

#[test]
fn test_dd_body_with_multiple_blocks_no_content_wrapper() {
    // A multi-block `<dd>` body (paragraph + list) still renders correctly
    // without the `<div class="lex-content">` wrapper around the dd contents.
    let lex_src = concat!(
        "term:\n",
        "    First paragraph in definition.\n\n",
        "    - item one\n",
        "    - item two\n",
    );
    let html = lex_to_html(lex_src, HtmlTheme::Modern);

    assert!(
        !html.contains("<dd><div class=\"lex-content\">"),
        "dd should not open with <div class=\"lex-content\">: {html}"
    );
    // The paragraph and list should be direct children of `<dd>`.
    assert!(
        html.contains("<dd><p>First paragraph in definition.</p>"),
        "first paragraph expected as direct child of dd: {html}"
    );
    assert!(
        html.contains("<ul class=\"lex-list\">"),
        "list still present: {html}"
    );
}

#[test]
fn test_document_subtitle_rendered_in_body() {
    // Title-with-subtitle uses a trailing colon on the title line + subtitle below.
    let lex_src = std::fs::read_to_string(
        "../../comms/specs/elements/document.docs/document-07-title-with-subtitle.lex",
    )
    .expect("document-07 spec file should exist");
    let html = lex_to_html(&lex_src, HtmlTheme::Modern);

    assert!(
        html.contains("<h1 class=\"lex-doc-title\">The Art of War</h1>"),
        "title h1 expected: {html}"
    );
    assert!(
        html.contains("<p class=\"lex-doc-subtitle\">A New Translation</p>"),
        "subtitle paragraph expected: {html}"
    );
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

// ============================================================================
// REFERENCE ANCHORING (references-general.lex §2.3) — PR B
// ============================================================================

#[test]
fn test_inline_word_anchor_preceding() {
    // §2.3.1: an inline reference anchors the preceding word; the bracketed
    // reference itself renders nothing, and the link wraps that word.
    let lex_src = "Body intro.\n\nthe project website [https://lex.ing] today\n\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);
    assert!(
        html.contains(r#"the project <a href="https://lex.ing">website</a> today"#),
        "preceding word anchor should wrap 'website', got: {html}"
    );
    assert!(
        !html.contains("[https://lex.ing]"),
        "the bracketed reference must not render as literal text"
    );
}

#[test]
fn test_inline_word_anchor_following() {
    // §2.3.1: a reference first on the line anchors the following word.
    let lex_src = "Body intro.\n\n[https://lex.ing] is the home page.\n\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);
    assert!(
        html.contains(r#"<a href="https://lex.ing">is</a> the home page."#),
        "following word anchor should wrap 'is', got: {html}"
    );
}

#[test]
fn test_whole_element_anchor_session_title() {
    // §2.3.2: a reference line below a session title anchors the whole title.
    let lex_src = "Getting Started\n[./readme.txt]\n\n    Welcome to the docs.\n\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);
    assert!(
        html.contains(r#"<h2><a href="./readme.txt">Getting Started</a></h2>"#),
        "session title should be wrapped in the link, got: {html}"
    );
}

#[test]
fn test_whole_element_anchor_list_item() {
    // §2.3.2: a reference line anchors the single list item above it.
    let lex_src = "Intro.\n\n- Food\n- Water\n[https://water.example]\n- Bread\n\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);
    assert!(
        html.contains(
            r#"<li class="lex-list-item"><a href="https://water.example">Water</a></li>"#
        ),
        "list item 'Water' should be wrapped, got: {html}"
    );
    // Sibling items are untouched.
    assert!(html.contains("Food") && html.contains("Bread"));
}

#[test]
fn test_whole_element_anchor_definition_term() {
    // §2.3.2: anchors the definition term (trailing colon excluded).
    let lex_src = "API Endpoint:\n[./endpoint.txt]\n    A URL that provides access.\n\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);
    assert!(
        html.contains(r#"<dt><a href="./endpoint.txt">API Endpoint</a></dt>"#),
        "definition term should be wrapped (no trailing colon), got: {html}"
    );
}

#[test]
fn test_whole_element_anchor_verbatim_subject() {
    // §2.3.2: anchors the verbatim subject (rendered as a linked caption).
    let lex_src = "Example Source:\n[./example.rs]\n    fn main() {}\n:: rust ::\n\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);
    assert!(
        html.contains(
            r#"<div class="lex-verbatim-subject"><a href="./example.rs">Example Source</a></div>"#
        ),
        "verbatim subject caption should wrap a link, got: {html}"
    );
}

#[test]
fn test_self_link_reference_line() {
    // §2.3.2: a reference line with a blank line above stands alone and links
    // its own text.
    let lex_src = "See the upstream project:\n\n[https://github.com/lex-fmt/lex]\n\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);
    assert!(
        html.contains(
            r#"<a href="https://github.com/lex-fmt/lex">https://github.com/lex-fmt/lex</a>"#
        ),
        "self-link should render as a standalone link of its own text, got: {html}"
    );
}

#[test]
fn test_marker_reference_line_not_anchored() {
    // §2.3.4: a marker-style annotation reference on its own line is NOT a
    // whole-element anchor; it stays an inline reference and resolves as usual.
    let lex_src = "Closing remarks.\n[::summary-note]\n\n:: summary-note ::\n    Resolved.\n\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);
    // The remarks paragraph is not wrapped in a link to the annotation ref.
    assert!(
        !html.contains(r#"<a href="::summary-note">Closing remarks</a>"#),
        "marker-style reference must not anchor the line above, got: {html}"
    );
}

#[test]
fn test_marker_references_unchanged() {
    // §2.3.4: footnotes / citations / annotation refs render as markers and are
    // not given word anchors even when adjacent to text.
    let lex_src = "Body.\n\nSee [42] and [@smith2023] later.\n\n";
    let html = lex_to_html(lex_src, HtmlTheme::Modern);
    // Footnote keeps its own text as the anchor (not the preceding word).
    assert!(
        html.contains(r#"<a href="42">42</a>"#),
        "footnote marker should be unchanged, got: {html}"
    );
    // Citation still anchors on #ref-key with its literal as text.
    assert!(
        html.contains(r##"<a href="#ref-smith2023">@smith2023</a>"##),
        "citation marker should be unchanged, got: {html}"
    );
    // No word anchor stole "See" or "and".
    assert!(!html.contains(r#"<a href="42">See</a>"#));
    assert!(!html.contains(r##"<a href="#ref-smith2023">and</a>"##));
}
