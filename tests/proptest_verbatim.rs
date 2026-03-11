//! Property-based tests for verbatim block parsing
//!
//! These tests verify that verbatim block content is preserved exactly through
//! the parse pipeline, with special attention to:
//!
//! - Internal indentation within content lines
//! - Content that looks like Lex syntax (colons, ::, subjects, annotations)
//! - Verbatim blocks nested inside sessions, definitions, lists
//! - Multi-level nesting (verbatim inside definition inside session, etc.)
//! - Fullwidth mode
//! - Verbatim groups (multiple subject/content pairs)

use lex_core::lex::ast::elements::ContentItem;
use lex_core::lex::ast::range::Range;
use lex_core::lex::ast::traits::Visitor;
use lex_core::lex::parsing::parse_document;
use proptest::prelude::*;

// =============================================================================
// Source Text Generators
// =============================================================================

/// Indent a block of text by `level` indentation steps (4 spaces each).
fn indent(text: &str, level: usize) -> String {
    let prefix = "    ".repeat(level);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate a verbatim block as Lex source text.
/// Returns (source_text, expected_subject, expected_content_lines).
fn verbatim_source(subject: &str, label: &str, content_lines: &[&str]) -> (String, Vec<String>) {
    let mut lines = Vec::new();
    lines.push(format!("{subject}:"));
    for cl in content_lines {
        lines.push(format!("    {cl}"));
    }
    lines.push(format!(":: {label} ::"));
    let expected: Vec<String> = content_lines.iter().map(|s| s.to_string()).collect();
    (lines.join("\n"), expected)
}

// =============================================================================
// Content Extraction Helpers
// =============================================================================

/// Extract all verbatim line content strings from a parsed document,
/// for the first verbatim block found (searching recursively).
fn extract_verbatim_lines(items: &[&ContentItem]) -> Option<(String, Vec<String>)> {
    for item in items {
        match item {
            ContentItem::VerbatimBlock(vb) => {
                let subject = vb.subject.as_string().to_string();
                let lines: Vec<String> = vb
                    .children
                    .iter()
                    .filter_map(|c| {
                        if let ContentItem::VerbatimLine(vl) = c {
                            Some(vl.content.as_string().to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
                return Some((subject, lines));
            }
            ContentItem::Session(s) => {
                let children: Vec<&ContentItem> = s.children.iter().collect();
                if let Some(result) = extract_verbatim_lines(&children) {
                    return Some(result);
                }
            }
            ContentItem::Definition(d) => {
                let children: Vec<&ContentItem> = d.children.iter().collect();
                if let Some(result) = extract_verbatim_lines(&children) {
                    return Some(result);
                }
            }
            ContentItem::ListItem(li) => {
                let children: Vec<&ContentItem> = li.children.iter().collect();
                if let Some(result) = extract_verbatim_lines(&children) {
                    return Some(result);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse source and extract verbatim content. Panics with diagnostics on failure.
fn parse_and_extract(source: &str) -> (String, Vec<String>) {
    let doc = parse_document(source).unwrap_or_else(|e| {
        panic!("Parse failed for:\n---\n{source}\n---\nError: {e}");
    });
    let items: Vec<&ContentItem> = doc.root.children.iter().collect();
    extract_verbatim_lines(&items).unwrap_or_else(|| {
        panic!("No verbatim block found in:\n---\n{source}\n---");
    })
}

// =============================================================================
// Content Strategies
// =============================================================================

/// A verbatim content line: plain text, no leading/trailing whitespace concerns
/// at this level (indentation is added by the source builder).
fn plain_content_line() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9_. ]{0,30}".prop_map(|s| s.trim_end().to_string())
}

/// A content line with internal indentation (the key thing we're testing).
fn indented_content_line() -> impl Strategy<Value = String> {
    (1..4usize, plain_content_line())
        .prop_map(|(spaces, text)| format!("{}{text}", "    ".repeat(spaces)))
}

/// Content that looks like a Lex subject (ends with colon).
fn subject_like_content() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9 ]{0,20}".prop_map(|s| format!("{}:", s.trim_end()))
}

/// Content that looks like a Lex annotation marker.
fn annotation_like_content() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9]{0,8}".prop_map(|label| format!(":: {label} ::"))
}

/// Content that looks like a list item.
fn list_like_content() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9 ]{0,20}".prop_map(|s| format!("- {}", s.trim_end()))
}

/// Content that mixes indentation patterns — simulates real code.
fn code_like_content() -> impl Strategy<Value = Vec<String>> {
    (
        plain_content_line(),    // e.g., "def foo():"
        indented_content_line(), // e.g., "    return bar"
        indented_content_line(), // e.g., "    x = 1"
        plain_content_line(),    // e.g., "def baz():"
        indented_content_line(), // e.g., "    pass"
    )
        .prop_map(|(a, b, c, d, e)| vec![a, b, c, d, e])
}

/// Any kind of tricky content line.
fn any_content_line() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => plain_content_line(),
        3 => indented_content_line(),
        1 => subject_like_content(),
        1 => annotation_like_content(),
        1 => list_like_content(),
    ]
}

/// A vector of mixed content lines.
fn mixed_content_lines() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(any_content_line(), 1..8)
}

/// A safe subject (no leading/trailing whitespace, no trailing colon — we add that).
fn subject_strategy() -> impl Strategy<Value = String> {
    "[A-Z][a-zA-Z0-9 ]{0,15}".prop_map(|s| s.trim_end().to_string())
}

/// A safe label for the closing annotation.
fn label_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{0,6}".prop_map(|s| s.to_string())
}

// =============================================================================
// Nesting Wrappers
// =============================================================================

/// Wrap content in a session at the given indent level.
fn wrap_in_session(inner: &str, title: &str, level: usize) -> String {
    let indented_inner = indent(inner, 1);
    let session = format!("{title}\n\n{indented_inner}");
    indent(&session, level)
}

/// Wrap content in a definition at the given indent level.
fn wrap_in_definition(inner: &str, subject: &str, level: usize) -> String {
    let indented_inner = indent(inner, 1);
    let def = format!("{subject}:\n{indented_inner}");
    indent(&def, level)
}

// =============================================================================
// Property Tests: Flat Verbatim Blocks
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Verbatim content lines are preserved exactly through parse.
    #[test]
    fn flat_verbatim_preserves_content(
        subject in subject_strategy(),
        label in label_strategy(),
        content in mixed_content_lines(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (source, expected) = verbatim_source(&subject, &label, &content_refs);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }

    /// Code-like content with internal indentation is preserved.
    #[test]
    fn flat_verbatim_preserves_code_indentation(
        subject in subject_strategy(),
        label in label_strategy(),
        content in code_like_content(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (source, expected) = verbatim_source(&subject, &label, &content_refs);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }
}

// =============================================================================
// Property Tests: Nested Verbatim Blocks
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Verbatim inside a session preserves content.
    #[test]
    fn verbatim_in_session_preserves_content(
        session_title in "[A-Z][a-zA-Z0-9]{0,10}",
        subject in subject_strategy(),
        label in label_strategy(),
        content in mixed_content_lines(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, expected) = verbatim_source(&subject, &label, &content_refs);
        let source = wrap_in_session(&verb_source, &session_title, 0);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }

    /// Verbatim inside a definition preserves content.
    #[test]
    fn verbatim_in_definition_preserves_content(
        def_subject in "[A-Z][a-zA-Z0-9 ]{0,10}".prop_map(|s| s.trim_end().to_string()),
        subject in subject_strategy(),
        label in label_strategy(),
        content in mixed_content_lines(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, expected) = verbatim_source(&subject, &label, &content_refs);
        let source = wrap_in_definition(&verb_source, &def_subject, 0);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }

    /// Verbatim inside session > definition (2 levels of nesting).
    #[test]
    fn verbatim_in_session_definition(
        session_title in "[A-Z][a-zA-Z0-9]{0,10}",
        def_subject in "[A-Z][a-zA-Z0-9 ]{0,10}".prop_map(|s| s.trim_end().to_string()),
        subject in subject_strategy(),
        label in label_strategy(),
        content in mixed_content_lines(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, expected) = verbatim_source(&subject, &label, &content_refs);
        let def_source = wrap_in_definition(&verb_source, &def_subject, 0);
        let source = wrap_in_session(&def_source, &session_title, 0);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }

    /// Verbatim inside session > session > definition (3 levels of nesting).
    #[test]
    fn verbatim_in_deep_nesting(
        s1_title in "[A-Z][a-zA-Z0-9]{0,8}",
        s2_title in "[A-Z][a-zA-Z0-9]{0,8}",
        def_subject in "[A-Z][a-zA-Z0-9 ]{0,8}".prop_map(|s| s.trim_end().to_string()),
        subject in subject_strategy(),
        label in label_strategy(),
        content in code_like_content(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, expected) = verbatim_source(&subject, &label, &content_refs);
        let def_source = wrap_in_definition(&verb_source, &def_subject, 0);
        let s2_source = wrap_in_session(&def_source, &s2_title, 0);
        let source = wrap_in_session(&s2_source, &s1_title, 0);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }

    /// Verbatim inside definition > definition (nested definitions).
    #[test]
    fn verbatim_in_nested_definitions(
        d1_subject in "[A-Z][a-zA-Z0-9 ]{0,8}".prop_map(|s| s.trim_end().to_string()),
        d2_subject in "[A-Z][a-zA-Z0-9 ]{0,8}".prop_map(|s| s.trim_end().to_string()),
        subject in subject_strategy(),
        label in label_strategy(),
        content in code_like_content(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, expected) = verbatim_source(&subject, &label, &content_refs);
        let d2_source = wrap_in_definition(&verb_source, &d2_subject, 0);
        let source = wrap_in_definition(&d2_source, &d1_subject, 0);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }
}

// =============================================================================
// Property Tests: Content Robustness
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Content lines that look like subjects (ending with `:`) are preserved.
    #[test]
    fn content_with_colons_preserved(
        subject in subject_strategy(),
        label in label_strategy(),
        n_plain in 1..3usize,
        colon_line in subject_like_content(),
        n_after in 1..3usize,
    ) {
        let mut content: Vec<String> = Vec::new();
        for i in 0..n_plain {
            content.push(format!("line{i}"));
        }
        content.push(colon_line);
        for i in 0..n_after {
            content.push(format!("after{i}"));
        }
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (source, expected) = verbatim_source(&subject, &label, &content_refs);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }

    /// Content that looks like annotation markers is preserved.
    #[test]
    fn content_with_annotation_markers_preserved(
        subject in subject_strategy(),
        label in label_strategy(),
        fake_anno in annotation_like_content(),
    ) {
        let content = [
            "before".to_string(),
            fake_anno,
            "after".to_string(),
        ];
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (source, expected) = verbatim_source(&subject, &label, &content_refs);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }

    /// Content that looks like list items is preserved.
    #[test]
    fn content_with_list_markers_preserved(
        subject in subject_strategy(),
        label in label_strategy(),
        list_line in list_like_content(),
    ) {
        let content = [
            "before".to_string(),
            list_line,
            "after".to_string(),
        ];
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (source, expected) = verbatim_source(&subject, &label, &content_refs);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }

    /// Deep nesting with tricky content (the stress test).
    #[test]
    fn deep_nesting_with_tricky_content(
        s_title in "[A-Z][a-zA-Z0-9]{0,6}",
        d_subject in "[A-Z][a-zA-Z0-9]{0,6}",
        subject in subject_strategy(),
        label in label_strategy(),
        content in mixed_content_lines(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, expected) = verbatim_source(&subject, &label, &content_refs);
        let def_source = wrap_in_definition(&verb_source, &d_subject, 0);
        let source = wrap_in_session(&def_source, &s_title, 0);
        let (parsed_subject, parsed_lines) = parse_and_extract(&source);

        assert_eq!(&parsed_subject, &subject, "Subject mismatch.\nSource:\n{source}");
        assert_eq!(&parsed_lines, &expected, "Content mismatch.\nSource:\n{source}");
    }
}

// =============================================================================
// Deterministic edge case tests
// =============================================================================

#[test]
fn verbatim_content_indentation_preserved_at_root() {
    let source = "\
Example:
    def foo():
        return bar
    x = 1
:: python ::";
    let (subject, lines) = parse_and_extract(source);
    assert_eq!(subject, "Example");
    assert_eq!(lines, vec!["def foo():", "    return bar", "x = 1"]);
}

#[test]
fn verbatim_content_indentation_preserved_in_session() {
    let source = "\
Title

    Example:
        def foo():
            return bar
        x = 1
    :: python ::";
    let (subject, lines) = parse_and_extract(source);
    assert_eq!(subject, "Example");
    assert_eq!(lines, vec!["def foo():", "    return bar", "x = 1"]);
}

#[test]
fn verbatim_content_indentation_preserved_in_definition() {
    let source = "\
Outer:
    Example:
        def foo():
            return bar
        x = 1
    :: python ::";
    let (subject, lines) = parse_and_extract(source);
    assert_eq!(subject, "Example");
    assert_eq!(lines, vec!["def foo():", "    return bar", "x = 1"]);
}

#[test]
fn verbatim_content_indentation_preserved_deep_nesting() {
    let source = "\
Section

    Category:
        Language:
            Example:
                def foo():
                    return bar
                x = 1
            :: python ::";
    let (subject, lines) = parse_and_extract(source);
    assert_eq!(subject, "Example");
    assert_eq!(lines, vec!["def foo():", "    return bar", "x = 1"]);
}

#[test]
fn verbatim_content_with_fake_annotation_inside() {
    let source = "\
Example:
    line one
    :: not_an_annotation ::
    line three
:: text ::";
    let (subject, lines) = parse_and_extract(source);
    assert_eq!(subject, "Example");
    assert_eq!(
        lines,
        vec!["line one", ":: not_an_annotation ::", "line three"]
    );
}

#[test]
fn verbatim_content_with_colon_lines_inside() {
    let source = "\
Example:
    def hello():
        pass
    class Foo:
        x = 1
:: python ::";
    let (subject, lines) = parse_and_extract(source);
    assert_eq!(subject, "Example");
    assert_eq!(
        lines,
        vec!["def hello():", "    pass", "class Foo:", "    x = 1"]
    );
}

#[test]
fn verbatim_multiple_indent_levels() {
    let source = "\
Example:
    level0
        level1
            level2
                level3
            back2
        back1
    back0
:: text ::";
    let (subject, lines) = parse_and_extract(source);
    assert_eq!(subject, "Example");
    assert_eq!(
        lines,
        vec![
            "level0",
            "    level1",
            "        level2",
            "            level3",
            "        back2",
            "    back1",
            "back0",
        ]
    );
}

// =============================================================================
// Span Bounds Validation
// =============================================================================

/// A visitor that collects all Range spans from every AST node it encounters.
struct SpanCollector {
    spans: Vec<(String, Range)>,
}

impl SpanCollector {
    fn new() -> Self {
        Self { spans: Vec::new() }
    }

    fn record(&mut self, node_type: &str, range: &Range) {
        self.spans.push((node_type.to_string(), range.clone()));
    }
}

impl Visitor for SpanCollector {
    fn visit_session(&mut self, s: &lex_core::lex::ast::Session) {
        use lex_core::lex::ast::traits::AstNode;
        self.record("Session", s.range());
    }

    fn visit_definition(&mut self, d: &lex_core::lex::ast::Definition) {
        use lex_core::lex::ast::traits::AstNode;
        self.record("Definition", d.range());
    }

    fn visit_list(&mut self, l: &lex_core::lex::ast::List) {
        use lex_core::lex::ast::traits::AstNode;
        self.record("List", l.range());
    }

    fn visit_list_item(&mut self, li: &lex_core::lex::ast::ListItem) {
        use lex_core::lex::ast::traits::AstNode;
        self.record("ListItem", li.range());
    }

    fn visit_paragraph(&mut self, p: &lex_core::lex::ast::Paragraph) {
        use lex_core::lex::ast::traits::AstNode;
        self.record("Paragraph", p.range());
    }

    fn visit_text_line(&mut self, tl: &lex_core::lex::ast::elements::paragraph::TextLine) {
        use lex_core::lex::ast::traits::AstNode;
        self.record("TextLine", tl.range());
    }

    fn visit_verbatim_block(&mut self, vb: &lex_core::lex::ast::Verbatim) {
        use lex_core::lex::ast::traits::AstNode;
        self.record("VerbatimBlock", vb.range());
    }

    fn visit_verbatim_line(&mut self, vl: &lex_core::lex::ast::elements::VerbatimLine) {
        use lex_core::lex::ast::traits::AstNode;
        self.record("VerbatimLine", vl.range());
    }

    fn visit_annotation(&mut self, a: &lex_core::lex::ast::Annotation) {
        use lex_core::lex::ast::traits::AstNode;
        self.record("Annotation", a.range());
    }

    fn visit_blank_line_group(
        &mut self,
        blg: &lex_core::lex::ast::elements::blank_line_group::BlankLineGroup,
    ) {
        use lex_core::lex::ast::traits::AstNode;
        self.record("BlankLineGroup", blg.range());
    }
}

/// Parse a document and assert that every AST node's span is within source bounds.
/// Returns the number of spans checked (for test diagnostics).
fn assert_all_spans_in_bounds(source: &str) -> usize {
    use lex_core::lex::ast::traits::AstNode;

    let doc = parse_document(source).unwrap_or_else(|e| {
        panic!("Parse failed for:\n---\n{source}\n---\nError: {e}");
    });

    let mut collector = SpanCollector::new();

    // Visit the root session
    doc.root.accept(&mut collector);

    // Also visit document-level annotations
    for ann in &doc.annotations {
        ann.accept(&mut collector);
    }

    let source_len = source.len();
    for (node_type, range) in &collector.spans {
        assert!(
            range.span.start <= range.span.end,
            "Span start > end for {node_type}: span={:?}, source_len={source_len}\nSource:\n---\n{source}\n---",
            range.span
        );
        assert!(
            range.span.end <= source_len,
            "Span end ({}) exceeds source length ({source_len}) for {node_type}: span={:?}\nSource:\n---\n{source}\n---",
            range.span.end,
            range.span
        );
    }

    collector.spans.len()
}

// =============================================================================
// Property Tests: Span Bounds on Verbatim Documents
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// All AST node spans are within source bounds for flat verbatim blocks.
    #[test]
    fn span_bounds_flat_verbatim(
        subject in subject_strategy(),
        label in label_strategy(),
        content in mixed_content_lines(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (source, _) = verbatim_source(&subject, &label, &content_refs);
        let count = assert_all_spans_in_bounds(&source);
        // Should have at least the VerbatimBlock + its VerbatimLine children + root session
        assert!(count >= 2, "Expected at least 2 spans, got {count} for source:\n{source}");
    }

    /// All AST node spans are within source bounds for verbatim in session.
    #[test]
    fn span_bounds_verbatim_in_session(
        session_title in "[A-Z][a-zA-Z0-9]{0,10}",
        subject in subject_strategy(),
        label in label_strategy(),
        content in mixed_content_lines(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, _) = verbatim_source(&subject, &label, &content_refs);
        let source = wrap_in_session(&verb_source, &session_title, 0);
        assert_all_spans_in_bounds(&source);
    }

    /// All AST node spans are within source bounds for verbatim in definition.
    #[test]
    fn span_bounds_verbatim_in_definition(
        def_subject in "[A-Z][a-zA-Z0-9 ]{0,10}".prop_map(|s| s.trim_end().to_string()),
        subject in subject_strategy(),
        label in label_strategy(),
        content in mixed_content_lines(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, _) = verbatim_source(&subject, &label, &content_refs);
        let source = wrap_in_definition(&verb_source, &def_subject, 0);
        assert_all_spans_in_bounds(&source);
    }

    /// All AST node spans are within source bounds for deep nesting.
    #[test]
    fn span_bounds_deep_nesting(
        s1_title in "[A-Z][a-zA-Z0-9]{0,8}",
        s2_title in "[A-Z][a-zA-Z0-9]{0,8}",
        def_subject in "[A-Z][a-zA-Z0-9 ]{0,8}".prop_map(|s| s.trim_end().to_string()),
        subject in subject_strategy(),
        label in label_strategy(),
        content in code_like_content(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, _) = verbatim_source(&subject, &label, &content_refs);
        let def_source = wrap_in_definition(&verb_source, &def_subject, 0);
        let s2_source = wrap_in_session(&def_source, &s2_title, 0);
        let source = wrap_in_session(&s2_source, &s1_title, 0);
        assert_all_spans_in_bounds(&source);
    }

    /// All AST node spans are within source bounds for nested definitions.
    #[test]
    fn span_bounds_nested_definitions(
        d1_subject in "[A-Z][a-zA-Z0-9 ]{0,8}".prop_map(|s| s.trim_end().to_string()),
        d2_subject in "[A-Z][a-zA-Z0-9 ]{0,8}".prop_map(|s| s.trim_end().to_string()),
        subject in subject_strategy(),
        label in label_strategy(),
        content in code_like_content(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, _) = verbatim_source(&subject, &label, &content_refs);
        let d2_source = wrap_in_definition(&verb_source, &d2_subject, 0);
        let source = wrap_in_definition(&d2_source, &d1_subject, 0);
        assert_all_spans_in_bounds(&source);
    }

    /// All AST node spans are within source bounds with tricky content.
    #[test]
    fn span_bounds_tricky_content(
        s_title in "[A-Z][a-zA-Z0-9]{0,6}",
        d_subject in "[A-Z][a-zA-Z0-9]{0,6}",
        subject in subject_strategy(),
        label in label_strategy(),
        content in mixed_content_lines(),
    ) {
        let content_refs: Vec<&str> = content.iter().map(|s| s.as_str()).collect();
        let (verb_source, _) = verbatim_source(&subject, &label, &content_refs);
        let def_source = wrap_in_definition(&verb_source, &d_subject, 0);
        let source = wrap_in_session(&def_source, &s_title, 0);
        assert_all_spans_in_bounds(&source);
    }
}

// =============================================================================
// Deterministic Span Bounds Tests
// =============================================================================

#[test]
fn span_bounds_simple_verbatim() {
    let source = "\
Example:
    def foo():
        return bar
    x = 1
:: python ::";
    let count = assert_all_spans_in_bounds(source);
    assert!(
        count >= 3,
        "Expected at least 3 spans (session, verbatim block, verbatim lines), got {count}"
    );
}

#[test]
fn span_bounds_verbatim_in_session_deterministic() {
    let source = "\
Title

    Example:
        def foo():
            return bar
        x = 1
    :: python ::";
    assert_all_spans_in_bounds(source);
}

#[test]
fn span_bounds_verbatim_deep_nesting_deterministic() {
    let source = "\
Section

    Category:
        Language:
            Example:
                def foo():
                    return bar
                x = 1
            :: python ::";
    assert_all_spans_in_bounds(source);
}

#[test]
fn span_bounds_empty_verbatim_content() {
    // Verbatim block with no content lines (binary marker)
    let source = "\
Image:
:: png ::";
    assert_all_spans_in_bounds(source);
}

#[test]
fn span_bounds_single_char_verbatim() {
    let source = "\
X:
    a
:: t ::";
    assert_all_spans_in_bounds(source);
}

#[test]
fn span_bounds_verbatim_with_fake_annotations() {
    let source = "\
Example:
    line one
    :: not_an_annotation ::
    line three
:: text ::";
    assert_all_spans_in_bounds(source);
}
