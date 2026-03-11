//! Tests that VerbatimLine spans never exceed source text bounds.
//!
//! Regression test for the panic at lex-lsp server.rs:541 where
//! `text[token.range.span.clone()]` panics with "byte index N is out of bounds".
//! The root cause is `extract_content_line` in verbatim.rs producing spans
//! that exceed `source.len()`.

use lex_core::lex::ast::elements::{ContentItem, Session};
use lex_core::lex::ast::range::Range;
use lex_core::lex::ast::AstNode;
use lex_core::lex::parsing::parse_document;
use lex_core::lex::testing::workspace_path;

// =============================================================================
// Span validation helpers
// =============================================================================

fn assert_span_in_bounds(range: &Range, source: &str, context: &str) {
    assert!(
        range.span.start <= range.span.end,
        "[{context}] span start {} > end {} (source len {})",
        range.span.start,
        range.span.end,
        source.len(),
    );
    assert!(
        range.span.end <= source.len(),
        "[{context}] span end {} exceeds source length {} (start={}, slice would panic)\n\
         Source around end:\n{:?}",
        range.span.end,
        source.len(),
        range.span.start,
        &source[source.len().saturating_sub(40)..],
    );
}

fn check_verbatim_spans_in_session(session: &Session, source: &str, context: &str) {
    for child in session.children.iter() {
        check_verbatim_spans_in_item(child, source, context);
    }
}

fn check_verbatim_spans_in_item(item: &ContentItem, source: &str, context: &str) {
    match item {
        ContentItem::VerbatimBlock(block) => {
            assert_span_in_bounds(block.range(), source, &format!("{context}/VerbatimBlock"));
            if let Some(loc) = &block.subject.location {
                assert_span_in_bounds(loc, source, &format!("{context}/VerbatimBlock.subject"));
            }
            for (i, child) in block.children.iter().enumerate() {
                if let ContentItem::VerbatimLine(line) = child {
                    assert_span_in_bounds(
                        &line.location,
                        source,
                        &format!("{context}/VerbatimLine[{i}]"),
                    );
                    if let Some(loc) = &line.content.location {
                        assert_span_in_bounds(
                            loc,
                            source,
                            &format!("{context}/VerbatimLine[{i}].content"),
                        );
                        // Verify the slice actually works (this is what panics in lex-lsp)
                        let _slice = &source[loc.span.clone()];
                    }
                    // Also verify the outer location slice works
                    let _slice = &source[line.location.span.clone()];
                } else {
                    check_verbatim_spans_in_item(child, source, context);
                }
            }
        }
        ContentItem::Session(s) => check_verbatim_spans_in_session(s, source, context),
        ContentItem::Definition(d) => {
            for child in d.children.iter() {
                check_verbatim_spans_in_item(child, source, context);
            }
        }
        ContentItem::ListItem(li) => {
            for child in li.children.iter() {
                check_verbatim_spans_in_item(child, source, context);
            }
        }
        ContentItem::List(l) => {
            for item in l.items.iter() {
                check_verbatim_spans_in_item(item, source, context);
            }
        }
        ContentItem::Paragraph(p) => {
            for line in &p.lines {
                check_verbatim_spans_in_item(line, source, context);
            }
        }
        _ => {}
    }
}

fn check_source(source: &str, name: &str) {
    let doc = match parse_document(source) {
        Ok(doc) => doc,
        Err(_) => return, // skip unparseable
    };
    check_verbatim_spans_in_session(doc.root_session(), source, name);
}

// =============================================================================
// Fixture tests — parse ALL .lex fixtures and validate VerbatimLine spans
// =============================================================================

#[test]
fn all_benchmark_fixtures_have_valid_verbatim_spans() {
    let fixtures = [
        "comms/specs/benchmark/000-empty.lex",
        "comms/specs/benchmark/010-kitchensink.lex",
        "comms/specs/benchmark/050-lsp-fixture.lex",
        "comms/specs/benchmark/20-ideas-naked.lex",
        "comms/specs/benchmark/30-a-place-for-ideas.lex",
    ];
    for rel in fixtures {
        let path = workspace_path(rel);
        if let Ok(source) = std::fs::read_to_string(&path) {
            check_source(&source, rel);
        }
    }
}

#[test]
fn all_verbatim_element_fixtures_have_valid_spans() {
    let fixtures = [
        "comms/specs/elements/verbatim.docs/verbatim-01-flat-simple-code.lex",
        "comms/specs/elements/verbatim.docs/verbatim-02-flat-with-caption.lex",
        "comms/specs/elements/verbatim.docs/verbatim-03-flat-with-params.lex",
        "comms/specs/elements/verbatim.docs/verbatim-04-flat-marker-form.lex",
        "comms/specs/elements/verbatim.docs/verbatim-05-flat-special-chars.lex",
        "comms/specs/elements/verbatim.docs/verbatim-06-nested-in-definition.lex",
        "comms/specs/elements/verbatim.docs/verbatim-07-nested-in-list.lex",
        "comms/specs/elements/verbatim.docs/verbatim-08-nested-deep.lex",
        "comms/specs/elements/verbatim.docs/verbatim-09-flat-simple-beyong-wall.lex",
        "comms/specs/elements/verbatim.docs/verbatim-10-flat-simple-empty.lex",
        "comms/specs/elements/verbatim.docs/verbatim-11-group-shell.lex",
        "comms/specs/elements/verbatim.docs/verbatim-12-document-simple.lex",
        "comms/specs/elements/verbatim.docs/verbatim-13-group-spades.lex",
        "comms/specs/elements/verbatim.docs/verbatim-14-fullwidth.lex",
        "comms/specs/elements/verbatim.docs/verbatim-15-inflow-leading-blank.lex",
        "comms/specs/elements/verbatim.docs/verbatim-16-fullwidth-nested.lex",
        "comms/specs/elements/verbatim.docs/verbatim-17-fullwidth-leading-blank.lex",
        "comms/specs/elements/verbatim.lex",
    ];
    for rel in fixtures {
        let path = workspace_path(rel);
        if let Ok(source) = std::fs::read_to_string(&path) {
            check_source(&source, rel);
        }
    }
}

#[test]
fn all_trifecta_fixtures_have_valid_verbatim_spans() {
    let fixtures = [
        "comms/specs/trifecta/000-paragraphs.lex",
        "comms/specs/trifecta/010-paragraphs-sessions-flat-single.lex",
        "comms/specs/trifecta/020-paragraphs-sessions-flat-multiple.lex",
        "comms/specs/trifecta/030-paragraphs-sessions-nested-multiple.lex",
        "comms/specs/trifecta/040-lists.lex",
        "comms/specs/trifecta/050-paragraph-lists.lex",
        "comms/specs/trifecta/060-trifecta-nesting.lex",
        "comms/specs/trifecta/070-trifecta-flat-simple.lex",
    ];
    for rel in fixtures {
        let path = workspace_path(rel);
        if let Ok(source) = std::fs::read_to_string(&path) {
            check_source(&source, rel);
        }
    }
}

#[test]
fn all_other_element_fixtures_have_valid_verbatim_spans() {
    let fixtures = [
        "comms/specs/elements/definition.lex",
        "comms/specs/elements/annotation.lex",
        "comms/specs/elements/data.lex",
        "comms/specs/elements/document.lex",
        "comms/specs/elements/escaping.lex",
        "comms/specs/elements/footnotes.lex",
        "comms/specs/elements/inlines.lex",
        "comms/specs/elements/label.lex",
        "comms/specs/elements/list.lex",
        "comms/specs/elements/paragraph.lex",
        "comms/specs/elements/parameter.lex",
        "comms/specs/elements/XXX-document-simple.lex",
        "comms/specs/elements/XXX-document-tricky.lex",
        "comms/specs/general.lex",
        "comms/specs/grammar-core.lex",
        "comms/specs/grammar-inline.lex",
        "comms/specs/grammar-line.lex",
    ];
    for rel in fixtures {
        let path = workspace_path(rel);
        if let Ok(source) = std::fs::read_to_string(&path) {
            check_source(&source, rel);
        }
    }
}

// =============================================================================
// Synthetic tests — crafted documents targeting the rfind('\n') bug
// =============================================================================

#[test]
fn verbatim_block_at_document_start_no_preceding_newline() {
    // No newline before first_token_start — unwrap_or(0) path
    let source = "Code:\n    x = 1\n    y = 2\n:: python ::";
    check_source(source, "doc-start-no-newline");
}

#[test]
fn verbatim_block_after_single_newline() {
    let source = "\nCode:\n    x = 1\n:: python ::";
    check_source(source, "after-single-newline");
}

#[test]
fn verbatim_block_with_blank_lines_in_content() {
    let source = "\
Code:
    line one

    line three

    line five
:: text ::";
    check_source(source, "blank-lines-in-content");
}

#[test]
fn verbatim_block_nested_in_definition() {
    let source = "\
Outer:
    Code:
        x = 1
        y = 2
    :: python ::";
    check_source(source, "nested-in-definition");
}

#[test]
fn verbatim_block_nested_in_session() {
    let source = "\
Title

    Code:
        x = 1
        y = 2
    :: python ::";
    check_source(source, "nested-in-session");
}

#[test]
fn verbatim_block_deeply_nested() {
    let source = "\
Section

    Category:
        Language:
            Code:
                fn main() {
                    println!(\"hello\");
                }
            :: rust ::";
    check_source(source, "deeply-nested");
}

#[test]
fn verbatim_block_with_trailing_newline() {
    let source = "Code:\n    x = 1\n:: text ::\n";
    check_source(source, "trailing-newline");
}

#[test]
fn verbatim_block_with_no_trailing_newline() {
    let source = "Code:\n    x = 1\n:: text ::";
    check_source(source, "no-trailing-newline");
}

#[test]
fn verbatim_block_with_many_trailing_newlines() {
    let source = "Code:\n    x = 1\n:: text ::\n\n\n";
    check_source(source, "many-trailing-newlines");
}

#[test]
fn multiple_verbatim_blocks_sequential() {
    let source = "\
First:
    a = 1
:: python ::

Second:
    b = 2
:: python ::

Third:
    c = 3
:: python ::";
    check_source(source, "multiple-sequential");
}

#[test]
fn verbatim_block_large_content() {
    // 500+ byte document
    let mut lines = Vec::new();
    lines.push("BigCode:".to_string());
    for i in 0..50 {
        lines.push(format!(
            "    line_{i} = \"value_{i}\" + some_more_padding_text"
        ));
    }
    lines.push(":: python ::".to_string());
    let source = lines.join("\n");
    assert!(source.len() > 500, "test source should be 500+ bytes");
    check_source(&source, "large-content");
}

#[test]
fn verbatim_block_large_nested() {
    // 500+ byte nested document
    let mut lines = vec![
        "Documentation".to_string(),
        "".to_string(),
        "    Examples:".to_string(),
        "        BigCode:".to_string(),
    ];
    for i in 0..40 {
        lines.push(format!(
            "            line_{i} = \"value_{i}\" + padding_text_here"
        ));
    }
    lines.push("        :: python ::".to_string());
    let source = lines.join("\n");
    assert!(source.len() > 500, "test source should be 500+ bytes");
    check_source(&source, "large-nested");
}

#[test]
fn verbatim_block_with_only_blank_content_lines() {
    let source = "\
Code:

:: text ::";
    check_source(source, "only-blank-content");
}

#[test]
fn verbatim_block_single_content_line() {
    let source = "Code:\n    x\n:: text ::";
    check_source(source, "single-content-line");
}

#[test]
fn verbatim_block_content_with_deep_indentation() {
    let source = "\
Code:
    a
        b
            c
                d
                    e
                        f
:: text ::";
    check_source(source, "deep-indentation");
}

#[test]
fn verbatim_block_nested_in_list() {
    let source = "\
- Item one
    Code:
        x = 1
    :: python ::
- Item two";
    check_source(source, "nested-in-list");
}

#[test]
fn verbatim_group_multiple_subjects() {
    let source = "\
Input:
    x = 1
Output:
    y = 2
:: python ::";
    check_source(source, "verbatim-group");
}

#[test]
fn verbatim_block_preceded_by_long_paragraph() {
    // Ensure spans are correct when verbatim is far into the document
    let mut lines: Vec<String> = Vec::new();
    for i in 0..20 {
        lines.push(format!(
            "This is paragraph line number {i} with enough text to be realistic."
        ));
    }
    lines.push("".to_string());
    lines.push("Code:".to_string());
    lines.push("    some_code = True".to_string());
    lines.push(":: python ::".to_string());
    let source = lines.join("\n");
    assert!(source.len() > 500);
    check_source(&source, "preceded-by-long-paragraph");
}

#[test]
fn verbatim_block_unicode_content() {
    let source = "\
Code:
    let emoji = \"hello world\";
    let cjk = \"Chinese Japanese Korean\";
    let accents = \"cafe resume naive\";
:: text ::";
    check_source(source, "unicode-content");
}

#[test]
fn verbatim_block_content_looks_like_subjects() {
    let source = "\
Code:
    def hello():
        pass
    class Foo:
        x = 1
    Something:
        nested
:: python ::";
    check_source(source, "content-looks-like-subjects");
}

#[test]
fn verbatim_block_crlf_line_endings() {
    let source = "Code:\r\n    x = 1\r\n    y = 2\r\n:: python ::\r\n";
    check_source(source, "crlf-line-endings");
}

#[test]
fn verbatim_block_mixed_line_endings() {
    let source = "Code:\n    x = 1\r\n    y = 2\n:: python ::";
    check_source(source, "mixed-line-endings");
}

#[test]
fn verbatim_block_nested_crlf() {
    let source = "Section\r\n\r\n    Code:\r\n        x = 1\r\n    :: python ::\r\n";
    check_source(source, "nested-crlf");
}

#[test]
fn verbatim_block_at_exact_source_end() {
    // Source ends exactly at the last byte of the closing annotation
    let source = "Code:\n    x = 1\n:: text ::";
    assert!(!source.ends_with('\n'));
    check_source(source, "exact-source-end");
}

#[test]
fn verbatim_block_single_char_content() {
    let source = "C:\n    x\n:: t ::";
    check_source(source, "single-char-content");
}

#[test]
fn verbatim_deeply_nested_large_document() {
    // Stress test: 3-level nesting with 500+ bytes
    let mut lines = vec![
        "Top Level Section".to_string(),
        "".to_string(),
        "    Mid Level Section".to_string(),
        "".to_string(),
        "        Container:".to_string(),
        "            Code:".to_string(),
    ];
    for i in 0..30 {
        lines.push(format!(
            "                line_{i} = value_{i} + extra_padding_for_length"
        ));
    }
    lines.push("            :: python ::".to_string());
    let source = lines.join("\n");
    assert!(source.len() > 500);
    check_source(&source, "deeply-nested-large");
}

#[test]
fn verbatim_multiple_blocks_in_same_session() {
    let source = "\
Section

    First:
        a = 1
    :: python ::

    Second:
        b = 2
    :: python ::

    Third:
        c = 3
    :: python ::";
    check_source(source, "multiple-in-session");
}

#[test]
fn verbatim_block_after_definition_in_session() {
    let source = "\
Section

    Term:
        A definition.

    Code:
        x = 1
    :: python ::";
    check_source(source, "after-definition-in-session");
}

#[test]
fn verbatim_block_before_and_after_lists() {
    let source = "\
Code:
    x = 1
:: python ::

- Item one
- Item two

More Code:
    y = 2
:: python ::";
    check_source(source, "before-and-after-lists");
}
