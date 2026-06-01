//! Integration tests for whole-element / word anchoring of references.
//!
//! Pins the resolved anchor for every case in the canonical comms fixture
//! `comms/specs/elements/inlines.docs/specs/references/reference-anchoring.lex`
//! (the authoritative behavior spec is §2.3 of `references-general.lex`).
//!
//! The fixture is a *documentation* `.lex` file: each example is shown inside a
//! 4-space-indented illustration block, so parsing the whole file does not
//! surface the examples as top-level reference lines. These tests therefore
//! exercise the canonical snippets directly through the parser (the same text
//! that appears in the fixture), and additionally assert that the fixture file
//! itself parses cleanly.

use lex_core::lex::ast::anchoring::{AnchoredElement, ReferenceAnchor};
use lex_core::lex::ast::traits::AstNode;
use lex_core::lex::inlines::AnchorDirection;
use lex_core::lex::parsing::parse_document;
use lex_core::lex::testing::workspace_path;

const FIXTURE: &str = "comms/specs/elements/inlines.docs/specs/references/reference-anchoring.lex";

fn whole_anchor(src: &str) -> (String, AnchoredElement) {
    let doc = parse_document(src).unwrap();
    assert_eq!(
        doc.reference_lines.len(),
        1,
        "expected one reference line in {src:?}, got {:?}",
        doc.reference_lines
    );
    match &doc.reference_lines[0].anchor {
        ReferenceAnchor::WholeElement {
            anchor_text,
            element,
            ..
        } => (anchor_text.clone(), *element),
        other => panic!("expected whole-element anchor, got {other:?}"),
    }
}

#[test]
fn fixture_file_exists_and_parses() {
    let path = workspace_path(FIXTURE);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    assert!(source.contains("Reference Anchoring Fixture"));
    parse_document(&source).expect("fixture parses");
}

// Fixture §1 — inline word anchors.

#[test]
fn fixture_1_preceding_word() {
    let doc = parse_document("the project website [https://lex.ing] is fast.\n\n").unwrap();
    let wa = doc
        .iter_all_references()
        .find_map(|r| r.word_anchor.clone())
        .unwrap();
    assert_eq!(wa.word, "website");
    assert_eq!(wa.direction, AnchorDirection::Preceding);
}

#[test]
fn fixture_1_following_word() {
    let doc = parse_document("[https://lex.ing] is the home page.\n\n").unwrap();
    let wa = doc
        .iter_all_references()
        .find_map(|r| r.word_anchor.clone())
        .unwrap();
    assert_eq!(wa.word, "is");
    assert_eq!(wa.direction, AnchorDirection::Following);
}

#[test]
fn fixture_1_explicit_single_word() {
    let doc = parse_document("Hello[./file.txt] World\n\n").unwrap();
    let wa = doc
        .iter_all_references()
        .find_map(|r| r.word_anchor.clone())
        .unwrap();
    assert_eq!(wa.word, "Hello");
    assert_eq!(wa.direction, AnchorDirection::Preceding);
}

// Fixture §2 — reference line on a session title.

#[test]
fn fixture_2_session_title() {
    let (anchor, _) =
        whole_anchor("Getting Started\n[./readme.txt]\n\n    Welcome to the docs.\n\n");
    assert_eq!(anchor, "Getting Started");
}

// Fixture §3 — reference line on a list item.

#[test]
fn fixture_3_list_item() {
    let (anchor, kind) = whole_anchor("- Food\n- Water\n[https://water.example]\n- Bread\n\n");
    assert_eq!(anchor, "Water");
    assert_eq!(kind, AnchoredElement::ListItem);
}

// Fixture §4 — reference line on a definition term (transparency).

#[test]
fn fixture_4_definition_term_transparent() {
    let src = "API Endpoint:\n[./endpoint.txt]\n    A URL that provides access to a resource.\n\n";
    let (anchor, kind) = whole_anchor(src);
    assert_eq!(anchor, "API Endpoint");
    assert_eq!(kind, AnchoredElement::Subject);
    let doc = parse_document(src).unwrap();
    assert_eq!(doc.root.children[0].node_type(), "Definition");
}

// Fixture §5 — reference line on a verbatim subject.

#[test]
fn fixture_5_verbatim_subject() {
    let src =
        "Example Source:\n[./example.rs]\n    fn main() {\n        ok();\n    }\n:: rust ::\n\n";
    let (anchor, kind) = whole_anchor(src);
    assert_eq!(anchor, "Example Source");
    assert_eq!(kind, AnchoredElement::Subject);
    let doc = parse_document(src).unwrap();
    assert_eq!(doc.root.children[0].node_type(), "VerbatimBlock");
}

// Fixture §6 — reference line on a paragraph.

#[test]
fn fixture_6_paragraph_line() {
    let src =
        "First line of notes.\nThe release notes cover every change in this cycle.\n[./CHANGELOG.md]\n\n";
    let (anchor, kind) = whole_anchor(src);
    assert_eq!(
        anchor,
        "The release notes cover every change in this cycle."
    );
    assert_eq!(kind, AnchoredElement::WholeLine);
}

// Fixture §7 — self-link fallback.

#[test]
fn fixture_7_self_link() {
    let src = "There is no content line directly above:\n\n[https://github.com/lex-fmt/lex]\n\n";
    let doc = parse_document(src).unwrap();
    assert_eq!(doc.reference_lines.len(), 1);
    assert_eq!(doc.reference_lines[0].anchor, ReferenceAnchor::SelfLink);
}

// Fixture §8 — marker-style references on a reference line.

#[test]
fn fixture_8_marker_style_not_a_reference_line() {
    let src =
        "Closing remarks.\n[::summary-note]\n\n:: summary-note ::\n    Resolved by label.\n\n";
    let doc = parse_document(src).unwrap();
    assert!(
        doc.reference_lines.is_empty(),
        "marker-style reference must not take a whole-element anchor: {:?}",
        doc.reference_lines
    );
}
