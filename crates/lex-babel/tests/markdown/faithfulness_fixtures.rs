//! Real-reader fixture corpus + adjacency-pair + degrade cases (lex#785).
//!
//! Slice 5 of the Markdown↔Lex faithfulness epic: end-to-end confidence on real
//! documents, driven through the **real Markdown reader** (`MarkdownFormat::parse`,
//! comrak), not synthetic ASTs. The invariant under test is Faithfulness
//! (CONTEXT.md): `canon(md_read(src)) == canon(reparse(serialize(md_read(src))))`,
//! via the shared `crate::skeleton::check_faithful`.
//!
//! ## MAJOR FINDING — real documents do NOT round-trip faithfully today
//!
//! Every real fixture (`010-kitchensink.md`, `comrak-readme.md`, the CommonMark
//! reference, …) still FAILS end-to-end Faithfulness through the real reader.
//! The empirical causes, all **pre-existing** and confirmed by minimal repros:
//!
//!   - **lex#798 (reader-built loose lists collapse on serialize) — FIXED.** The
//!     Markdown reader builds every list item as
//!     `ListItem { text: "", children: [Paragraph] }` (comrak's block model — an
//!     item contains a paragraph); the Lex serializer used to emit that as the
//!     loose `-\n    text` form, which lex-core does not re-parse as a list (it
//!     collapsed the whole list into one Paragraph). The serializer now hoists the
//!     item's leading Paragraph onto the tight `- text` marker line (and the
//!     reader records the list's decoration style on the List node), so
//!     reader-built lists — unordered, ordered, nested, multi-block — round-trip.
//!     Lists are no longer a blocker for any fixture; the entries below were
//!     re-attributed to the OTHER bug each still trips once #798 was out of the way.
//!   - **lex#790 (verbatim/definition nested bodies de-indent)** — a colon-terminated
//!     paragraph before a verbatim is absorbed as the verbatim subject / turned into
//!     a Definition, and the body de-indents. This is what now blocks the CommonMark
//!     reference, comrak-readme, and comrak-reference (each has such a colon-para →
//!     fenced-code adjacency, some inside a list item). It also blocks kitchensink:
//!     its empty ```` ``` image ```` fence becomes an empty-subject verbatim whose
//!     colon line is re-anchored by the closer hijack and swallows the preceding
//!     `:: todo ::` block-annotation body.
//!   - **lex#795 (session-marker inference) — FIXED.** A style-less session whose
//!     title text begins with a marker-like token (`## 1\. Primary Session` in
//!     kitchensink) used to serialize to a bare `1. Primary Session`, which
//!     re-parsed WITH a Numerical session marker where the reader had `style: None`.
//!     The Lex serializer now escapes the marker's structural separator
//!     (`1\. Primary Session`, escaping.lex §3.4) when — and only when — the session
//!     carries no explicit marker, and lex-core strips that guard backslash on
//!     re-parse, so the session round-trips with `style: None` and its title text
//!     unchanged. Sessions are no longer a blocker for any fixture.
//!   - **lex#791 (leading-annotation reorder)** — `20-ideas-naked.md`'s leading
//!     document-level annotations reorder around the title on serialize. A related
//!     annotation-attachment limitation (a floating block annotation attaches to
//!     its neighbor rather than round-tripping as a sibling) is a second residual
//!     blocker for kitchensink.
//!
//! ## How this suite stays honest (no forced green, no weakened canon)
//!
//! It does NOT skip or weaken the assertions to go green. Instead it uses the same
//! **known-failure sweep** the formatter suite uses (`format_invariants`): every
//! fixture is driven through the real reader, and each one still blocked by a
//! tracked bug is listed against that issue. The sweep asserts (a) no *unlisted*
//! fixture violates Faithfulness (so a genuinely-new regression fails loudly) and
//! (b) every *listed* fixture still violates it (anti-rot — the moment its bug
//! (#798/#790/#791/#795) is fixed, the fixture flips to faithful and this list
//! forces its removal, turning the criterion into a live assertion). The floor
//! that DOES hold today — the reader always emits well-formed, re-parseable Lex —
//! is asserted directly (`every_fixture_reader_output_is_reparseable`).

use lex_babel::format::Format;
use lex_babel::formats::lex::LexFormat;
use lex_babel::formats::markdown::MarkdownFormat;
use lex_core::lex::ast::ContentItem;
use std::path::PathBuf;

use crate::skeleton::{canon, check_faithful};

fn crate_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read(rel: &str) -> String {
    let path = crate_path(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ============================================================================
// REAL-READER FIXTURE CORPUS (acceptance criterion 1)
// ============================================================================

/// The real Markdown fixtures, keyed by a stable name, path relative to the
/// crate root. `010-kitchensink.md` and `20-ideas-naked.md` are the curated
/// comms benchmark markdown fixtures (the tests/fixtures/kitchensink.md sibling
/// is an empty placeholder). comrak-readme + the two references are large real
/// documents that already live in tests/fixtures.
const FIXTURES: &[(&str, &str)] = &[
    (
        "kitchensink",
        "../../comms/specs/benchmark/010-kitchensink.md",
    ),
    ("comrak-readme", "tests/fixtures/comrak-readme.md"),
    (
        "commonmark-reference",
        "tests/fixtures/markdown-reference-commonmark.md",
    ),
    (
        "comrak-reference",
        "tests/fixtures/markdown-reference-comrak.md",
    ),
    (
        "ideas-naked",
        "../../comms/specs/benchmark/20-ideas-naked.md",
    ),
];

/// Fixtures whose end-to-end Faithfulness is BLOCKED by a tracked pre-existing
/// bug, mapped to the remaining tracked issue now that lex#798 (lists) is fixed.
/// See the module docs for the full per-fixture cause analysis.
///
/// DO NOT clear an entry by weakening `canon` — fix the referenced bug. The
/// sweep fails loudly if a listed fixture starts passing (promote it) or an
/// unlisted one starts failing (a new regression).
const FIXTURE_KNOWN_FAIL: &[(&str, &str)] = &[
    // #798 (reader-built loose lists) is FIXED — reader-built lists now round-trip.
    // #795 (session-marker inference) is FIXED — a style-less marker-like heading
    // (`## 1\. Primary Session`) now serializes with an escaped guard (`1\.`) and
    // re-parses with `style: None`. Each fixture below was re-verified (recursive
    // Skeleton diff) to have no remaining list- or session-marker divergence; the
    // entry names the OTHER bug it still trips.
    //
    // commonmark-reference / comrak-readme / comrak-reference → #790 (a
    // colon-terminated paragraph before a fenced code block is absorbed as the
    // verbatim subject / becomes a Definition, some inside a list item).
    //
    // kitchensink → #790 as well: with #795 fixed, its residual divergence is the
    // empty ```` ``` image ```` fence, which serializes to an empty-subject
    // verbatim (`:` + `:: image ::`) whose colon line is re-anchored by the
    // closer hijack (separation.rs "Closer re-anchoring") and swallows the
    // preceding `:: todo ::` block-annotation body. It ALSO trips the standalone
    // block-annotation limitation (a floating annotation attaches to its
    // neighboring paragraph rather than round-tripping as a sibling — documented
    // in separation.rs §"Annotations", the #791 class), so kitchensink stays
    // blocked until both land. Tagged against #790, the first tracked cause.
    ("kitchensink", "lex#790"),
    ("comrak-readme", "lex#790"),
    ("commonmark-reference", "lex#790"),
    ("comrak-reference", "lex#790"),
    // #791 — leading document-level annotations reorder around the title.
    ("ideas-naked", "lex#791"),
];

/// Drive the real reader over every fixture and grade Faithfulness against the
/// known-failure list. Asserts no unlisted fixture fails and no listed fixture
/// passes (anti-rot).
#[test]
fn faithfulness_over_real_fixtures() {
    let mut unexpected_fail = Vec::new();
    let mut unexpected_pass = Vec::new();
    let mut blocked = Vec::new();

    for (key, rel) in FIXTURES {
        let src = read(rel);
        let issue = FIXTURE_KNOWN_FAIL
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, i)| *i);
        match (check_faithful(&MarkdownFormat, &src), issue) {
            (Ok(()), None) => {}
            (Ok(()), Some(issue)) => unexpected_pass.push(format!(
                "{key}: listed as known-failing ({issue}) but now round-trips faithfully — \
                 remove it from FIXTURE_KNOWN_FAIL and let this become a live assertion"
            )),
            (Err(report), None) => unexpected_fail.push(format!("[{key}]\n{report}")),
            (Err(_), Some(issue)) => blocked.push(format!("{key} -> {issue}")),
        }
    }

    if !blocked.is_empty() {
        eprintln!(
            "faithfulness_over_real_fixtures: {} fixture(s) blocked by tracked bugs:\n  {}",
            blocked.len(),
            blocked.join("\n  ")
        );
    }

    let mut problems = Vec::new();
    if !unexpected_pass.is_empty() {
        problems.push(unexpected_pass.join("\n"));
    }
    if !unexpected_fail.is_empty() {
        problems.push(format!(
            "{} NEW Faithfulness violation(s) (not in FIXTURE_KNOWN_FAIL):\n\n{}",
            unexpected_fail.len(),
            unexpected_fail.join("\n\n========\n\n")
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}

/// The property that DOES hold end-to-end today, even while full Faithfulness is
/// blocked: the reader's Lex output is always well-formed and re-parses without
/// error into a non-empty document. This is the "predictable, non-corrupt"
/// floor — the reader never emits Lex that fails to parse.
#[test]
fn every_fixture_reader_output_is_reparseable() {
    let lex = LexFormat::default();
    for (key, rel) in FIXTURES {
        let src = read(rel);
        let doc = MarkdownFormat
            .parse(&src)
            .unwrap_or_else(|e| panic!("[{key}] reader failed: {e}"));
        let lex_text = lex
            .serialize(&doc)
            .unwrap_or_else(|e| panic!("[{key}] serialize failed: {e}"));
        let reparsed = lex.parse(&lex_text).unwrap_or_else(|e| {
            panic!("[{key}] reader produced non-reparseable Lex: {e}\n--- Lex ---\n{lex_text}")
        });
        assert!(
            !reparsed.root.children.is_empty(),
            "[{key}] reader output re-parsed to an empty document"
        );
    }
}

// ============================================================================
// TARGETED ADJACENCY PAIRS (acceptance criterion 2)
//
// Each block-type adjacency pair as a minimal Markdown input to the real reader.
// Inputs are wrapped in a `# T` H1 (the Document Title) so the first body block
// is not title-promoted, isolating the pair from the title boundary.
//
// All adjacency pairs — para→para, heading→body, para→verbatim, para→definition,
// and (since lex#798) para→list and list→para — round-trip faithfully.
// ============================================================================

const H1: &str = "# T\n\n";

#[test]
fn adjacency_paragraph_to_paragraph() {
    check_faithful(
        &MarkdownFormat,
        &format!("{H1}First body.\n\nSecond body.\n"),
    )
    .unwrap();
}

#[test]
fn adjacency_heading_to_body() {
    check_faithful(&MarkdownFormat, &format!("{H1}## Section\n\nBody text.\n")).unwrap();
}

#[test]
fn adjacency_paragraph_to_verbatim() {
    check_faithful(
        &MarkdownFormat,
        &format!("{H1}Lead in.\n\n```rust\nfn x() {{}}\n```\n"),
    )
    .unwrap();
}

#[test]
fn adjacency_paragraph_to_definition() {
    check_faithful(
        &MarkdownFormat,
        &format!("{H1}Lead in.\n\n**Term**:\n\nDescription.\n"),
    )
    .unwrap();
}

/// paragraph→list and list→paragraph now round-trip faithfully (lex#798 fixed):
/// the Lex serializer hoists a reader-built item's leading Paragraph onto the
/// `- text` marker line, which re-parses as a list instead of collapsing to a
/// paragraph.
#[test]
fn adjacency_list_pairs() {
    let para_to_list = format!("{H1}Lead in.\n\n- one\n- two\n");
    let list_to_para = format!("{H1}- one\n- two\n\nTrailer.\n");
    check_faithful(&MarkdownFormat, &para_to_list).unwrap();
    check_faithful(&MarkdownFormat, &list_to_para).unwrap();
}

// ============================================================================
// H1-LED / TITLE-LESS (acceptance criterion 2)
//
// The title-model faithfulness cases (ADR-0002 / lex#783) are also asserted
// directly in import.rs; these restate them here as the fixture-suite's own
// H1-led and title-less coverage so the acceptance criterion is self-contained.
// ============================================================================

#[test]
fn h1_led_document_is_faithful() {
    let md = "# My Title\n\nFirst paragraph.\n\nSecond paragraph.\n";
    check_faithful(&MarkdownFormat, md).unwrap();
    let doc = MarkdownFormat.parse(md).unwrap();
    assert_eq!(
        doc.title.as_ref().map(|t| t.as_str()),
        Some("My Title"),
        "leading # H1 is the Document Title, not a body paragraph"
    );
}

#[test]
fn title_less_document_is_faithful() {
    let md = "First paragraph.\n\nSecond paragraph.\n";
    check_faithful(&MarkdownFormat, md).unwrap();
    let doc = MarkdownFormat.parse(md).unwrap();
    assert!(doc.title.is_none(), "heading-less source has no title");
    assert!(
        doc.annotations
            .iter()
            .any(|a| a.data.label.value == "doc.untitled"),
        "heading-less source carries the :: doc.untitled :: marker"
    );
}

// ============================================================================
// DECLARED-LOSSY DEGRADE: backtick inside a code span (acceptance criterion 3)
//
// Markdown's double-backtick code span `` `` a`b `` `` can contain a literal
// backtick; a Lex code span is single-backtick delimited and literal, so it
// CANNOT represent an embedded backtick. This is Declared Lossy — the code-span
// *markup* is dropped. The requirement is a PREDICTABLE degrade: the produced
// Lex must re-parse to well-formed text, never corrupt structure.
// ============================================================================

#[test]
fn backtick_in_code_span_degrades_predictably() {
    let md = "Use `` a`b `` here.\n";
    let doc = MarkdownFormat.parse(md).unwrap();
    let lex = LexFormat::default();
    let lex_text = lex.serialize(&doc).unwrap();

    // The degrade must not corrupt structure: the Lex re-parses cleanly...
    let reparsed = lex
        .parse(&lex_text)
        .unwrap_or_else(|e| panic!("degraded Lex did not re-parse: {e}\n{lex_text}"));

    // ...into exactly one body Paragraph (well-formed text), not a spurious
    // verbatim / definition / broken nesting, and the text content survives
    // (the backtick chars become literal text — no data lost to corruption).
    let paragraphs: Vec<&str> = reparsed
        .root
        .children
        .iter()
        .filter_map(|c| match c {
            ContentItem::Paragraph(p) => Some(p),
            _ => None,
        })
        .flat_map(|p| p.lines.iter())
        .filter_map(|l| match l {
            ContentItem::TextLine(tl) => Some(tl.content.as_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        reparsed
            .root
            .children
            .iter()
            .filter(|c| !matches!(c, ContentItem::BlankLineGroup(_)))
            .count(),
        1,
        "degrade must yield exactly one body block, got:\n{lex_text}"
    );
    assert_eq!(paragraphs, vec!["Use `a`b` here."]);

    // And it is Skeleton-faithful at the text level: dropping the code-span
    // markup does not change the compared text, so this particular degrade
    // happens to survive `canon` too (the guarantee is non-corruption, not
    // round-trip; here both hold).
    assert_eq!(canon(&doc), canon(&reparsed));
}

// ============================================================================
// lex#798 — reader-built lists round-trip (unordered / ordered / nested /
// multi-block), driven through the real Markdown reader. Each item comrak builds
// is `ListItem { text: "", children: [Paragraph, …] }`; the serializer hoists the
// leading paragraph onto the tight `- text` marker line so it re-parses as a
// list. Inputs are minimal in-test Markdown literals (no new .lex fixtures).
// ============================================================================

#[test]
fn unordered_list_round_trips() {
    check_faithful(&MarkdownFormat, &format!("{H1}- one\n- two\n- three\n")).unwrap();
}

#[test]
fn ordered_list_keeps_numbers_and_round_trips() {
    let md = format!("{H1}1. first\n2. second\n3. third\n");
    check_faithful(&MarkdownFormat, &md).unwrap();
    // The numbers must survive: a reader-built ordered list used to lose them to
    // the plain dash because the List node carried no decoration style.
    let lex = LexFormat::default()
        .serialize(&MarkdownFormat.parse(&md).unwrap())
        .unwrap();
    assert!(
        lex.contains("1. first") && lex.contains("2. second"),
        "ordered markers lost on serialize:\n{lex}"
    );
}

#[test]
fn nested_list_round_trips() {
    // Two-plus levels of nesting; sub-items stay under their parents.
    let md = format!("{H1}- a\n    - b\n        - c\n        - c2\n    - b2\n- d\n");
    check_faithful(&MarkdownFormat, &md).unwrap();
}

#[test]
fn ordered_outer_with_nested_bullets_round_trips() {
    let md = format!("{H1}1. first\n    - bullet\n    - bullet2\n2. second\n");
    check_faithful(&MarkdownFormat, &md).unwrap();
}

#[test]
fn multi_block_list_item_round_trips() {
    // A genuinely multi-block item: lead paragraph + a nested paragraph + a
    // sublist + a trailing paragraph. These must stay in the indented body (only
    // the single leading paragraph is hoisted to the marker line), and the whole
    // thing must round-trip.
    let md = format!(
        "{H1}- First item\n\n    Nested paragraph.\n\n    - sub a\n    - sub b\n\n    Trailing paragraph.\n\n- Second item\n"
    );
    check_faithful(&MarkdownFormat, &md).unwrap();
}

#[test]
fn single_item_list_degrades_to_paragraph() {
    // Declared Lossy (per list.lex): a Lex list needs >= 2 items, so a single
    // `- x` is prose, not a list. comrak builds a one-item list; the serializer
    // emits the tight `- x`, which lex-core re-parses as a Paragraph. The degrade
    // must be predictable — content preserved, well-formed, no corruption — not a
    // faithful round-trip.
    let md = format!("{H1}- only one\n");
    let doc = MarkdownFormat.parse(&md).unwrap();
    let lex = LexFormat::default().serialize(&doc).unwrap();
    let reparsed = LexFormat::default()
        .parse(&lex)
        .unwrap_or_else(|e| panic!("degraded Lex did not re-parse: {e}\n{lex}"));
    let body: Vec<&ContentItem> = reparsed
        .root
        .children
        .iter()
        .filter(|c| !matches!(c, ContentItem::BlankLineGroup(_)))
        .collect();
    assert_eq!(body.len(), 1, "expected a single body block, got:\n{lex}");
    match body[0] {
        ContentItem::Paragraph(p) => assert!(
            p.text().contains("only one"),
            "single-item content lost in degrade:\n{lex}"
        ),
        other => panic!("expected the one-item list to degrade to a Paragraph, got {other:?}"),
    }
}

#[test]
fn nested_single_item_degrades_but_outer_list_survives() {
    // The >= 2-item rule (list.lex) applies at EVERY level: a one-item *nested*
    // list is prose too. So `- a / <indent>- only / - b` keeps the two-item outer
    // list, and item `a`'s single nested item degrades to a Paragraph in its body
    // — a clean, non-corrupting degrade (the outer structure is preserved), not a
    // #798 list-collapse. This pins the boundary between #798 (fixed) and the
    // 2-item Declared-Lossy rule.
    let md = format!("{H1}- a\n    - only\n- b\n");
    let doc = MarkdownFormat.parse(&md).unwrap();
    let lex = LexFormat::default().serialize(&doc).unwrap();
    let reparsed = LexFormat::default()
        .parse(&lex)
        .unwrap_or_else(|e| panic!("degraded Lex did not re-parse: {e}\n{lex}"));

    let outer = reparsed
        .root
        .children
        .iter()
        .find_map(|c| match c {
            ContentItem::List(l) => Some(l),
            _ => None,
        })
        .unwrap_or_else(|| panic!("outer list must survive, got:\n{lex}"));
    assert_eq!(
        outer.items.len(),
        2,
        "outer list should keep 2 items:\n{lex}"
    );

    let first = outer.items.iter().next().unwrap();
    match first {
        ContentItem::ListItem(li) => {
            assert!(
                li.children
                    .iter()
                    .all(|c| !matches!(c, ContentItem::List(_))),
                "the one-item nested list should degrade to prose, not stay a List:\n{lex}"
            );
            let has_only = li.children.iter().any(|c| match c {
                ContentItem::Paragraph(p) => p.text().contains("only"),
                _ => false,
            });
            assert!(has_only, "nested single-item content lost:\n{lex}");
        }
        other => panic!("expected a ListItem, got {other:?}"),
    }
}

// ============================================================================
// #795 — MARKER-LIKE SESSION TITLES
//
// A style-less session whose title text begins with a marker-like token
// (`1.`, `a)`, `IV.`, `(1)`, `1.2.3`) must survive serialize→re-parse with
// `style: None`. The serializer escapes the marker's structural separator
// (escaping.lex §3.4) only when the session carries no explicit marker, and
// lex-core strips that guard on re-parse. Genuine markers are untouched.
// ============================================================================

/// Every marker-like Markdown heading round-trips faithfully — the reader builds
/// a `style: None` session, and the escaped Lex re-parses to the same skeleton
/// (checked directly by `check_faithful`). Markdown headings carry no decoration
/// style, so each of these reads as `style: None`.
#[test]
fn marker_like_headings_round_trip() {
    for heading in [
        "## 1\\. Numbered",
        "## IV\\. Roman",
        "## a\\. Alpha",
        "## 1\\)  Paren",
        "## \\(1) Double paren",
        "## 1\\.2.3 Extended",
        // A dash-leading heading is never a session marker (sessions reject dash
        // markers), so it round-trips without needing a guard.
        "## - Dash leading",
        // A heading with no marker-like token must be unaffected.
        "## Plain Heading",
    ] {
        let src = format!("# Doc\n\n{heading}\n\nBody text.\n");
        check_faithful(&MarkdownFormat, &src)
            .unwrap_or_else(|e| panic!("heading {heading:?} not faithful:\n{e}"));
    }
}

/// The reader shape built directly: a `Session { marker: None, title: "1. X" }`
/// (exactly what the Markdown reader produces for `## 1. X`). It must serialize
/// to an escaped form and re-parse with `style: None` and its title text intact.
#[test]
fn reader_shaped_marker_title_reparses_style_none() {
    use lex_core::lex::ast::elements::container::SessionContainer;
    use lex_core::lex::ast::elements::typed_content::{ContentElement, SessionContent};
    use lex_core::lex::ast::{Document, Paragraph, Session, TextContent};

    let session = Session::new(
        TextContent::from_string("1. Primary".to_string(), None),
        vec![SessionContent::Element(ContentElement::Paragraph(
            Paragraph::from_line("Body.".to_string()),
        ))],
    );
    assert!(
        session.marker.is_none(),
        "reader builds a style-less session"
    );

    let mut doc = Document::new();
    doc.root.children = SessionContainer::from_typed(vec![SessionContent::Session(session)]);

    let lex = LexFormat::default();
    let out = lex.serialize(&doc).expect("serialize");
    assert!(
        out.contains("1\\. Primary"),
        "the style-less marker-like title must be escaped:\n{out}"
    );

    let reparsed = lex.parse(&out).expect("reparse");
    let Some(ContentItem::Session(s)) = reparsed.root.children.iter().next() else {
        panic!("expected a Session, got:\n{out}");
    };
    assert!(
        s.marker.is_none(),
        "re-parsed session must stay style-less, got {:?}\n{out}",
        s.marker
    );
    assert_eq!(s.title.as_string(), "1. Primary", "title text preserved");

    // And the whole document is Skeleton-faithful.
    assert_eq!(canon(&doc), canon(&reparsed));
}

/// lex → lex byte-identity: a genuinely numbered session keeps its real marker
/// (no spurious escape), and a hand-written escaped title round-trips verbatim.
#[test]
fn lex_sourced_sessions_are_byte_identical() {
    let lex = LexFormat::default();
    for source in [
        // Genuine numbered session — the marker is real, must not be escaped.
        "1. Real Session\n\n    Body.\n",
        // Genuine alphabetical/roman/parenthetical.
        "a. Alpha Session\n\n    Body.\n",
        "IV. Roman Session\n\n    Body.\n",
        // Style-less plain title — unaffected.
        "Introduction\n\n    Body.\n",
        // Hand-written escaped marker-like title — re-escaped identically.
        "1\\. Primary\n\n    Body.\n",
    ] {
        let doc = lex.parse(source).expect("parse");
        let out = lex.serialize(&doc).expect("serialize");
        assert_eq!(out, source, "lex → lex must be byte-identical");
    }
}
