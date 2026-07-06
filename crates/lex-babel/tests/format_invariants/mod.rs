//! Formatter sanity invariants (lex#681).
//!
//! Two text-first properties that must hold for a pure formatter:
//!
//!   Tier 1 — idempotence:          format(format(D)) == format(D)
//!   Tier 2 — semantic preservation: canon(parse(D)) == canon(parse(format(D)))
//!
//! The existing `round_trip_proptest` module is *AST-first*: it constructs
//! `Document` values and serializes them. That path cannot exercise the
//! formatter's normalizers (indent width, blank-line runs, marker spelling,
//! table-cell padding) because a constructed AST carries no presentation to
//! normalize. These tests start from *source text*, which is the only way to
//! feed the normalizers messy input.
//!
//! The Skeleton reducer `canon` (and what it quotients out) lives in the shared
//! `crate::skeleton` module, so this suite and the conversion-faithfulness tests
//! compare Skeletons through one comparator.
//!
//! Note on footnotes: the only footnote form is the canonical `:: notes ::`
//! *list*, which the formatter serializes as-is. (A legacy session→list
//! migration once ran here as part of `format`; it has been removed, so `canon`
//! does not model that equivalence and no test input uses the legacy form.)

use crate::skeleton::canon;
use lex_babel::transforms::format_lex_source;
use lex_core::lex::parsing::parse_document;

// -----------------------------------------------------------------------------
// format() — the function under test
// -----------------------------------------------------------------------------

/// `format(D)` = parse, serialize (default rules) — the same path as
/// `lexd format`. Returns the formatter error as `Err` rather than panicking,
/// so proptest can shrink a formatting failure to a minimal input.
fn format(source: &str) -> Result<String, String> {
    format_lex_source(source).map_err(|e| format!("formatting failed: {e}"))
}

// -----------------------------------------------------------------------------
// The two invariants, as reusable check fns that return a failure report.
// -----------------------------------------------------------------------------

/// Tier 1: `format(format(D)) == format(D)`. Pure text equality.
fn check_idempotent(source: &str) -> Result<(), String> {
    let once = format(source)?;
    let twice = format(&once)?;
    if once == twice {
        Ok(())
    } else {
        Err(format!(
            "NOT IDEMPOTENT\n--- format(D) ---\n{once}\n--- format(format(D)) ---\n{twice}"
        ))
    }
}

/// Tier 2: `canon(parse(D)) == canon(parse(format(D)))`.
fn check_semantic_preserved(source: &str) -> Result<(), String> {
    let parsed = match parse_document(source) {
        Ok(d) => d,
        Err(e) => return Err(format!("source did not parse: {e}")),
    };
    let formatted = format(source)?;
    let reparsed = match parse_document(&formatted) {
        Ok(d) => d,
        Err(e) => {
            return Err(format!(
                "formatted output did not parse: {e}\n--- output ---\n{formatted}"
            ))
        }
    };
    let c1 = canon(&parsed);
    let c2 = canon(&reparsed);
    if c1 == c2 {
        Ok(())
    } else {
        Err(format!(
            "SEMANTICS CHANGED\n--- formatted output ---\n{formatted}\n--- canon(parse(D)) ---\n{c1:#?}\n--- canon(parse(format(D))) ---\n{c2:#?}"
        ))
    }
}

// -----------------------------------------------------------------------------
// Targeted snippets — explicitly cover tables, footnotes, and every reference
// type (the "newer, sometimes-missed-on-fixtures" features per lex#681). Each
// is intentionally *messy* (odd indent, blank-line runs, ragged markers/cells)
// so the formatter's normalizers are exercised.
// -----------------------------------------------------------------------------

/// (name, source). Sources use real-world messiness the normalizers must absorb.
fn targeted_cases() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "footnotes_doc_scoped",
            "Doc\n===\n\nHere is a reference [1] and another [2].\n\n:: notes ::\n\n1. First note.\n2. Second note.\n",
        ),
        (
            "footnotes_section_scoped",
            "Chapter:\n\n    The method was first proposed in 2019 [1].\n\n    :: notes ::\n\n    1. Original paper: Smith et al., 2019.\n",
        ),
        (
            "ref_url_file_session_general",
            "Doc\n===\n\nSee [https://example.com] and [./file.txt] and [#42] and [Introduction].\n",
        ),
        (
            "ref_tk_citation_annref",
            "Doc\n===\n\nDraft here [TK] and [TK-budget]. Per [@smith2019 p. 5]. As noted [::caveat].\n\n:: caveat ::\n    A caveat.\n",
        ),
        (
            "table_basic_ragged",
            "Stats:\n    |A|B|\n    |1|2|\n:: table ::\n",
        ),
        // Mismatched row widths (a short middle row) must not be padded out to a
        // rectangular grid: the short row must keep its own cell count across a
        // round-trip (lex#792).
        (
            "table_mismatched_rows",
            "Sparse Data:\n    | Name  | Age | City     |\n    | Alice | 30  |\n    | Bob   | 25  | Paris    |\n",
        ),
        (
            "table_aligned_header",
            "Stats:\n    | Name | Score |\n    |:---|---:|\n    | Alice | 10 |\n:: table ::\n",
        ),
        (
            "table_with_footnotes",
            "Results:\n    | Metric | Value |\n    | Speed [1] | Fast |\n\n    1. Measured on a warm cache.\n:: table ::\n",
        ),
        (
            "table_colspan",
            "Grid:\n    | A | B | C |\n    | span | >> | c |\n:: table ::\n",
        ),
        (
            "table_rowspan",
            "Grid:\n    | a | b |\n    | ^^ | c |\n:: table ::\n",
        ),
        // A rowspan stacked across three rows: each continuation row re-emits `^^`
        // in the spanning column, and the parser must keep crediting the *same*
        // top cell (lex#694 review — the old index-based resolution mis-aimed the
        // second `^^` once the first row had been shrunk).
        (
            "table_rowspan_multirow",
            "Grid:\n    | a | b |\n    | ^^ | c |\n    | ^^ | d |\n:: table ::\n",
        ),
        // Two adjacent rowspans in one continuation row (`| ^^ | ^^ | f |`): both
        // columns must keep their own span and `f` must stay in the third column.
        (
            "table_rowspan_adjacent",
            "Grid:\n    | a | b | c |\n    | ^^ | ^^ | f |\n:: table ::\n",
        ),
        // A rowspan in a middle column, with cells on either side.
        (
            "table_rowspan_midcolumn",
            "Grid:\n    | a | b | c |\n    | d | ^^ | f |\n:: table ::\n",
        ),
        // Colspan and rowspan in the same continuation row (`| dd | >> | ^^ |`):
        // the grid projection must re-derive both `>>` and `^^`.
        (
            "table_colspan_rowspan",
            "Grid:\n    | a | b | c |\n    | dd | >> | ^^ |\n    | e | f | g |\n:: table ::\n",
        ),
        (
            "messy_blank_runs_and_markers",
            "Doc\n===\n\n\n\n\nFirst para.\n\n\n\n* one\n* two\n* three\n",
        ),
        (
            "messy_numbered_restart",
            "List:\n\n    3. third looking\n    7. seventh looking\n    9. ninth looking\n",
        ),
    ]
}

// -----------------------------------------------------------------------------
// Known failures.
//
// These are inputs where an invariant currently fails because of a *real,
// filed formatter bug* — not a test defect. Listing them keeps the suite green
// while the bugs are open, WITHOUT silently skipping: the runner asserts every
// listed case still fails (so the list cannot rot — when a bug is fixed the
// test tells you to delete the entry) and logs the excluded set on each run.
//
// The `TARGETED` lists cover the in-code `targeted_cases()`; the `FIXTURE` lists
// cover the curated comms corpus (`corpus_fixtures()` — elements/**, trifecta,
// benchmark). Tier-1 (idempotence) and Tier-2 (canon) are listed separately so
// each entry names the issue that actually explains *that tier's* failure.
//
// Every corpus entry below is a PRE-EXISTING faithfulness gap newly surfaced by
// the expanded coverage — NOT a regression from #782. Each was confirmed by
// running the real `check_idempotent` / `check_semantic_preserved` on the file:
//
//   #790 — nested block bodies de-indent on serialize. The subject-colon
//          de-indent (a verbatim/definition subject whose source colon was
//          followed by trailing whitespace re-serialized as `Subject::`, escaping
//          the body) and multi-line *text* table cells (whose embedded newline
//          split the pipe row) are now FIXED — the two benchmark docs, the
//          flat-multiline table, and the verbatim-group fixture pass. What remains
//          under #790 is table cells that carry *block* content (a list, verbatim,
//          annotation, or a table nested in a definition inside a cell): that
//          inner structure still de-indents/escapes on serialize.
//   #791 — leading document-level annotations reorder around the title / subtitle
//          on serialize. The serializer used to emit the first-class title node
//          before the body stream, hoisting it ahead of any annotation authored
//          above it; it now emits the title at its own source position, so the two
//          named repros (document-09, annotation-27) pass. A DEEPER case remains:
//          annotations that in the source attach to the first *session* re-attach
//          to the document *root* on round-trip (they serialize at the document
//          head and re-parse as root-owned) — the inlines-spec fixtures and
//          20-ideas-naked still fail Tier-2 on that attachment change.
//   #792 — FIXED. Ragged/mismatched-row tables used to get padded + a separator
//          row injected, adding cells to short rows (Tier-2). The serializer no
//          longer pads short rows, so table-13 round-trips faithfully.
//   #783 — the `:: doc.untitled ::` title-model sentinel (document-06) now
//          parses and round-trips under the ADR-0002 title model, so it is no
//          longer a known failure.
// -----------------------------------------------------------------------------

const TIER1_TARGETED_KNOWN_FAIL: &[(&str, &str)] = &[];

const TIER2_TARGETED_KNOWN_FAIL: &[(&str, &str)] = &[];

const TIER1_FIXTURE_KNOWN_FAIL: &[(&str, &str)] = &[
    // #790 (residual) — a table nested inside definitions emits its closing
    // `:: table ::` annotation one indent level too shallow, escaping the
    // innermost container. The cell-block-content de-indent that used to list
    // table-19/21/22/23 here is fixed (the serializer no longer re-walks cell
    // children); this remaining case is the nested-closer indent.
    ("table.docs/table-08-nested-in-definition.lex", "lex#790"),
];

const TIER2_FIXTURE_KNOWN_FAIL: &[(&str, &str)] = &[
    // #790 (residual) — a table nested inside definitions emits its closing
    // `:: table ::` annotation one indent level too shallow, so on re-parse the
    // closer (and the table) detach from the innermost container. The cell-block-
    // content class (table-19/20/21/22/23) is fixed; this nested-closer case
    // remains.
    ("table.docs/table-08-nested-in-definition.lex", "lex#790"),
    // #791 (deeper case) — leading/document-level annotations that in the source
    // are attached to the first *session* re-attach to the document *root* across
    // a round-trip: the annotations serialize at the document head and re-parse as
    // root-owned rather than session-owned. #791's serializer-ordering fix keeps a
    // leading annotation above the title (document-09, annotation-27 pass) but does
    // not change this attachment target, so these fixtures still fail Tier-2.
    ("benchmark/20-ideas-naked.lex", "lex#791"),
    ("inlines.docs/specs/formatting/formatting.lex", "lex#791"),
    (
        "inlines.docs/specs/formatting/inlines-general.lex",
        "lex#791",
    ),
    ("inlines.docs/specs/references/citations.lex", "lex#791"),
    (
        "inlines.docs/specs/references/references-general.lex",
        "lex#791",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn specs_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../comms/specs")
    }

    /// Recursively collect every `*.lex` under `root`, keyed by its path relative
    /// to `root` prefixed with `key_prefix` (so callers can namespace the corpus
    /// roots into one flat map). The relative path keeps `.docs/` sub-fixtures
    /// distinct from their bare-stem top-level siblings — e.g.
    /// `table.docs/table-05-flat-multiline.lex` never collides with `table.lex`.
    fn collect_lex(root: &Path, key_prefix: &str, out: &mut Vec<(String, String)>) {
        let entries =
            std::fs::read_dir(root).unwrap_or_else(|e| panic!("read {}: {e}", root.display()));
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                let sub = path.file_name().unwrap().to_string_lossy();
                let prefix = format!("{key_prefix}{sub}/");
                collect_lex(&path, &prefix, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("lex") {
                let name = format!(
                    "{key_prefix}{}",
                    path.file_name().unwrap().to_string_lossy()
                );
                let src = std::fs::read_to_string(&path).unwrap();
                out.push((name, src));
            }
        }
    }

    /// The full curated comms corpus: every `.lex` under `specs/elements/**`
    /// (including the ~150 `.docs/` sub-fixtures), plus `specs/trifecta/*` and
    /// `specs/benchmark/*`. These are all real, maintained documents that
    /// authors keep faithful, so they are the right anchor for the formatter's
    /// two text-first invariants — no machine-generated snapshot required.
    ///
    /// Keys are namespaced by corpus root: `trifecta/…` and `benchmark/…` carry
    /// their root; element keys are relative to `elements/` (top-level stems bare,
    /// `.docs/` fixtures prefixed with their subdir). Every key is unique.
    fn corpus_fixtures() -> Vec<(String, String)> {
        let specs = specs_dir();
        let mut out = Vec::new();
        collect_lex(&specs.join("elements"), "", &mut out);
        collect_lex(&specs.join("trifecta"), "trifecta/", &mut out);
        collect_lex(&specs.join("benchmark"), "benchmark/", &mut out);
        out.sort();
        assert!(
            out.len() > 150,
            "expected the full curated corpus (>150 files); got {} under {}",
            out.len(),
            specs.display()
        );
        out
    }

    /// Run `check` over every (name, source) case, honouring the known-failure
    /// list. Asserts: (a) no un-listed case violates the invariant, and (b)
    /// every listed case still violates it (anti-rot). Logs the excluded set.
    fn run_sweep(
        label: &str,
        cases: &[(String, String)],
        known_fail: &[(&str, &str)],
        check: impl Fn(&str) -> Result<(), String>,
    ) {
        let mut unexpected_fail = Vec::new();
        let mut unexpected_pass = Vec::new();
        let mut excluded = Vec::new();

        for (name, src) in cases {
            let issue = known_fail.iter().find(|(n, _)| n == name).map(|(_, i)| *i);
            match (check(src), issue) {
                (Ok(()), None) => {}
                (Ok(()), Some(issue)) => unexpected_pass.push(format!(
                    "{name}: listed as known-failing ({issue}) but now PASSES — remove it from the known-failure list"
                )),
                (Err(report), None) => unexpected_fail.push(format!("[{name}]\n{report}")),
                (Err(_), Some(issue)) => excluded.push(format!("{name} -> {issue}")),
            }
        }

        if !excluded.is_empty() {
            eprintln!(
                "[{label}] {} known failure(s) excluded (open bugs):\n  {}",
                excluded.len(),
                excluded.join("\n  ")
            );
        }

        let mut problems = Vec::new();
        if !unexpected_pass.is_empty() {
            problems.push(format!(
                "{} case(s) now satisfy the invariant but are still listed as known-failing:\n{}",
                unexpected_pass.len(),
                unexpected_pass.join("\n")
            ));
        }
        if !unexpected_fail.is_empty() {
            problems.push(format!(
                "{} NEW invariant violation(s) (not in the known-failure list):\n\n{}",
                unexpected_fail.len(),
                unexpected_fail.join("\n\n========\n\n")
            ));
        }
        assert!(
            problems.is_empty(),
            "[{label}]\n\n{}",
            problems.join("\n\n")
        );
    }

    fn owned(cases: Vec<(&'static str, &'static str)>) -> Vec<(String, String)> {
        cases
            .into_iter()
            .map(|(n, s)| (n.to_string(), s.to_string()))
            .collect()
    }

    #[test]
    fn tier1_idempotence_targeted() {
        run_sweep(
            "tier1/targeted",
            &owned(targeted_cases()),
            TIER1_TARGETED_KNOWN_FAIL,
            check_idempotent,
        );
    }

    #[test]
    fn tier2_semantic_preservation_targeted() {
        run_sweep(
            "tier2/targeted",
            &owned(targeted_cases()),
            TIER2_TARGETED_KNOWN_FAIL,
            check_semantic_preserved,
        );
    }

    #[test]
    fn tier1_idempotence_corpus_fixtures() {
        run_sweep(
            "tier1/corpus",
            &corpus_fixtures(),
            TIER1_FIXTURE_KNOWN_FAIL,
            check_idempotent,
        );
    }

    #[test]
    fn tier2_semantic_preservation_corpus_fixtures() {
        run_sweep(
            "tier2/corpus",
            &corpus_fixtures(),
            TIER2_FIXTURE_KNOWN_FAIL,
            check_semantic_preserved,
        );
    }

    /// The `table_aligned_header` Tier-2 case only proves alignment is *symmetric*
    /// across a round-trip — it would also pass if alignment were dropped on both
    /// sides (the old #702 blind spot). This asserts the stronger property: the
    /// markdown separator-row alignment hints are actually *retained* in the
    /// formatted output, not flattened to a plain `---` separator.
    #[test]
    fn table_separator_alignment_is_retained() {
        let src =
            "Stats:\n    | Name | Score |\n    |:---|---:|\n    | Alice | 10 |\n:: table ::\n";
        let formatted = format(src).expect("format");
        assert!(
            formatted.contains(":---") && formatted.contains("---:"),
            "alignment markers must survive formatting (lex#702); got:\n{formatted}"
        );
        // And re-emitting is a fixed point.
        assert_eq!(format(&formatted).expect("reformat"), formatted);
    }
}

// -----------------------------------------------------------------------------
// Messy-input proptest.
//
// Generates non-canonical *source text* and asserts both invariants. This is
// the fuzzing complement to the curated cases above: it stresses the
// presentation normalizers the formatter is supposed to absorb.
//
// Scope is deliberately limited to normalizers verified to round-trip by the
// targeted cases — blank-line runs, bullet-marker variety, flat numbered
// renumbering, trailing whitespace, and inline references. Two messiness axes
// are intentionally excluded to avoid re-tripping open bugs / untriaged edges:
//   - attached annotations            (#682)
//   - tables                          (#683, #684)
//   - nested/extended numbered lists  (#685)
//   - a bare document title line       (#687 — generator leads with a 2-line
//                                        paragraph, which is never title-absorbed)
// Widen this generator as those are fixed. (The paragraph-merge-on-indent edge,
// #699, is now fixed — hanging-indent continuations fold back into the paragraph.)
// -----------------------------------------------------------------------------

#[cfg(test)]
mod messy_proptest {
    use super::*;
    use proptest::prelude::*;

    fn words() -> impl Strategy<Value = String> {
        prop::collection::vec("[a-z]{1,7}", 1..6).prop_map(|w| w.join(" "))
    }

    /// Inline reference tokens spanning several of the 8 reference forms. They
    /// are preserved verbatim by the formatter, so they must survive both tiers.
    fn inline_ref() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("[1]".to_string()),
            Just("[42]".to_string()),
            Just("[https://example.com]".to_string()),
            Just("[./file.txt]".to_string()),
            Just("[#3]".to_string()),
            Just("[TK]".to_string()),
            Just("[TK-budget]".to_string()),
        ]
    }

    /// Trailing whitespace of random width (the formatter must trim it).
    fn trailing() -> impl Strategy<Value = String> {
        (0usize..3).prop_map(|n| " ".repeat(n))
    }

    fn paragraph_block() -> impl Strategy<Value = String> {
        (
            prop::collection::vec((words(), trailing()), 1..3),
            prop::option::of(inline_ref()),
        )
            .prop_map(|(lines, maybe_ref)| {
                let mut out: Vec<String> = lines
                    .into_iter()
                    .map(|(w, tr)| format!("{w}{tr}"))
                    .collect();
                if let Some(r) = maybe_ref {
                    let last = out.len() - 1;
                    out[last] = format!("{} {r}", out[last].trim_end());
                }
                out.join("\n")
            })
    }

    fn unordered_block() -> impl Strategy<Value = String> {
        // Mixed bullet chars ('-', '*', '+'); the formatter normalizes to '-'.
        prop::collection::vec(
            (prop_oneof![Just('-'), Just('*'), Just('+')], words()),
            2..5,
        )
        .prop_map(|items| {
            items
                .into_iter()
                .map(|(m, w)| format!("{m} {w}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    fn numbered_block() -> impl Strategy<Value = String> {
        // Flat numbered list with arbitrary, non-sequential start markers; the
        // formatter renumbers to 1., 2., 3., ...
        (prop::collection::vec(words(), 2..5), 1usize..20).prop_map(|(items, start)| {
            items
                .into_iter()
                .enumerate()
                .map(|(i, w)| format!("{}. {w}", start + i * 3))
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    fn definition_block() -> impl Strategy<Value = String> {
        ("[A-Z][a-z]{2,8}", prop::collection::vec(words(), 1..3)).prop_map(|(subject, body)| {
            let body = body
                .into_iter()
                .map(|w| format!("    {w}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{subject}:\n{body}")
        })
    }

    fn block() -> impl Strategy<Value = String> {
        prop_oneof![
            paragraph_block(),
            unordered_block(),
            numbered_block(),
            definition_block(),
        ]
    }

    /// A whole document. To avoid the title bug (#687) it opens with a
    /// guaranteed two-line paragraph (never title-absorbed), then blocks
    /// separated by blank-line runs of random width (1..4 — the formatter
    /// clamps to <= 2, including the 3+-blanks-after-a-list case fixed in #686).
    fn messy_document() -> impl Strategy<Value = String> {
        (
            (words(), words()),
            prop::collection::vec((block(), 1usize..4), 0..4),
        )
            .prop_map(|((lead1, lead2), blocks)| {
                let mut out = format!("{lead1}\n{lead2}\n");
                for (b, blanks) in blocks.iter() {
                    out.push_str(&"\n".repeat(*blanks));
                    out.push_str(b);
                    out.push('\n');
                }
                out
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn tier1_idempotence_messy(src in messy_document()) {
            if let Err(report) = check_idempotent(&src) {
                prop_assert!(false, "--- source ---\n{}\n{}", src, report);
            }
        }

        #[test]
        fn tier2_semantic_preservation_messy(src in messy_document()) {
            if let Err(report) = check_semantic_preserved(&src) {
                prop_assert!(false, "--- source ---\n{}\n{}", src, report);
            }
        }
    }
}
