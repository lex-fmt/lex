//! Structural block-separation tests for the Lex serializer (lex#781).
//!
//! These drive the separation-matrix seam directly, on *reader-shaped* ASTs
//! (built by hand with NO `BlankLineGroup` nodes — the shape every non-lex
//! Reader produces) and on lex-sourced ASTs (which carry `BlankLineGroup`s).
//! They lock three things for the paragraph→paragraph cell:
//!
//!   1. a reader-shaped pair of paragraphs gains the grammar-mandated blank,
//!   2. composition with `BlankLineGroup` is max (`ensure-at-least-N`), never
//!      additive,
//!   3. lex → lex output is byte-identical (zero regression).
//!
//! Slice #781 wires only paragraph→paragraph; other pairs still emit 0 here.

use lex_babel::formats::lex::export;
use lex_babel::transforms::format_lex_source;
use lex_core::lex::ast::elements::container::SessionContainer;
use lex_core::lex::ast::elements::typed_content::{ContentElement, SessionContent};
use lex_core::lex::ast::elements::BlankLineGroup;
use lex_core::lex::ast::{ContentItem, Document, Paragraph, Session};
use lex_core::lex::parsing::parse_document;

/// Serialize a hand-built (reader-shaped) document to Lex.
fn serialize(doc: &Document) -> String {
    export(doc).expect("serialization should not fail")
}

/// A one-line paragraph as a top-level `SessionContent`.
fn para(text: &str) -> SessionContent {
    SessionContent::Element(ContentElement::Paragraph(Paragraph::from_line(text.into())))
}

/// A blank-line group of `count` blanks as a top-level `SessionContent`.
fn blanks(count: usize) -> SessionContent {
    SessionContent::Element(ContentElement::BlankLineGroup(BlankLineGroup {
        count,
        source_tokens: vec![],
        location: Default::default(),
    }))
}

/// Build a document whose body is exactly `children`.
fn doc_with(children: Vec<SessionContent>) -> Document {
    let mut doc = Document::new();
    doc.root.children = SessionContainer::from_typed(children);
    doc
}

/// Count the blank lines that sit between the first occurrence of `before` and
/// the next occurrence of `after` (0 = the lines are adjacent, 1 = one blank).
fn blanks_between(s: &str, before: &str, after: &str) -> usize {
    let start = s.find(before).expect("`before` present") + before.len();
    let end = s[start..]
        .find(after)
        .expect("`after` present after `before`")
        + start;
    s[start..end].matches('\n').count().saturating_sub(1)
}

#[test]
fn reader_shaped_two_paragraphs_gain_structural_blank() {
    // No BlankLineGroup between them — the matrix must supply the separator.
    let out = serialize(&doc_with(vec![para("First."), para("Second.")]));
    assert_eq!(out, "First.\n\nSecond.\n");
}

#[test]
fn single_paragraph_has_no_leading_or_trailing_blank() {
    // A paragraph at document start and end must not gain a spurious blank on
    // either side.
    let out = serialize(&doc_with(vec![para("Solo.")]));
    assert_eq!(out, "Solo.\n");
}

#[test]
fn first_block_gets_no_leading_blank() {
    let out = serialize(&doc_with(vec![para("First."), para("Second.")]));
    assert!(
        out.starts_with("First."),
        "the first block must not be preceded by a blank; got:\n{out}"
    );
}

#[test]
fn composition_with_blank_line_group_is_max_not_additive() {
    // Structural minimum is 1. Composing with a BlankLineGroup(k) must yield
    // max(1, k) blanks (clamped to the formatter's max_blank_lines = 2), never
    // 1 + k. k = 1 is the additive tell: additive would give 2, max gives 1.
    let cases = [(0usize, 1usize), (1, 1), (2, 2), (5, 2)];
    for (k, want) in cases {
        let out = serialize(&doc_with(vec![para("A."), blanks(k), para("B.")]));
        let got = blanks_between(&out, "A.", "B.");
        assert_eq!(
            got, want,
            "BlankLineGroup({k}) should compose to {want} blank(s), got {got}; output:\n{out}"
        );
    }
}

#[test]
fn lex_source_two_paragraphs_are_byte_identical() {
    // The zero-regression guarantee: a lex-sourced two-paragraph document (its
    // separator already carried by a BlankLineGroup) formats byte-for-byte
    // unchanged. A two-line leading paragraph keeps lex-core's title-steal rule
    // from promoting the first line to the document title.
    let src = "Alpha line one.\nAlpha line two.\n\nBeta paragraph.\n";
    let formatted = format_lex_source(src).expect("format");
    assert_eq!(
        formatted, src,
        "lex -> lex must be byte-identical for two-paragraph documents"
    );
}

#[test]
fn lex_source_multiple_body_paragraphs_byte_identical() {
    // Three body paragraphs (leading two-line paragraph again avoids title-steal),
    // each separated by a single blank — the common formatter shape.
    let src = "Head line one.\nHead line two.\n\nMiddle.\n\nTail.\n";
    let formatted = format_lex_source(src).expect("format");
    assert_eq!(formatted, src);
}

/// Count `Paragraph` nodes directly under a session's children.
fn paragraphs_in_session(session: &Session) -> usize {
    session
        .children
        .iter()
        .filter(|c| matches!(c, ContentItem::Paragraph(_)))
        .count()
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(96))]

    /// For N reader-shaped paragraphs nested under a session, with an ARBITRARY
    /// BlankLineGroup count in each gap (including 0 — the reader-shaped case),
    /// serialize→reparse preserves the paragraph count: the structural minimum
    /// guarantees every adjacent pair stays two paragraphs, and max-composition
    /// keeps a present BlankLineGroup from adding a second merge-or-split.
    ///
    /// The paragraphs are nested in a session so the document-title boundary
    /// (lex-core's title-steal, addressed by the ADR-0002 model in #783) does
    /// not confound the count.
    #[test]
    fn nested_paragraphs_preserve_count_under_arbitrary_blanks(
        n in 1usize..6,
        gaps in proptest::collection::vec(0usize..4, 0..5),
    ) {
        // Build: session "Doc" { p0 [blanks] p1 [blanks] ... p{n-1} }.
        let mut body: Vec<SessionContent> = Vec::new();
        for i in 0..n {
            if i > 0 {
                let k = gaps.get(i - 1).copied().unwrap_or(0);
                if k > 0 {
                    body.push(blanks(k));
                }
            }
            body.push(para(&format!("para number {i}")));
        }
        let session = Session::new(
            lex_core::lex::ast::TextContent::from_string("Doc".to_string(), None),
            body,
        );
        let doc = doc_with(vec![SessionContent::Session(session)]);

        let out = serialize(&doc);
        let reparsed = parse_document(&out).expect("serialized Lex must re-parse");

        let session = reparsed
            .root
            .children
            .iter()
            .find_map(|c| match c {
                ContentItem::Session(s) => Some(s),
                _ => None,
            })
            .expect("reparsed doc must contain the session");

        proptest::prop_assert_eq!(
            paragraphs_in_session(session),
            n,
            "expected {} paragraphs to survive; Lex was:\n{}",
            n,
            out
        );
    }
}
