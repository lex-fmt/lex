use super::tables::{build_grid, Slot};
use super::{FormattingRules, LexSerializer};
use crate::format::Format;
use lex_core::lex::ast::text_content::TextContent;
use lex_core::lex::ast::traits::AstNode;
use lex_core::lex::ast::{ContentItem, TableCell, TableRow};
use lex_core::lex::testing::lexplore::{ElementType, Lexplore};
use lex_core::lex::testing::text_diff::assert_text_eq;

/// Drive the bare `LexSerializer` directly. NOTE: this bypasses
/// `LexFormat::serialize`'s pre/post passes (annotation inlining, blank
/// coalescing, trailing-blank trim), so its output can differ from
/// `lexd format`. Use `format_full` when the full pipeline matters (e.g.
/// annotation cases). Kept for the many element tests that assert the
/// serializer's raw structural output.
fn format_source(source: &str) -> String {
    let format = super::super::LexFormat::default();
    let doc = format.parse(source).unwrap();
    let rules = FormattingRules::default();
    let mut serializer = LexSerializer::new(rules);
    doc.accept(&mut serializer);
    serializer.output
}

/// Format through the full `LexFormat` pipeline (annotation inlining +
/// blank coalescing), i.e. what `lexd format` actually does — as opposed to
/// driving the bare `LexSerializer`. Needed for annotation cases, where the
/// pipeline strips the empty-paragraph marker artifact.
fn format_full(source: &str) -> String {
    use crate::format::Format;
    let format = super::super::LexFormat::default();
    let doc = format.parse(source).unwrap();
    format.serialize(&doc).unwrap()
}

// ==== Form-preserving roundtrip tests (#584 PR 3) =====================

#[test]
fn shortcut_form_round_trips_to_shortcut_spelling() {
    // `:: author ::` source classifies as form=Shortcut for
    // canonical `lex.metadata.author`. The formatter must emit the
    // shortcut back, not the canonical. (The serializer's
    // single-line-vs-block emission is a separate concern; this
    // test focuses on the label-spelling preservation contract.)
    let formatted = format_source(":: author :: Alice\n\nBody.\n");
    assert!(
        formatted.contains(":: author"),
        "shortcut spelling should round-trip; got: {formatted}"
    );
    assert!(
        !formatted.contains("lex.metadata.author"),
        "canonical spelling must not leak into output: {formatted}"
    );
}

#[test]
fn stripped_form_round_trips_to_stripped_spelling() {
    // `:: metadata.category ::` classifies as Stripped — formatter
    // must emit `metadata.category`, not the canonical.
    let formatted = format_source(":: metadata.category :: tech\n\nBody.\n");
    assert!(
        formatted.contains(":: metadata.category"),
        "stripped spelling should round-trip; got: {formatted}"
    );
    assert!(
        !formatted.contains("lex.metadata.category"),
        "canonical spelling must not leak: {formatted}"
    );
}

#[test]
fn canonical_form_round_trips_unchanged() {
    // `:: lex.metadata.title ::` classifies as Canonical and
    // formats back as itself.
    let formatted = format_source(":: lex.metadata.title :: My Doc\n\nBody.\n");
    assert!(
        formatted.contains(":: lex.metadata.title"),
        "canonical spelling should round-trip; got: {formatted}"
    );
}

#[test]
fn community_form_round_trips_unchanged() {
    let formatted = format_source(":: acme.task id=42 :: foo\n\nBody.\n");
    assert!(
        formatted.contains(":: acme.task"),
        "community label should round-trip; got: {formatted}"
    );
}

#[test]
fn verbatim_shortcut_closer_round_trips() {
    // `:: image src=x.png ::` (marker form) classifies as
    // Shortcut for `lex.media.image`. The closing label must
    // emit as `image`, not canonical.
    let formatted = format_source("Photo subject:\n    alt text\n:: image src=\"x.png\" ::\n");
    assert!(
        formatted.contains(":: image"),
        "verbatim closer should preserve shortcut: {formatted}"
    );
    assert!(
        !formatted.contains("lex.media.image"),
        "canonical must not leak: {formatted}"
    );
}

// ==== Paragraph Tests ====

#[test]
fn test_paragraph_01_oneline() {
    let source = Lexplore::load(ElementType::Paragraph, 1).source();
    let formatted = format_source(&source);
    assert_text_eq(
        &formatted,
        "This is a simple paragraph with just one line.\n",
    );
}

#[test]
fn test_paragraph_02_multiline() {
    let source = Lexplore::load(ElementType::Paragraph, 2).source();
    let formatted = format_source(&source);
    assert!(formatted.contains("This is a multi-line paragraph"));
    assert!(formatted.contains("second line"));
    assert!(formatted.contains("third line"));
}

#[test]
fn test_paragraph_03_special_chars() {
    let source = Lexplore::load(ElementType::Paragraph, 3).source();
    let formatted = format_source(&source);
    assert!(formatted.contains("!@#$%^&*()"));
}

// ==== Session Tests ====

#[test]
fn test_session_01_simple() {
    let source = Lexplore::load(ElementType::Session, 1).source();
    let formatted = format_source(&source);
    assert!(formatted.contains("Introduction\n"));
    assert!(formatted.contains("    This is a simple session"));
}

#[test]
fn test_session_02_numbered_title() {
    let source = Lexplore::load(ElementType::Session, 2).source();
    let formatted = format_source(&source);
    assert!(formatted.contains("1. Introduction:\n"));
}

#[test]
fn test_session_05_nested() {
    let source = Lexplore::load(ElementType::Session, 5).source();
    let formatted = format_source(&source);
    // This is actually a complex doc with paragraphs and sessions
    assert!(formatted.contains("1. Introduction {{session-title}}\n"));
    assert!(formatted.contains("    This is the content of the session"));
}

// ==== List Tests ====

#[test]
fn test_list_01_dash() {
    let source = Lexplore::load(ElementType::List, 1).source();
    let formatted = format_source(&source);
    assert!(formatted.contains("- First item\n"));
    assert!(formatted.contains("- Second item\n"));
}

#[test]
fn test_list_02_numbered() {
    let source = Lexplore::load(ElementType::List, 2).source();
    let formatted = format_source(&source);
    // Should normalize to sequential numbering
    assert!(formatted.contains("1. "));
    assert!(formatted.contains("2. "));
    assert!(formatted.contains("3. "));
}

#[test]
fn test_list_03_alphabetical() {
    let source = Lexplore::load(ElementType::List, 3).source();
    let formatted = format_source(&source);
    assert!(formatted.contains("a. "));
    assert!(formatted.contains("b. "));
    assert!(formatted.contains("c. "));
}

#[test]
fn test_list_04_mixed_markers() {
    let source = Lexplore::load(ElementType::List, 4).source();
    let formatted = format_source(&source);
    // Should normalize to consistent markers
    assert!(formatted.contains("1. First item\n"));
    assert!(formatted.contains("2. Second item\n"));
    assert!(formatted.contains("3. Third item\n"));
}

#[test]
fn test_list_07_nested_simple() {
    let source = Lexplore::load(ElementType::List, 7).source();
    let formatted = format_source(&source);
    // Check for proper indentation of nested items
    assert!(formatted.contains("- First outer item\n"));
    assert!(formatted.contains("    - First nested item\n"));
}

#[test]
fn test_list_extended_markers_preserved() {
    // NOTE: Extended markers (e.g., "1.2.3") require core parser support
    // for Form::Extended. Currently the parser treats them as standard
    // numbered lists, so normalization produces "1.", "2.", etc.
    let source = "1.2.3 Item one\n1.2.4 Item two\n";
    let formatted = format_source(source);
    assert!(formatted.contains("1. Item one\n"));
    assert!(formatted.contains("2. Item two\n"));
}

#[test]
fn test_list_extended_markers_nested_normalization() {
    // Nested list with extended markers: formatter should rebuild hierarchical markers
    let source = "Test:\n\n1. Outer level one\n    1.a Middle level one\n        1.a.1 Inner level one\n        1.a.2 Inner level two\n    1.b Middle level two\n2. Outer level two\n";
    let formatted = format_source(source);
    // Outer level items
    assert!(
        formatted.contains("1. Outer level one"),
        "Expected '1. Outer level one' in: {formatted}"
    );
    assert!(
        formatted.contains("2. Outer level two"),
        "Expected '2. Outer level two' in: {formatted}"
    );
}

#[test]
fn test_list_12_extended_form_fixture() {
    let source = Lexplore::load(ElementType::List, 12).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

// ==== Definition Tests ====

#[test]
fn test_definition_01_simple() {
    let source = Lexplore::load(ElementType::Definition, 1).source();
    let formatted = format_source(&source);
    assert!(formatted.contains("Cache:\n"));
    assert!(formatted.contains("    Temporary storage"));
}

#[test]
fn test_definition_02_multi_paragraph() {
    let source = Lexplore::load(ElementType::Definition, 2).source();
    let formatted = format_source(&source);
    // Should handle multiple paragraphs in definition body
    assert!(formatted.contains("Microservice:\n"));
    assert!(formatted.contains("    An architectural style"));
    assert!(formatted.contains("    Each service is independently"));
}

// ==== Verbatim Tests ====

#[test]
fn test_verbatim_01_simple_code() {
    let source = Lexplore::load(ElementType::Verbatim, 1).source();
    let formatted = format_source(&source);
    assert!(formatted.contains(":: javascript"));
    assert!(formatted.contains("function hello()"));
}

#[test]
fn test_verbatim_02_with_caption() {
    let source = Lexplore::load(ElementType::Verbatim, 2).source();
    let formatted = format_source(&source);
    // Should preserve verbatim content and captions
    assert!(formatted.contains("API Response:"));
}

// ==== Annotation Tests ====

#[test]
fn test_annotation_01_marker_simple() {
    let source = Lexplore::load(ElementType::Annotation, 1).source();
    let formatted = format_full(&source);
    // Marker annotation: closed `:: label ::` form (the open form is invalid
    // and dropped on re-parse — lex#682).
    assert_eq!(formatted, ":: note ::\n");
}

#[test]
fn test_annotation_02_with_params() {
    let source = Lexplore::load(ElementType::Annotation, 2).source();
    let formatted = format_full(&source);
    assert_eq!(formatted, ":: warning severity=high ::\n");
}

#[test]
fn test_annotation_multi_param_keeps_comma_separator() {
    // The parser uses the comma as the only parameter separator, so a
    // space-only join collapses `k1=v1, k2=v2` into a single value on
    // re-parse (lex#703). The formatted output must keep the comma and be a
    // fixed point.
    let formatted = format_full(":: warning type=critical, id=123 ::\n");
    assert_eq!(formatted, ":: warning type=critical, id=123 ::\n");
    assert_eq!(format_full(&formatted), formatted, "must be idempotent");
}

#[test]
fn test_annotation_05_block_paragraph() {
    let source = Lexplore::load(ElementType::Annotation, 5).source();
    let formatted = format_full(&source);
    assert_eq!(
        formatted,
        ":: note ::\n    This is an important note that requires a detailed explanation.\n"
    );
}

// ==== Round-trip Tests ====
// Format → parse → format should be idempotent

#[test]
fn test_round_trip_paragraph_01() {
    let source = Lexplore::load(ElementType::Paragraph, 1).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_paragraph_02_multiline() {
    let source = Lexplore::load(ElementType::Paragraph, 2).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_session_01() {
    let source = Lexplore::load(ElementType::Session, 1).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_session_02_numbered() {
    let source = Lexplore::load(ElementType::Session, 2).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_list_01_dash() {
    let source = Lexplore::load(ElementType::List, 1).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_list_02_numbered() {
    let source = Lexplore::load(ElementType::List, 2).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_list_03_alphabetical() {
    let source = Lexplore::load(ElementType::List, 3).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_list_04_mixed_markers() {
    let source = Lexplore::load(ElementType::List, 4).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_list_07_nested() {
    let source = Lexplore::load(ElementType::List, 7).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_definition_01() {
    let source = Lexplore::load(ElementType::Definition, 1).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_definition_02_multi() {
    let source = Lexplore::load(ElementType::Definition, 2).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_verbatim_01() {
    let source = Lexplore::load(ElementType::Verbatim, 1).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_round_trip_verbatim_02_caption() {
    let source = Lexplore::load(ElementType::Verbatim, 2).source();
    let formatted = format_source(&source);
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_verbatim_03_table_formatting() {
    // PR 2 of #584 retired the legacy verbatim-with-markdown-body
    // path: `:: doc.table ::` is forbidden, and `:: lex.tabular.table ::`
    // / `:: tabular.table ::` source no longer round-trips through
    // a markdown reformatter. The only surviving path is the
    // structural Table element triggered by the bare `:: table ::`
    // closer — `LexSerializer::visit_table` emits the pipe table
    // directly with column alignment.
    let source = "Table Example:\n    | A | B |\n    |---|---|\n    | 1 | 2 |\n:: table ::\n";
    let formatted = format_source(source);

    // Column-aligned pipe-table output from visit_table.
    assert!(formatted.contains("| A   | B   |"));
    assert!(formatted.contains("| --- | --- |"));
    assert!(formatted.contains("| 1   | 2   |"));

    // Also test with unformatted input — visit_table normalises.
    let unformatted = "Table Example:\n    |A|B|\n    |-|-|\n    |1|2|\n:: table ::\n";
    let formatted_2 = format_source(unformatted);

    // Should be formatted nicely
    assert!(formatted_2.contains("| A   | B   |"));
    assert!(formatted_2.contains("| --- | --- |"));
    assert!(formatted_2.contains("| 1   | 2   |"));
}

#[test]
fn test_round_trip_paragraph_then_verbatim_lex505() {
    // Regression: Verbatim preceded by a paragraph must keep its leading
    // blank line through a parse → serialize → parse round-trip. Without
    // the blank, the re-parser merges the verbatim's subject line into
    // the prior paragraph and the verbatim is lost. The parser consumes
    // that blank as part of the verbatim's preamble (it doesn't appear
    // as a BlankLineGroup in the AST), so the serializer has to emit it.
    //
    // Uses a `Title\n=====\n` header so the first line isn't absorbed as
    // the document title — without it, "Intro paragraph." would become
    // the doc title and the regression wouldn't be exercised.
    let source =
        "Doc\n===\n\nIntro paragraph.\n\nCode Example:\n\n    fn main() {}\n\n:: rust ::\n";
    let formatted = format_source(source);
    assert!(
        formatted.contains("Intro paragraph.\n\nCode Example:"),
        "expected blank line between paragraph and verbatim subject, got:\n{formatted}"
    );
    let formatted_again = format_source(&formatted);
    assert_text_eq(&formatted, &formatted_again);
}

#[test]
fn test_verbatim_04_user_repro() {
    // The original user input had dedented marker "::  doc.table ::"
    // which caused parse-as-Definition + Document Annotation. The
    // fix is to indent the marker to match the subject. Updated
    // for PR 2 of #584: source uses the blessed `table` closer
    // which triggers structural-Table parsing; the legacy verbatim
    // path with markdown reformat is gone.
    let source = "  The Table:\n    | Markup Language | Great |\n    |--------------------|--------|\n    | Markdown | No |\n    | Lex | Yes |\n  ::  table ::\n";

    let formatted = format_source(source);

    let table_start = formatted
        .find("| Markup Language | Great |")
        .expect("Table start not found");
    let separator = formatted
        .find("| --------------- | ----- |")
        .expect("Separator not found");
    // PR 3 of #584 wired form-preserving emission: the `:: table ::`
    // source classifies as Shortcut, so the emitted closer is also
    // `:: table ::`, not the canonical `:: lex.tabular.table ::`.
    let footer_start = formatted.find(":: table ::").expect("Footer not found");

    assert!(table_start < separator);
    assert!(separator < footer_start);
}

#[test]
fn build_grid_pads_hole_before_trailing_rowspan() {
    // Regression for the lex#694 review: a short continuation row whose cells
    // run out before a rowspan-covered column further right must still emit
    // that column's `^^` (and not let the carry leak into the next row).
    // row0: a, b, c(rowspan 2) — c spans down into row1's third column.
    // row1: a single cell `d`; the middle column is a hole, the third is `^^`.
    let tc = |s: &str| TextContent::from_string(s.to_string(), None);
    let row0 = TableRow::new(vec![
        TableCell::new(tc("a")),
        TableCell::new(tc("b")),
        TableCell::new(tc("c")).with_span(1, 2),
    ]);
    let row1 = TableRow::new(vec![TableCell::new(tc("d"))]);
    let grid = build_grid(&[&row0, &row1]);
    let render = |slots: &[Slot]| {
        slots
            .iter()
            .map(|s| s.text().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(render(&grid[0]), ["a", "b", "c"]);
    assert_eq!(
        render(&grid[1]),
        ["d", "", "^^"],
        "hole padded empty, trailing rowspan marker kept"
    );
}

// ==== Leading-annotation / title ordering (lex#791) ====

#[test]
fn test_leading_annotation_stays_above_title_with_subtitle() {
    // A document-level annotation authored *before* the title must serialize
    // above it, not be hoisted below the title/subtitle (lex#791). The title is
    // a first-class node parked outside the body stream; the serializer must
    // still emit it at its source position relative to the leading annotations.
    let source = "\
:: author :: Yuval Noah Harari

Sapiens:
A Brief History of Humankind

This document has annotations before the title with subtitle.
";
    let formatted = format_full(source);
    let author_at = formatted.find(":: author").expect("annotation emitted");
    let title_at = formatted.find("Sapiens:").expect("title emitted");
    assert!(
        author_at < title_at,
        "leading annotation must stay above the title; got:\n{formatted}"
    );
    assert_eq!(
        format_full(&formatted),
        formatted,
        "leading-annotation/title ordering must be idempotent"
    );
}

#[test]
fn test_multiple_leading_annotations_keep_first_paragraph_below() {
    // Several leading document-level annotations followed by the first body
    // paragraph (which the parser adopts as the title). The paragraph must not
    // jump above the annotation run on serialize (lex#791).
    let source = "\
:: foo ::

:: bar ::

Some text here.

There is something in the way she moves.
";
    let formatted = format_full(source);
    let foo_at = formatted.find(":: foo").expect("foo emitted");
    let bar_at = formatted.find(":: bar").expect("bar emitted");
    let text_at = formatted
        .find("Some text here.")
        .expect("paragraph emitted");
    assert!(
        foo_at < bar_at && bar_at < text_at,
        "leading annotations must stay above the first paragraph; got:\n{formatted}"
    );
    assert_eq!(
        format_full(&formatted),
        formatted,
        "ordering must be idempotent"
    );
}

#[test]
fn test_title_without_leading_annotations_stays_at_head() {
    // No leading annotation: the title still leads and a blank separates it from
    // the body (the pre-existing head behavior must be unchanged).
    let formatted = format_full("Sapiens:\nA Brief History of Humankind\n\nBody line.\n");
    assert_eq!(
        formatted,
        "Sapiens:\nA Brief History of Humankind\n\nBody line.\n"
    );
}

#[test]
fn verbatim_subject_with_trailing_space_round_trips_lex790() {
    // Regression for lex#790: a verbatim (group) subject whose source colon is
    // followed by trailing whitespace must NOT be re-serialized with a doubled
    // colon. The bug stored the subject as "The Tower of Babel:" (colon kept,
    // because a trailing-whitespace token pushed the bounding box past the colon),
    // and the serializer's `{subject}:` then produced "The Tower of Babel::",
    // which re-parses as a plain paragraph and the verbatim is lost.
    let source = "Doc\n===\n\nThe Tower of Babel: \n    Body line one.\n:: image ref=x.jpg ::\n";
    let formatted = format_source(source);
    assert!(
        formatted.contains("The Tower of Babel:\n"),
        "subject must keep exactly one colon; got:\n{formatted}"
    );
    assert!(
        !formatted.contains("Babel::"),
        "subject colon must not be doubled; got:\n{formatted}"
    );
    // And the verbatim survives a reparse (idempotent second format).
    let again = format_source(&formatted);
    assert_text_eq(&formatted, &again);
}

#[test]
fn multiline_table_cell_stacks_and_round_trips_lex790() {
    // Regression for lex#790: a multi-line table cell (stacked pipe-line group
    // in the source) must serialize back as stacked pipe rows separated by a
    // blank line — not with the cell's embedded newline dumped inline, which
    // splits the pipe row and collapses the whole table into prose on reparse.
    let source = "Log:\n    | Trial   | Result         |\n\n    | Trial 1 | Successful     |\n    |         | after 48 hours |\n\n    | Trial 2 | No growth      |\n:: table ::\n";
    let formatted = format_source(source);

    // The multi-line cell is emitted as two physical rows within one group, and
    // the continuation line's first column is empty padding.
    assert!(
        formatted.contains("| Trial 1 | Successful     |"),
        "first physical line of the multi-line row missing:\n{formatted}"
    );
    assert!(
        formatted.contains("|         | after 48 hours |"),
        "continuation line of the multi-line cell missing:\n{formatted}"
    );
    // No raw newline inside a pipe row: every non-blank output line that starts
    // with `|` also ends with `|`.
    for line in formatted.lines() {
        let t = line.trim();
        if t.starts_with('|') {
            assert!(
                t.ends_with('|'),
                "pipe row split across lines (lex#790):\n{formatted}"
            );
        }
    }

    // The table structure survives a reparse: a Table node is still present (the
    // bug collapsed it into loose paragraphs), with one header and two body rows
    // and the multi-line cell content intact.
    let format = super::super::LexFormat::default();
    let doc = format.parse(&formatted).unwrap();
    let table = doc
        .root
        .children
        .iter()
        .find_map(|item| match item {
            ContentItem::Table(t) => Some(t),
            _ => None,
        })
        .expect("table must survive round-trip, not collapse to paragraphs");
    assert_eq!(table.header_rows.len(), 1);
    assert_eq!(table.body_rows.len(), 2);
    assert_eq!(
        table.body_rows[0].cells[1].content.as_string(),
        "Successful\nafter 48 hours",
        "multi-line cell content must survive the round-trip"
    );

    // Idempotent second format.
    let again = format_source(&formatted);
    assert_text_eq(&formatted, &again);
}
