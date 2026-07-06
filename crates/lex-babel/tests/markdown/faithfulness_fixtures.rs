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
//! reference, …) currently FAILS end-to-end Faithfulness through the real reader.
//! The empirical causes, all **pre-existing** and confirmed by minimal repros:
//!
//!   - **lex#798 (reader-built loose lists collapse on serialize)** — the dominant
//!     blocker. The Markdown reader builds every list item as
//!     `ListItem { text: "", children: [Paragraph] }` (comrak's block model — an
//!     item contains a paragraph). The Lex serializer emits that as the loose form
//!     `-\n    text`, which lex-core does **not** re-parse as a list — it collapses
//!     the whole list into one Paragraph blob. So NO reader-built list round-trips;
//!     since every real fixture contains lists, this alone sinks all of them. Same
//!     root *mechanism* as #790 (a nested block body under a marker collapses to a
//!     paragraph) but a distinct, separately-tracked manifestation — #790's title
//!     and repros name only verbatim/definition/table.
//!   - **lex#790 (verbatim/definition nested bodies de-indent)** — a definition
//!     body carrying a paragraph+list, and a colon-terminated paragraph before a
//!     verbatim (the paragraph is absorbed as the verbatim subject and the body
//!     de-indents). Present in comrak-readme / comrak-reference on top of #798.
//!   - **lex#795 (session-marker inference)** — a heading whose text begins with a
//!     marker-like token (`## 1\. Primary Session` in kitchensink) serializes to
//!     `1. Primary Session`, which re-parses with a Numerical session marker where
//!     the reader had `style: None`. A distinct axis; filed by this slice.
//!   - **lex#791 (leading-annotation reorder)** — `20-ideas-naked.md`'s leading
//!     document-level annotations reorder around the title on serialize.
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
/// bug, mapped to the dominant tracked issue. See the module docs for the full
/// per-fixture cause analysis (kitchensink is additionally blocked by #795).
///
/// DO NOT clear an entry by weakening `canon` — fix the referenced bug. The
/// sweep fails loudly if a listed fixture starts passing (promote it) or an
/// unlisted one starts failing (a new regression).
const FIXTURE_KNOWN_FAIL: &[(&str, &str)] = &[
    // #798 — reader-built loose lists collapse on serialize (every fixture has
    // lists, so #798 blocks them all). kitchensink ALSO hits #795 (numbered
    // heading); comrak-readme / comrak-reference ALSO hit #790 (colon-para →
    // verbatim-subject absorption). See the module docs for the full analysis.
    ("kitchensink", "lex#798 (+ lex#795)"),
    ("comrak-readme", "lex#798 (+ lex#790)"),
    ("commonmark-reference", "lex#798"),
    ("comrak-reference", "lex#798 (+ lex#790)"),
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
// Empirically today: para→para, heading→body, para→verbatim, para→definition
// round-trip faithfully; para→list and list→para do NOT (blocked by lex#798 —
// reader-built loose lists collapse on serialize). The blocked pair is asserted
// as a currently-failing known gap rather than skipped, with the same anti-rot
// as the fixture sweep.
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

/// paragraph→list and list→paragraph are the two adjacency pairs blocked by
/// lex#798. Asserted as currently-failing (not skipped): the Markdown reader
/// builds loose list items (`ListItem { text: "", children: [Paragraph] }`)
/// which the serializer emits as `-\n    text` — a form lex-core re-parses as a
/// paragraph, collapsing the list. When #798 is fixed these round-trip and this
/// test fails, prompting promotion to plain `check_faithful(...).unwrap()`.
#[test]
fn adjacency_list_pairs_blocked_by_lex798() {
    let para_to_list = format!("{H1}Lead in.\n\n- one\n- two\n");
    let list_to_para = format!("{H1}- one\n- two\n\nTrailer.\n");
    for (name, md) in [
        ("paragraph->list", &para_to_list),
        ("list->paragraph", &list_to_para),
    ] {
        assert!(
            check_faithful(&MarkdownFormat, md).is_err(),
            "{name} now round-trips faithfully — lex#798 appears fixed; \
             promote this to a live `check_faithful(...).unwrap()` adjacency assertion"
        );
    }
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
