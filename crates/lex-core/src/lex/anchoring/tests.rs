use crate::lex::ast::anchoring::{AnchoredElement, ReferenceAnchor, ReferenceLine};
use crate::lex::ast::traits::AstNode;
use crate::lex::inlines::{AnchorDirection, ReferenceType, WordAnchor};
use crate::lex::parsing::parse_document;

/// Helper: resolved reference lines for a source.
fn ref_lines(src: &str) -> Vec<ReferenceLine> {
    parse_document(src).unwrap().reference_lines
}

/// Helper: the single whole-element anchor text for a one-reference-line
/// source. Panics if there isn't exactly one whole-element anchor.
fn sole_whole_anchor(src: &str) -> (String, AnchoredElement) {
    let lines = ref_lines(src);
    assert_eq!(
        lines.len(),
        1,
        "expected exactly one reference line: {lines:?}"
    );
    match &lines[0].anchor {
        ReferenceAnchor::WholeElement {
            anchor_text,
            element,
            ..
        } => (anchor_text.clone(), *element),
        other => panic!("expected whole-element anchor, got {other:?}"),
    }
}

// -- Fixture §1: inline word anchors -----------------------------------

fn word_anchor(src: &str) -> WordAnchor {
    let doc = parse_document(src).unwrap();
    let r = doc
        .iter_all_references()
        .find(|r| r.word_anchor.is_some())
        .expect("a reference with a word anchor");
    r.word_anchor.clone().unwrap()
}

#[test]
fn inline_preceding_word_anchor() {
    // "the project website [https://lex.ing] is fast."
    let wa = word_anchor("the project website [https://lex.ing] is fast.\n\n");
    assert_eq!(wa.word, "website");
    assert_eq!(wa.direction, AnchorDirection::Preceding);
}

#[test]
fn inline_following_word_anchor() {
    // "[https://lex.ing] is the home page." — first on line → following.
    let wa = word_anchor("[https://lex.ing] is the home page.\n\n");
    assert_eq!(wa.word, "is");
    assert_eq!(wa.direction, AnchorDirection::Following);
}

#[test]
fn inline_abutting_word_anchor() {
    // "Hello[./file.txt] World" — abutting → preceding "Hello".
    let wa = word_anchor("Hello[./file.txt] World\n\n");
    assert_eq!(wa.word, "Hello");
    assert_eq!(wa.direction, AnchorDirection::Preceding);
}

#[test]
fn inline_preceding_word_anchor_trims_trailing_punctuation() {
    // "website, [https://x]" — the preceding token is "website," but the
    // stored word must carry no surrounding punctuation (per the
    // `WordAnchor::word` contract): "website".
    let wa = word_anchor("the project website, [https://x] is fast.\n\n");
    assert_eq!(wa.word, "website");
    assert_eq!(wa.direction, AnchorDirection::Preceding);
}

#[test]
fn inline_following_word_anchor_trims_punctuation() {
    // First-on-line reference, following token has trailing punctuation.
    let wa = word_anchor("[https://x] (home) page.\n\n");
    assert_eq!(wa.word, "home");
    assert_eq!(wa.direction, AnchorDirection::Following);
}

#[test]
fn inline_word_anchor_preserves_interior_punctuation() {
    // Interior dots/apostrophes are part of the word, not surrounding it.
    let wa = word_anchor("visit lex.ing [https://lex.ing] now.\n\n");
    assert_eq!(wa.word, "lex.ing");
    assert_eq!(wa.direction, AnchorDirection::Preceding);
}

#[test]
fn inline_punctuation_only_neighbor_yields_no_anchor() {
    // The token preceding the reference is punctuation-only; after trimming
    // nothing alphanumeric remains, so no word anchor is produced.
    let doc = parse_document("word -- [https://x] end.\n\n").unwrap();
    let r = doc
        .iter_all_references()
        .find(|r| matches!(r.reference_type, ReferenceType::Url { .. }))
        .expect("the url reference");
    assert!(
        r.word_anchor.is_none(),
        "punctuation-only neighbor must not produce an anchor: {:?}",
        r.word_anchor
    );
}

// -- Fixture §2: reference line on a session title ---------------------

#[test]
fn reference_line_anchors_session_title() {
    let src = "Getting Started\n[./readme.txt]\n\n    Welcome to the docs.\n\n";
    let (anchor, _kind) = sole_whole_anchor(src);
    assert_eq!(anchor, "Getting Started");
    // The reference line is removed; structure is a session with a body.
    let doc = parse_document(src).unwrap();
    assert_eq!(doc.root.children[0].node_type(), "Session");
}

// -- Fixture §3: reference line on a list item ------------------------

#[test]
fn reference_line_anchors_list_item() {
    let src = "- Food\n- Water\n[https://water.example]\n- Bread\n\n";
    let (anchor, kind) = sole_whole_anchor(src);
    assert_eq!(anchor, "Water");
    assert_eq!(kind, AnchoredElement::ListItem);
    // List structure is preserved (3 items, the reference line removed).
    let doc = parse_document(src).unwrap();
    assert_eq!(doc.root.children[0].node_type(), "List");
}

#[test]
fn reference_line_on_list_item_keeps_trailing_colon() {
    // A list item ending in `:` is not a subject — the colon is part of the
    // item text. Anchoring must keep it literal (`Note:`), never strip it
    // the way a definition/verbatim subject would.
    let src = "- Note:\n[./n.txt]\n- Other\n\n";
    let (anchor, kind) = sole_whole_anchor(src);
    assert_eq!(anchor, "Note:");
    assert_eq!(kind, AnchoredElement::ListItem);
}

// -- Fixture §4: reference line on a definition term (transparency) ----

#[test]
fn reference_line_keeps_definition_a_definition() {
    // The critical transparency case: with the reference line *removed*
    // (not blanked), `API Endpoint:` stays adjacent to its indented body,
    // so it remains a definition — not a session.
    let src = "API Endpoint:\n[./endpoint.txt]\n    A URL that provides access to a resource.\n\n";
    let (anchor, kind) = sole_whole_anchor(src);
    assert_eq!(anchor, "API Endpoint");
    assert_eq!(kind, AnchoredElement::Subject);

    let doc = parse_document(src).unwrap();
    assert_eq!(
        doc.root.children[0].node_type(),
        "Definition",
        "reference line must be transparent: blanking it would wrongly \
         turn the definition into a session"
    );
}

#[test]
fn reference_line_as_blank_would_make_a_session() {
    // Control: the *same* source but with a genuine blank line in place of
    // the reference line parses as a session. This pins down exactly what
    // the transparency rule prevents.
    let src = "API Endpoint:\n\n    A URL that provides access to a resource.\n\n";
    let doc = parse_document(src).unwrap();
    assert_eq!(doc.root.children[0].node_type(), "Session");
}

// -- Fixture §5: reference line on a verbatim subject -----------------

#[test]
fn reference_line_anchors_verbatim_subject() {
    let src = "Example Source:\n[./example.rs]\n    fn main() {}\n:: rust ::\n\n";
    let (anchor, kind) = sole_whole_anchor(src);
    assert_eq!(anchor, "Example Source");
    assert_eq!(kind, AnchoredElement::Subject);

    let doc = parse_document(src).unwrap();
    assert_eq!(doc.root.children[0].node_type(), "VerbatimBlock");
}

// -- Fixture §6: reference line on a paragraph -----------------------

#[test]
fn reference_line_anchors_paragraph_line() {
    // A multi-line paragraph above so the line above the reference is a
    // genuine paragraph line (not promoted to a document title).
    let src = "First paragraph line.\nThe release notes cover every change.\n[./CHANGELOG.md]\n\n";
    let (anchor, kind) = sole_whole_anchor(src);
    assert_eq!(anchor, "The release notes cover every change.");
    assert_eq!(kind, AnchoredElement::WholeLine);
}

// -- Fixture §7: self-link fallback ----------------------------------

#[test]
fn reference_line_self_links_when_blank_above() {
    let src = "See the upstream project:\n\n[https://github.com/lex-fmt/lex]\n\n";
    let lines = ref_lines(src);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].anchor, ReferenceAnchor::SelfLink);
}

#[test]
fn reference_line_self_links_at_start_of_container() {
    // First line of the document → no content above → self-link.
    let src = "[https://lex.ing]\n\n";
    let lines = ref_lines(src);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].anchor, ReferenceAnchor::SelfLink);
}

// -- Fixture §8: marker-style references on a reference line ----------

#[test]
fn marker_reference_on_its_own_line_is_not_a_reference_line() {
    // `[::summary-note]` is marker-style: it does NOT take a whole-element
    // anchor; it stays in the stream and resolves as usual.
    let src = "Closing remarks.\n[::summary-note]\n\n:: summary-note ::\n    Resolved.\n\n";
    let lines = ref_lines(src);
    assert!(
        lines.is_empty(),
        "marker-style reference must not become a whole-element anchor: {lines:?}"
    );
    // It remains an inline reference in the document.
    let doc = parse_document(src).unwrap();
    assert!(doc
        .iter_all_references()
        .any(|r| matches!(r.reference_type, ReferenceType::AnnotationReference { .. })));
}

#[test]
fn footnote_on_its_own_line_is_not_a_reference_line() {
    let src = "Some claim.\n[42]\n\n:: 42 :: A footnote.\n\n";
    assert!(ref_lines(src).is_empty());
}

// -- §2.3.3: overlap / stacking diagnostics --------------------------

#[test]
fn stacked_reference_lines_warn_and_keep_first() {
    // Two reference lines over the same paragraph line.
    let src = "First line.\nClaim line here.\n[./a.txt]\n[./b.txt]\n\n";
    let doc = parse_document(src).unwrap();
    let lines = &doc.reference_lines;
    assert_eq!(lines.len(), 2, "both reference lines are collected");
    // Exactly one stacked-reference-line warning is emitted.
    let warns: Vec<_> = doc
        .diagnostics()
        .into_iter()
        .filter(|d| d.code.as_deref() == Some("stacked-reference-line"))
        .collect();
    assert_eq!(warns.len(), 1, "one stacking warning: {warns:?}");
}

#[test]
fn reference_line_over_head_line_with_inline_reference_warns() {
    // The head line already carries an inline reference, so the
    // whole-element anchor would nest two links over the same text.
    let src = "See more here.\nVisit [https://a.example] now.\n[./b.txt]\n\n";
    let doc = parse_document(src).unwrap();
    let warns: Vec<_> = doc
        .diagnostics()
        .into_iter()
        .filter(|d| d.code.as_deref() == Some("overlapping-reference-line"))
        .collect();
    assert_eq!(warns.len(), 1, "one overlap warning: {warns:?}");
    // The whole-line anchor is still honored (§2.3.3).
    assert!(doc.reference_lines[0].anchor.is_whole_element());
}

#[test]
fn head_line_with_stray_bracket_does_not_warn() {
    // The head line contains a `[` but no genuine inline reference (it is a
    // code span, not a reference). The string/bracket heuristic used to
    // false-positive here; the AST-based check must not fire the overlap
    // warning.
    let src = "Intro.\nThe array index `a[0]` matters.\n[./b.txt]\n\n";
    let doc = parse_document(src).unwrap();
    let warns: Vec<_> = doc
        .diagnostics()
        .into_iter()
        .filter(|d| d.code.as_deref() == Some("overlapping-reference-line"))
        .collect();
    assert!(
        warns.is_empty(),
        "a stray bracket is not an inline reference: {warns:?}"
    );
    // The whole-line anchor is still resolved.
    assert!(doc.reference_lines[0].anchor.is_whole_element());
}

// -- Type-level anchoring split (§2.3.4) -----------------------------

#[test]
fn anchor_kind_split_matches_spec() {
    use crate::lex::inlines::AnchorKind;
    assert_eq!(
        ReferenceType::Url { target: "x".into() }.anchoring(),
        AnchorKind::WholeLineCapable
    );
    assert_eq!(
        ReferenceType::File { target: "x".into() }.anchoring(),
        AnchorKind::WholeLineCapable
    );
    assert_eq!(
        ReferenceType::Session { target: "1".into() }.anchoring(),
        AnchorKind::WholeLineCapable
    );
    assert_eq!(
        ReferenceType::General { target: "x".into() }.anchoring(),
        AnchorKind::WholeLineCapable
    );
    assert_eq!(
        ReferenceType::FootnoteNumber { number: 1 }.anchoring(),
        AnchorKind::MarkerOnly
    );
    assert_eq!(
        ReferenceType::AnnotationReference { label: "n".into() }.anchoring(),
        AnchorKind::MarkerOnly
    );
    assert_eq!(ReferenceType::NotSure.anchoring(), AnchorKind::MarkerOnly);
}

// -- Range fidelity ---------------------------------------------------

#[test]
fn anchor_range_covers_the_head_line_text() {
    let src = "Getting Started\n[./readme.txt]\n\n    Body.\n\n";
    let doc = parse_document(src).unwrap();
    let ReferenceAnchor::WholeElement { anchor_range, .. } = &doc.reference_lines[0].anchor else {
        panic!("expected whole-element anchor");
    };
    assert_eq!(&src[anchor_range.span.clone()], "Getting Started");
}

#[test]
fn reference_range_covers_brackets_inclusive() {
    let src = "Getting Started\n[./readme.txt]\n\n    Body.\n\n";
    let doc = parse_document(src).unwrap();
    let range = &doc.reference_lines[0].reference_range;
    assert_eq!(&src[range.span.clone()], "[./readme.txt]");
}

// -- Original-coordinate invariant (regression for the cleaned-source
//    coordinate bug) --------------------------------------------------

/// Removing a reference line by *editing the source string* used to shift
/// every byte offset after it, so parsed AST nodes that followed a reference
/// line carried "cleaned-source" coordinates instead of original-source
/// ones. The token-filtering pre-pass keeps tokens at their original ranges,
/// so every node after a reference line must still report its position in
/// the ORIGINAL source.
///
/// This asserts a later element's parsed range start equals the byte offset
/// of its text in the original source. It fails against the old
/// cleaned-source approach (the offset is short by the removed line's
/// length) and passes with token filtering.
#[test]
fn later_element_keeps_original_source_coordinates() {
    // A reference line near the top, then a clearly later paragraph. The
    // removed `[./top.txt]\n` line is 12 bytes; under the old cleaned-source
    // approach every node after it was shifted left by 12.
    let original =
        "Intro paragraph here.\n[./top.txt]\n\nLater Section paragraph text.\n\n".to_string();

    let doc = parse_document(&original).unwrap();

    // Find the parsed paragraph whose text starts with "Later Section".
    let later = doc
        .root
        .children
        .iter()
        .find(|c| {
            c.text()
                .map(|t| t.contains("Later Section"))
                .unwrap_or(false)
        })
        .expect("a 'Later Section' element after the reference line");

    let expected_start = original
        .find("Later Section")
        .expect("the literal text in the original source");

    assert_eq!(
        later.range().span.start,
        expected_start,
        "node after a reference line must carry an ORIGINAL-source offset \
         (got {}, expected {}); a mismatch means a cleaned-source coordinate \
         leaked into the AST",
        later.range().span.start,
        expected_start,
    );

    // And the slice at that range is the actual original text.
    assert!(original[later.range().span.clone()].starts_with("Later Section"));
}

// -- Cleaned-source / no-reference-line passthrough ------------------

#[test]
fn documents_without_reference_lines_have_empty_collection() {
    let doc = parse_document("Just a paragraph.\n\n").unwrap();
    assert!(doc.reference_lines.is_empty());
    assert!(doc.reference_line_diagnostics.is_empty());
}

#[test]
fn list_marker_stripping_handles_ordered_markers() {
    let src = "1. First item\n[./x.txt]\n2. Second item\n\n";
    let (anchor, kind) = sole_whole_anchor(src);
    assert_eq!(anchor, "First item");
    assert_eq!(kind, AnchoredElement::ListItem);
}

// -- §lex#755: verbatim bodies are raw, `[...]`-led lines stay literal ---

/// Every verbatim body line, in document order — collected by walking the
/// verbatim block's groups. Empty when there is no verbatim block. Used to
/// assert that a given bracket line survives the parse literally (the tests
/// look it up among these lines).
fn verbatim_body_lines(doc: &crate::lex::ast::Document) -> Vec<String> {
    use crate::lex::ast::elements::ContentItem;
    let mut out = Vec::new();
    for child in &doc.root.children {
        if let ContentItem::VerbatimBlock(vb) = child {
            for group in vb.group() {
                for line in group.children.iter() {
                    if let ContentItem::VerbatimLine(vl) = line {
                        out.push(vl.content.as_string().to_string());
                    }
                }
            }
        }
    }
    out
}

#[test]
fn verbatim_body_bracket_lines_stay_literal() {
    // The lex#755 repro: a TOML example inside an inflow verbatim block. A
    // single-word table header (`[server]`) and a dotted one
    // (`[formatting.rules]`) must both survive verbatim — never be ejected
    // as reference lines and re-emitted as auto-links.
    let src =
        "Config example:\n\n    [server]\n    [formatting.rules]\n    port = 8080\n:: toml ::\n\n";
    let doc = parse_document(src).unwrap();

    // No reference line was ejected from the verbatim body.
    assert!(
        doc.reference_lines.is_empty(),
        "verbatim body lines must not become reference lines: {:?}",
        doc.reference_lines
    );
    // And no inline reference leaked out of the raw body.
    assert!(
        doc.iter_all_references().next().is_none(),
        "verbatim body must carry no parsed references"
    );

    // Both bracket lines are preserved literally inside the block.
    let lines = verbatim_body_lines(&doc);
    assert!(
        lines.iter().any(|l| l == "[server]"),
        "`[server]` must stay literal in the verbatim body: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "[formatting.rules]"),
        "`[formatting.rules]` must stay literal in the verbatim body: {lines:?}"
    );
}

#[test]
fn fullwidth_verbatim_sole_bracket_body_stays_literal() {
    // A fullwidth verbatim block whose entire body is a single bracket line.
    // With no body following it, the bracket is the block's content, not an
    // anchoring reference line for the subject — it must stay literal.
    let src = "Config:\n[section]\n:: toml ::\n\n";
    let doc = parse_document(src).unwrap();
    assert!(
        doc.reference_lines.is_empty(),
        "sole-body bracket must not become a reference line: {:?}",
        doc.reference_lines
    );
    let lines = verbatim_body_lines(&doc);
    assert!(
        lines.iter().any(|l| l == "[section]"),
        "`[section]` must stay literal: {lines:?}"
    );
}

#[test]
fn fullwidth_verbatim_first_bracket_body_line_stays_literal() {
    // Regression for the fullwidth variant (caught in review): when the
    // first fullwidth body line is a bracket AND more body follows at the
    // subject's own indentation, that bracket is still body — it must stay
    // literal, not be ejected as an anchor for the subject. The anchoring
    // slot only applies when *deeper-indented* (inflow) body follows.
    let src = "Config:\n[server]\nport = 8080\n:: toml ::\n\n";
    let doc = parse_document(src).unwrap();
    assert!(
        doc.reference_lines.is_empty(),
        "fullwidth first-line bracket must not become a reference line: {:?}",
        doc.reference_lines
    );
    let lines = verbatim_body_lines(&doc);
    assert!(
        lines.iter().any(|l| l == "[server]"),
        "`[server]` must stay literal in the fullwidth body: {lines:?}"
    );
}

#[test]
fn prose_bracket_line_still_becomes_a_reference_line() {
    // No-regression guard: a `[token]` line in ORDINARY prose (not inside a
    // verbatim block) must still be extracted as a reference line and anchor
    // the paragraph above — exactly as before lex#755's fix.
    let src = "First paragraph line.\nThe project home page.\n[server]\n\n";
    let (anchor, kind) = sole_whole_anchor(src);
    assert_eq!(anchor, "The project home page.");
    assert_eq!(kind, AnchoredElement::WholeLine);
}

#[test]
fn verbatim_subject_anchor_still_works_with_body_following() {
    // No-regression guard for the documented anchoring shape: a reference
    // line directly below a verbatim subject, with the indented body
    // following, still anchors the subject (it is not protected as body).
    let src = "Example Source:\n[https://lex.ing]\n    fn main() {}\n:: rust ::\n\n";
    let (anchor, kind) = sole_whole_anchor(src);
    assert_eq!(anchor, "Example Source");
    assert_eq!(kind, AnchoredElement::Subject);
    // The body line is still literal inside the block.
    let doc = parse_document(src).unwrap();
    let lines = verbatim_body_lines(&doc);
    assert!(
        lines.iter().any(|l| l == "fn main() {}"),
        "verbatim body preserved: {lines:?}"
    );
}
