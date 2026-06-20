//! Table-driven tests for smart paste (comms#73, spec §6).
//!
//! Three layers: the pure [`reanchor`] whitespace transform, classification +
//! anchor resolution against a real parse, and end-to-end [`prepare_paste`]
//! covering every §6 edge case.

use super::*;
use lex_core::lex::parsing::parse_document;

fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

fn empty_range(line: u32, character: u32) -> Range {
    let p = pos(line, character);
    Range { start: p, end: p }
}

// ---------------------------------------------------------------------------
// The pure re-anchor transform (§4.2–§4.4).
// ---------------------------------------------------------------------------

/// Each case: (name, pasted_text, anchor, fresh_line, expected).
struct ReanchorCase {
    name: &'static str,
    pasted: &'static str,
    anchor: usize,
    fresh_line: bool,
    expected: &'static str,
}

#[test]
fn reanchor_table() {
    let cases = [
        ReanchorCase {
            name: "zero delta — baseline already matches anchor",
            pasted: "    parent\n        child\n",
            anchor: 4,
            fresh_line: true,
            expected: "    parent\n        child\n",
        },
        ReanchorCase {
            name: "positive delta — pasting deeper",
            pasted: "parent\n    child\n",
            anchor: 8,
            fresh_line: true,
            expected: "        parent\n            child\n",
        },
        ReanchorCase {
            name: "negative delta — pasting shallower, clamps at zero",
            pasted: "        parent\n            child\n",
            anchor: 0,
            fresh_line: true,
            expected: "parent\n    child\n",
        },
        ReanchorCase {
            name: "negative delta deeper than some lines — per-line clamp",
            // baseline = 4 (parent), delta = 0 - 4 = -4. parent -> 0, child(8) -> 4.
            pasted: "    parent\n        child\n",
            anchor: 0,
            fresh_line: true,
            expected: "parent\n    child\n",
        },
        ReanchorCase {
            name: "blank lines stay empty, never padded",
            pasted: "parent\n\n    child\n",
            anchor: 4,
            fresh_line: true,
            expected: "    parent\n\n        child\n",
        },
        ReanchorCase {
            name: "whitespace-only line emitted empty",
            pasted: "parent\n   \n    child",
            anchor: 4,
            fresh_line: true,
            expected: "    parent\n\n        child",
        },
        ReanchorCase {
            name: "merge first line — strip ws, no anchor; rest re-anchored",
            pasted: "joined\n    second\n",
            anchor: 8,
            fresh_line: false,
            expected: "joined\n            second\n",
        },
        ReanchorCase {
            name: "merge first line that was itself indented",
            pasted: "    joined\n        second\n",
            anchor: 4,
            fresh_line: false,
            // baseline = 4, delta = 0. line1 at baseline -> "joined"; line2 (8) -> 8.
            expected: "joined\n        second\n",
        },
        ReanchorCase {
            name: "merge first line deeper than baseline preserves relative indent",
            // gemini refinement (#2): baseline = min(8, 4) = 4. delta = anchor(8)
            // - baseline(4) = +4. line1 (8) merges -> strip only to baseline,
            // keep 8-4=4 relative spaces ("    joined"); line2 (4) re-anchors:
            // 4 + delta(4) = 8.
            pasted: "        joined\n    second\n",
            anchor: 8,
            fresh_line: false,
            expected: "    joined\n        second\n",
        },
        ReanchorCase {
            name: "merge single deeper-than-baseline line keeps relative indent",
            // A one-line merge whose sole line is indented: baseline equals that
            // indent, so relative indent is zero — strips entirely, as before.
            pasted: "        only\n",
            anchor: 4,
            fresh_line: false,
            expected: "only\n",
        },
        ReanchorCase {
            name: "trailing newline preserved",
            pasted: "a\nb\n",
            anchor: 0,
            fresh_line: true,
            expected: "a\nb\n",
        },
        ReanchorCase {
            name: "no trailing newline preserved",
            pasted: "a\nb",
            anchor: 0,
            fresh_line: true,
            expected: "a\nb",
        },
        ReanchorCase {
            name: "mixed tabs and spaces measured in display columns",
            // line1 "\tparent" -> width 4; line2 "\t    child" -> width 8.
            // baseline = 4, anchor 4 -> delta 0; emit spaces: parent at 4, child at 8.
            pasted: "\tparent\n\t    child\n",
            anchor: 4,
            fresh_line: true,
            expected: "    parent\n        child\n",
        },
        ReanchorCase {
            name: "partial indentation carried through offset unchanged",
            // baseline = 2 (the 2-space line), delta = anchor(4) - 2 = +2.
            // line1 (2) -> 4 ; line2 (5) -> 7 (partial indent preserved).
            pasted: "  parent\n     odd\n",
            anchor: 4,
            fresh_line: true,
            expected: "    parent\n       odd\n",
        },
        ReanchorCase {
            name: "clipboard that is itself a verbatim block keeps body shape",
            // subject + indented body + closing label; offset is constant so the
            // body stays well-formed relative to its subject.
            pasted: "code\n    fn main() {}\n:: rust ::\n",
            anchor: 8,
            fresh_line: true,
            expected: "        code\n            fn main() {}\n        :: rust ::\n",
        },
    ];

    for case in cases {
        let got = reanchor(case.pasted, case.anchor, case.fresh_line, 0);
        assert_eq!(
            got, case.expected,
            "case `{}`: got {:?}, expected {:?}",
            case.name, got, case.expected
        );
    }
}

#[test]
fn reanchor_baseline_is_min_over_nonblank_lines() {
    // baseline must ignore blank lines: min(8, 4) = 4, not 0 from the blank.
    let got = reanchor("        deep\n\n    shallow\n", 4, true, 0);
    assert_eq!(got, "        deep\n\n    shallow\n");
}

#[test]
fn reanchor_subtracts_surviving_caret_indent_from_first_line() {
    // comms#73 #3: the editor auto-indented the fresh line to the anchor (4) but
    // its empty range leaves those 4 spaces in the buffer. The first emitted
    // line must drop them (4 - 4 = 0) so the buffer total is the anchor, not
    // double it; line 2 is a brand-new line and takes the full §4.3 treatment.
    let got = reanchor("first\n    second\n", 4, true, 4);
    assert_eq!(got, "first\n        second\n");
}

#[test]
fn reanchor_caret_indent_only_affects_first_line() {
    // A surviving caret indent shallower than the anchor still leaves the rest
    // of the block at full anchor depth: baseline 0, anchor 8, caret_indent 4.
    // line1: 8 - 4 = 4 emitted (+ 4 surviving = 8 total); line2: 4 + 8 = 12.
    let got = reanchor("parent\n    child\n", 8, true, 4);
    assert_eq!(got, "    parent\n            child\n");
}

#[test]
fn reanchor_caret_indent_clamps_when_it_exceeds_target() {
    // Surviving whitespace deeper than the anchor: an insert-only edit cannot
    // remove it, so the first line clamps to no added indent (saturating_sub).
    // Exact dedent here needs the editor to expand the range — the §4.4 contract.
    let got = reanchor("only\n    tail\n", 4, true, 8);
    assert_eq!(got, "only\n        tail\n");
}

// ---------------------------------------------------------------------------
// Fresh-line vs. merge detection (§4.4). `Position.character` is a UTF-8 byte
// offset in this server, so multi-byte content before the caret must not throw
// off the whitespace check.
// ---------------------------------------------------------------------------

#[test]
fn is_fresh_line_blank_line_is_fresh() {
    let source = "Top\n\n    body\n";
    assert!(is_fresh_line(source, pos(1, 0)));
}

#[test]
fn is_fresh_line_after_content_is_merge() {
    let source = "Top\n\n    body\n";
    // Caret after "    body" (byte 8) — content precedes it, so it's a merge.
    assert!(!is_fresh_line(source, pos(2, 8)));
}

#[test]
fn is_fresh_line_multibyte_before_caret_uses_byte_offset() {
    // "    café" — the 'é' is two UTF-8 bytes, so the byte offset of the caret
    // at end-of-content (9) exceeds the char count (8). A char-counting check
    // would stop one char early; a byte-correct check sees the non-whitespace
    // content and reports a merge.
    let source = "Top\n\n    café\n";
    let caret = "    café".len() as u32; // 9 bytes
    assert!(!is_fresh_line(source, pos(2, caret)));
}

#[test]
fn is_fresh_line_whitespace_only_prefix_with_later_multibyte_is_fresh() {
    // Caret sits within the leading whitespace; the multi-byte content after it
    // must not be consumed (we stop at the caret byte offset).
    let source = "Top\n\n    café\n";
    assert!(is_fresh_line(source, pos(2, 4)));
}

#[test]
fn surviving_leading_indent_counts_tabs_in_display_columns() {
    // Auto-indent compensation measures the surviving whitespace in display
    // columns (TAB_WIDTH), not bytes, so it matches the space-based anchor
    // arithmetic for editors that indent with tabs. A single tab before the
    // caret (byte offset 1) is 4 columns.
    let source = "Top\n\n\t\n";
    assert_eq!(surviving_leading_indent(source, pos(2, 1)), 4);
}

#[test]
fn surviving_leading_indent_mixes_tabs_and_spaces() {
    // Tab (advances to column 4) followed by two spaces = 6 columns; the caret
    // is past all three (byte offset 3).
    let source = "Top\n\n\t  \n";
    assert_eq!(surviving_leading_indent(source, pos(2, 3)), 6);
}

#[test]
fn surviving_leading_indent_stops_at_caret() {
    // Only whitespace *before* the caret survives: a caret mid-indent counts
    // just the spaces up to its byte offset, not the rest of the line.
    let source = "Top\n\n        body\n";
    assert_eq!(surviving_leading_indent(source, pos(2, 4)), 4);
}

// ---------------------------------------------------------------------------
// Classification (§3), innermost-first.
// ---------------------------------------------------------------------------

#[test]
fn classify_single_line_merge_passthrough() {
    // Merge case (caret follows existing content): a single line continues the
    // current line, so it stays passthrough — there is no new block to anchor.
    let source = "Top\n\n    body line\n";
    let doc = parse_document(source).expect("parse");
    // Caret at byte 13 — end of "    body line" (4 + 9), a merge position.
    let result = prepare_paste(&doc, source, empty_range(2, 13), "just one line");
    assert_eq!(result.mode, PasteMode::PassthroughSingleLine);
    assert_eq!(result.text, "just one line");
}

#[test]
fn classify_single_line_fresh_reanchors() {
    // Fresh-line case (gemini refinement): a single line dropped onto a
    // blank/whitespace-only prefix IS a new block, so it re-anchors to the
    // caret's structural level rather than passing through verbatim.
    let source = "Top\n\n    existing\n\n";
    let doc = parse_document(source).expect("parse");
    // Blank line 4, inside the session (content indent 4). A column-zero single
    // line re-anchors to 4.
    let result = prepare_paste(&doc, source, empty_range(4, 0), "just one line");
    assert_eq!(result.mode, PasteMode::Reanchor);
    assert_eq!(result.text, "    just one line");
}

#[test]
fn classify_single_line_fresh_dedents_overindented_clipboard() {
    // A single line lifted from deep nesting (absolute indent 8) dropped on a
    // fresh line at a shallower level (anchor 4) is re-anchored down to 4 —
    // exactly the bug smart paste exists to fix, now reached for single lines.
    let source = "Top\n\n    existing\n\n";
    let doc = parse_document(source).expect("parse");
    let result = prepare_paste(&doc, source, empty_range(4, 0), "        deep line");
    assert_eq!(result.mode, PasteMode::Reanchor);
    assert_eq!(result.text, "    deep line");
}

#[test]
fn classify_verbatim_passthrough_wins_over_single_line() {
    // A single-line paste inside a verbatim block reports the structural reason
    // (verbatim), not the incidental one (single-line) — §3 closing note.
    let source = "Code:\n    line one\n    line two\n:: text ::\n";
    let doc = parse_document(source).expect("parse");
    // Caret inside the verbatim body (line 1, the "line one" content).
    let result = prepare_paste(&doc, source, empty_range(1, 8), "x = 1");
    assert_eq!(result.mode, PasteMode::PassthroughVerbatim);
    assert_eq!(result.text, "x = 1");
}

#[test]
fn classify_verbatim_passthrough_multiline_unchanged() {
    let source = "Code:\n    line one\n    line two\n:: text ::\n";
    let doc = parse_document(source).expect("parse");
    let pasted = "  weird\n      indent\n";
    let result = prepare_paste(&doc, source, empty_range(1, 8), pasted);
    assert_eq!(result.mode, PasteMode::PassthroughVerbatim);
    // Indentation is literal content — emitted byte-for-byte.
    assert_eq!(result.text, pasted);
}

#[test]
fn classify_table_passthrough() {
    let source = "Top\n\n    | a | b |\n    | c | d |\n";
    let doc = parse_document(source).expect("parse");
    let pasted = "x\n    y\n";
    let result = prepare_paste(&doc, source, empty_range(2, 8), pasted);
    assert_eq!(result.mode, PasteMode::PassthroughTable);
    assert_eq!(result.text, pasted);
}

#[test]
fn classify_reanchor_in_session_body() {
    let source = "Top\n\n    existing\n";
    let doc = parse_document(source).expect("parse");
    let result = prepare_paste(&doc, source, empty_range(2, 4), "new\n    child\n");
    assert_eq!(result.mode, PasteMode::Reanchor);
}

// ---------------------------------------------------------------------------
// Anchor resolution (§4.1) — derived from the container, not the caret line.
// ---------------------------------------------------------------------------

#[test]
fn anchor_at_document_start_is_zero() {
    let source = "Top session\n\n    body\n";
    let doc = parse_document(source).expect("parse");
    // Range start at column zero, line zero — no enclosing container.
    assert_eq!(resolve_anchor(&doc, source, pos(0, 0)), 0);
}

#[test]
fn anchor_inside_session_is_content_indent() {
    let source = "Top\n\n    body\n";
    let doc = parse_document(source).expect("parse");
    // Anywhere inside the session resolves to its content indent (4).
    assert_eq!(resolve_anchor(&doc, source, pos(2, 4)), 4);
}

#[test]
fn anchor_innermost_wins_for_nested_session() {
    let source = "Top\n\n    Nested\n\n        deep\n";
    let doc = parse_document(source).expect("parse");
    // A caret on the deep content line resolves to the nested session's content
    // indent (8), not the outer session's (4).
    assert_eq!(resolve_anchor(&doc, source, pos(4, 8)), 8);
}

#[test]
fn anchor_ignores_caret_line_whitespace_on_blank_line() {
    // The central §4.1 correction: a blank line left at column zero deep inside a
    // session must still anchor at the session's content indent, not column zero.
    let source = "Top\n\n    Nested\n\n        deep\n\n";
    let doc = parse_document(source).expect("parse");
    // Blank line 5, caret at column 0 — but structurally inside Nested (indent 8).
    let anchor = resolve_anchor(&doc, source, pos(5, 0));
    assert!(
        anchor == 8 || anchor == 4,
        "blank-line anchor should come from an enclosing session, got {anchor}"
    );
}

// ---------------------------------------------------------------------------
// End-to-end edge cases (§6).
// ---------------------------------------------------------------------------

#[test]
fn empty_clipboard_is_noop() {
    let source = "Top\n\n    body\n";
    let doc = parse_document(source).expect("parse");
    let result = prepare_paste(&doc, source, empty_range(2, 4), "");
    assert_eq!(result.text, "");
}

#[test]
fn fresh_line_reanchors_first_line_too() {
    let source = "Top\n\n    existing\n\n";
    let doc = parse_document(source).expect("parse");
    // Fresh (blank) line 4 inside the session; paste a column-zero block.
    let result = prepare_paste(&doc, source, empty_range(4, 0), "first\n    second\n");
    assert_eq!(result.mode, PasteMode::Reanchor);
    // Both lines re-anchored to the session content indent (4).
    assert_eq!(result.text, "    first\n        second\n");
}

#[test]
fn fresh_line_with_auto_indent_does_not_double_anchor() {
    // comms#73 #3, end-to-end: the editor auto-indented the fresh line to the
    // session level (4 spaces) and sends a collapsed caret *after* them — an
    // empty range that does not overwrite the whitespace. The returned first
    // line must carry no anchor of its own (the 4 surviving spaces already are
    // the anchor); without the compensation it would emit 4 more and the spliced
    // line would land at column 8.
    let source = "Top\n\n    existing\n    \n    tail\n";
    let doc = parse_document(source).expect("parse");
    // Line 3 is the whitespace-only auto-indented line; caret at its end (col 4).
    let result = prepare_paste(&doc, source, empty_range(3, 4), "first\n    second\n");
    assert_eq!(result.mode, PasteMode::Reanchor);
    // First line emits no leading space (4 already survive in the buffer);
    // second line re-anchors fully to 8.
    assert_eq!(result.text, "first\n        second\n");
}

#[test]
fn merge_first_line_joins_existing_content() {
    let source = "Top\n\n    existing text\n";
    let doc = parse_document(source).expect("parse");
    // Caret mid-content on line 2 (after "existing "). First pasted line merges;
    // the rest re-anchor to the session content indent (4).
    let result = prepare_paste(&doc, source, empty_range(2, 13), "joined\n    tail\n");
    assert_eq!(result.mode, PasteMode::Reanchor);
    assert_eq!(result.text, "joined\n        tail\n");
}

#[test]
fn merge_first_line_deeper_than_baseline_preserves_relative_indent() {
    // gemini refinement (#2), end-to-end: the first clipboard line is indented
    // deeper than the block baseline. On a merge it must NOT be stripped to bare
    // content — the extra relative indentation (orig - baseline) is preserved.
    let source = "Top\n\n    existing text\n";
    let doc = parse_document(source).expect("parse");
    // Merge caret mid-content on line 2 (byte 13, after "    existing ").
    // Clipboard baseline = min(8, 4) = 4; first line at 8 keeps 8-4 = 4 spaces.
    let result = prepare_paste(
        &doc,
        source,
        empty_range(2, 13),
        "        joined\n    tail\n",
    );
    assert_eq!(result.mode, PasteMode::Reanchor);
    // line1 merge: keep relative indent 4 -> "    joined".
    // line2: anchor 4, baseline 4, delta 0; tail (4) -> 4.
    assert_eq!(result.text, "    joined\n    tail\n");
}

#[test]
fn selection_replace_anchor_from_selection_start() {
    let source = "Top\n\n    Nested\n\n        deep one\n        deep two\n";
    let doc = parse_document(source).expect("parse");
    // Selection spanning the two deep lines; anchor must come from the start.
    let range = Range {
        start: pos(4, 8),
        end: pos(5, 16),
    };
    let result = prepare_paste(&doc, source, range, "a\n    b\n");
    assert_eq!(result.mode, PasteMode::Reanchor);
    // Anchor 8 (nested content indent). The selection starts at column 8, *after*
    // the line's 8 leading spaces — those spaces are not in the replaced range
    // and survive the splice, so the first emitted line carries no anchor of its
    // own (8 - 8 = 0) and lands at column 8 once spliced; line 2 is a fresh line
    // at 8 + delta(8) = 12 (comms#73 #3). Returning "        a" here would
    // double the indent against the surviving spaces.
    assert_eq!(result.text, "a\n            b\n");
}

#[test]
fn paste_at_document_start_zero_baseline_is_identity() {
    let source = "";
    let doc = parse_document(source).expect("parse");
    let result = prepare_paste(&doc, source, empty_range(0, 0), "title\n    body\n");
    assert_eq!(result.mode, PasteMode::Reanchor);
    // Anchor 0, zero baseline -> identity.
    assert_eq!(result.text, "title\n    body\n");
}

#[test]
fn paste_at_document_start_dedents_indented_clipboard() {
    let source = "";
    let doc = parse_document(source).expect("parse");
    // Clipboard lifted from a nesting (baseline 8); anchor 0 dedents it.
    let result = prepare_paste(
        &doc,
        source,
        empty_range(0, 0),
        "        a\n            b\n",
    );
    assert_eq!(result.text, "a\n    b\n");
}

#[test]
fn trailing_newline_preserved_end_to_end() {
    let source = "Top\n\n    body\n\n";
    let doc = parse_document(source).expect("parse");
    let with_nl = prepare_paste(&doc, source, empty_range(4, 0), "a\nb\n");
    assert!(with_nl.text.ends_with('\n'));
    let without_nl = prepare_paste(&doc, source, empty_range(4, 0), "a\nb");
    assert!(!without_nl.text.ends_with('\n'));
}

#[test]
fn mixed_tabs_and_spaces_end_to_end() {
    let source = "Top\n\n    existing\n\n";
    let doc = parse_document(source).expect("parse");
    // Clipboard uses a tab for level-1 indent; emitted as spaces at anchor 4.
    let result = prepare_paste(&doc, source, empty_range(4, 0), "parent\n\tchild\n");
    assert_eq!(result.mode, PasteMode::Reanchor);
    // baseline 0, anchor 4, delta +4: parent -> 4; child (tab=4) -> 8.
    assert_eq!(result.text, "    parent\n        child\n");
}

#[test]
fn clipboard_verbatim_block_stays_well_formed_when_reanchored() {
    // §6: a clipboard whose content is itself a verbatim block, pasted into an
    // ordinary session body, is treated as plain multi-line text. The constant
    // offset preserves the body's indent relative to its subject.
    let source = "Top\n\n    existing\n\n";
    let doc = parse_document(source).expect("parse");
    let pasted = "snippet\n    fn main() {}\n:: rust ::\n";
    let result = prepare_paste(&doc, source, empty_range(4, 0), pasted);
    assert_eq!(result.mode, PasteMode::Reanchor);
    // anchor 4, baseline 0: subject -> 4, body -> 8, label -> 4. Body stays one
    // level under its subject, so the verbatim block remains well-formed.
    assert_eq!(
        result.text,
        "    snippet\n        fn main() {}\n    :: rust ::\n"
    );
}

#[test]
fn partial_indentation_carried_through_end_to_end() {
    let source = "Top\n\n    existing\n\n";
    let doc = parse_document(source).expect("parse");
    // Clipboard has a partially-indented second line (3 spaces under a 0 baseline).
    let result = prepare_paste(&doc, source, empty_range(4, 0), "head\n   odd\n");
    assert_eq!(result.mode, PasteMode::Reanchor);
    // baseline 0, anchor 4: head -> 4; odd (3) -> 7 (partial indent preserved).
    assert_eq!(result.text, "    head\n       odd\n");
}

#[test]
fn single_line_clipboard_merge_inside_session_passes_through() {
    let source = "Top\n\n    existing\n";
    let doc = parse_document(source).expect("parse");
    // Caret at byte 12 — end of "    existing" (4 + 8), a merge position.
    let result = prepare_paste(&doc, source, empty_range(2, 12), "one liner");
    assert_eq!(result.mode, PasteMode::PassthroughSingleLine);
    assert_eq!(result.text, "one liner");
}

// ---------------------------------------------------------------------------
// Wire format.
// ---------------------------------------------------------------------------

#[test]
fn mode_serializes_kebab_case() {
    assert_eq!(
        PasteMode::PassthroughVerbatim.as_str(),
        "passthrough-verbatim"
    );
    assert_eq!(PasteMode::PassthroughTable.as_str(), "passthrough-table");
    assert_eq!(
        PasteMode::PassthroughSingleLine.as_str(),
        "passthrough-single-line"
    );
    assert_eq!(PasteMode::Reanchor.as_str(), "re-anchor");

    let json = serde_json::to_string(&PasteMode::Reanchor).unwrap();
    assert_eq!(json, "\"re-anchor\"");
    let json = serde_json::to_string(&PasteMode::PassthroughVerbatim).unwrap();
    assert_eq!(json, "\"passthrough-verbatim\"");
}

#[test]
fn result_serializes_camel_case_fields() {
    let result = PreparePasteResult {
        text: "x".to_string(),
        mode: PasteMode::Reanchor,
    };
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["text"], "x");
    assert_eq!(json["mode"], "re-anchor");
}
