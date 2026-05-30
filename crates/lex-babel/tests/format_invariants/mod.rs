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
//! `canon` is a purpose-built lossless projection of the AST. We deliberately
//! do NOT reuse `AstSnapshot`: its `label` comes from `display_label()`, which
//! truncates text at 50 chars and omits table cells / footnotes — blind to
//! exactly the table/footnote/reference content this suite targets.
//!
//! What `canon` quotients out (everything the formatter is *allowed* to change):
//!   - source ranges / offsets            (never represented in Canon)
//!   - blank-line groups                  (dropped: purely presentational separators)
//!   - list/marker *spelling*             (dropped; decoration *style* kept)
//!   - trailing whitespace                (every text field trimmed)
//!   - annotation label *spelling*        (canonical `.value` kept)
//!   - table cell padding                 (cell text trimmed)
//!
//! Note on footnote normalization: `format` here is `format_lex_source`, which
//! runs `normalize_footnotes` before serializing. That step rewrites a trailing
//! legacy "Notes"/"Footnotes" *session* into a canonical `:: notes ::` *list*.
//! The legacy session form is deprecated and should not occur in practice, so
//! `canon` intentionally does not model the equivalence and no test input uses
//! it — the canonical `:: notes ::` list form is the only one exercised.

use lex_babel::transforms::format_lex_source;
use lex_core::lex::ast::elements::inlines::{InlineNode, ReferenceType};
use lex_core::lex::ast::elements::sequence_marker::DecorationStyle;
use lex_core::lex::ast::{Annotation, ContentItem, Document, TableRow, TextContent};
use lex_core::lex::parsing::parse_document;

// -----------------------------------------------------------------------------
// format() — the function under test
// -----------------------------------------------------------------------------

/// `format(D)` = parse, `normalize_footnotes`, serialize (default rules) — the
/// same path as `lexd format`. Returns the formatter error as `Err` rather than
/// panicking, so proptest can shrink a formatting failure to a minimal input.
fn format(source: &str) -> Result<String, String> {
    format_lex_source(source).map_err(|e| format!("formatting failed: {e}"))
}

// -----------------------------------------------------------------------------
// Canon — lossless, presentation-quotiented AST projection
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Canon {
    Document {
        title: Option<String>,
        subtitle: Option<String>,
        annotations: Vec<Canon>,
        children: Vec<Canon>,
    },
    Session {
        title: String,
        style: Option<String>,
        annotations: Vec<Canon>,
        children: Vec<Canon>,
    },
    Paragraph {
        text: String,
        refs: Vec<String>,
        annotations: Vec<Canon>,
    },
    List {
        style: Option<String>,
        annotations: Vec<Canon>,
        items: Vec<Canon>,
    },
    ListItem {
        text: String,
        refs: Vec<String>,
        annotations: Vec<Canon>,
        children: Vec<Canon>,
    },
    Definition {
        subject: String,
        annotations: Vec<Canon>,
        children: Vec<Canon>,
    },
    Verbatim {
        subject: String,
        closing_label: String,
        lines: Vec<String>,
    },
    Annotation {
        label: String,
        params: Vec<(String, String)>,
        children: Vec<Canon>,
    },
    Table {
        subject: String,
        rows: Vec<CanonRow>,
        footnotes: Vec<Canon>,
        annotations: Vec<Canon>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonRow {
    header: bool,
    cells: Vec<CanonCell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonCell {
    text: String,
    refs: Vec<String>,
    colspan: usize,
    rowspan: usize,
    align: String,
}

/// Stable tag for a reference's classified type. Captures *which* of the 8
/// reference forms the parser resolved, plus the target/key, so the suite
/// asserts reference-type preservation explicitly (lex#681 ask).
fn ref_tag(ty: &ReferenceType) -> String {
    match ty {
        ReferenceType::ToCome { identifier } => {
            format!("tk:{}", identifier.as_deref().unwrap_or(""))
        }
        ReferenceType::Citation(data) => format!("cite:{}", data.keys.join(",")),
        ReferenceType::AnnotationReference { label } => format!("annref:{label}"),
        ReferenceType::FootnoteNumber { number } => format!("foot:{number}"),
        ReferenceType::Session { target } => format!("session:{target}"),
        ReferenceType::Url { target } => format!("url:{target}"),
        ReferenceType::File { target } => format!("file:{target}"),
        ReferenceType::General { target } => format!("general:{target}"),
        ReferenceType::NotSure => "notsure".to_string(),
    }
}

/// Extract classified references from a text field by parsing its inlines.
/// `inlines()` may be lazy/unpopulated on a freshly-parsed document, so parse
/// on a clone.
fn refs_in(tc: &TextContent) -> Vec<String> {
    let mut tc = tc.clone();
    tc.inlines_or_parse()
        .iter()
        .filter_map(|node| match node {
            InlineNode::Reference { data, .. } => Some(ref_tag(&data.reference_type)),
            _ => None,
        })
        .collect()
}

fn style_tag(style: DecorationStyle) -> String {
    format!("{style:?}")
}

fn canon_annotations(anns: &[Annotation]) -> Vec<Canon> {
    anns.iter().map(canon_annotation).collect()
}

fn canon_annotation(ann: &Annotation) -> Canon {
    Canon::Annotation {
        // `.value` is the canonical label; the formatter re-emits the source
        // spelling form, which reparses back to the same canonical value.
        label: ann.data.label.value.clone(),
        params: ann
            .data
            .parameters
            .iter()
            .map(|p| (p.key.clone(), p.value.clone()))
            .collect(),
        children: canon_items(ann.children.iter()),
    }
}

fn canon_items<'a, I: Iterator<Item = &'a ContentItem>>(items: I) -> Vec<Canon> {
    items.filter_map(canon_item).collect()
}

fn canon_row(row: &TableRow) -> CanonRow {
    CanonRow {
        header: row.cells.iter().any(|c| c.header),
        cells: row
            .cells
            .iter()
            .map(|c| CanonCell {
                text: c.content.as_string().trim().to_string(),
                refs: refs_in(&c.content),
                colspan: c.colspan,
                rowspan: c.rowspan,
                align: format!("{:?}", c.align),
            })
            .collect(),
    }
}

/// Project a `ContentItem` to its semantic Canon, or `None` for nodes that are
/// purely presentational (blank-line groups) or already folded into a parent
/// (loose `TextLine`s are handled inside `Paragraph`).
fn canon_item(item: &ContentItem) -> Option<Canon> {
    Some(match item {
        ContentItem::BlankLineGroup(_) => return None,
        // A bare TextLine outside a Paragraph: treat as a one-line paragraph.
        ContentItem::TextLine(tl) => Canon::Paragraph {
            text: tl.content.as_string().trim_end().to_string(),
            refs: refs_in(&tl.content),
            annotations: Vec::new(),
        },
        ContentItem::Paragraph(p) => {
            let mut text = String::new();
            let mut refs = Vec::new();
            for line in &p.lines {
                if let ContentItem::TextLine(tl) = line {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(tl.content.as_string().trim_end());
                    refs.extend(refs_in(&tl.content));
                }
            }
            Canon::Paragraph {
                text,
                refs,
                annotations: canon_annotations(&p.annotations),
            }
        }
        ContentItem::Session(s) => Canon::Session {
            title: s.title.as_string().trim_end().to_string(),
            style: s.marker.as_ref().map(|m| style_tag(m.style)),
            annotations: canon_annotations(&s.annotations),
            children: canon_items(s.children.iter()),
        },
        ContentItem::List(l) => Canon::List {
            style: l.marker.as_ref().map(|m| style_tag(m.style)),
            annotations: canon_annotations(&l.annotations),
            items: canon_items(l.items.iter()),
        },
        ContentItem::ListItem(li) => {
            // Project *all* text elements (not just the first) so multi-line
            // item content is covered; collect refs from each.
            let text = li
                .text
                .iter()
                .map(|t| t.as_string().trim_end())
                .collect::<Vec<_>>()
                .join("\n");
            let refs = li.text.iter().flat_map(refs_in).collect();
            Canon::ListItem {
                text,
                refs,
                annotations: canon_annotations(&li.annotations),
                children: canon_items(li.children.iter()),
            }
        }
        ContentItem::Definition(d) => Canon::Definition {
            subject: d.subject.as_string().trim_end().to_string(),
            annotations: canon_annotations(&d.annotations),
            children: canon_items(d.children.iter()),
        },
        ContentItem::VerbatimBlock(v) => Canon::Verbatim {
            subject: v.subject.as_string().trim_end().to_string(),
            closing_label: v.closing_data.label.value.clone(),
            lines: v
                .children
                .iter()
                .filter_map(|c| match c {
                    ContentItem::VerbatimLine(vl) => Some(vl.content.as_string().to_string()),
                    _ => None,
                })
                .collect(),
        },
        ContentItem::VerbatimLine(vl) => Canon::Verbatim {
            subject: String::new(),
            closing_label: String::new(),
            lines: vec![vl.content.as_string().to_string()],
        },
        ContentItem::Annotation(a) => canon_annotation(a),
        ContentItem::Table(t) => {
            let mut rows: Vec<CanonRow> = Vec::new();
            for r in &t.header_rows {
                rows.push(canon_row(r));
            }
            for r in &t.body_rows {
                rows.push(canon_row(r));
            }
            let footnotes = match &t.footnotes {
                Some(list) => canon_items(list.items.iter()),
                None => Vec::new(),
            };
            Canon::Table {
                subject: t.subject.as_string().trim_end().to_string(),
                rows,
                footnotes,
                annotations: canon_annotations(&t.annotations),
            }
        }
    })
}

fn canon(doc: &Document) -> Canon {
    Canon::Document {
        title: doc
            .title
            .as_ref()
            .map(|t| t.content.as_string().trim_end().to_string()),
        subtitle: doc
            .title
            .as_ref()
            .and_then(|t| t.subtitle.as_ref())
            .map(|s| s.as_string().trim_end().to_string()),
        annotations: canon_annotations(&doc.annotations),
        children: canon_items(doc.root.children.iter()),
    }
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
// Bugs (shipped fixes removed their entries here):
//   #681 — umbrella for remaining mixed/untriaged element-fixture sweep failures
//          (e.g. paragraph merge of siblings distinguished only by irregular indent)
// -----------------------------------------------------------------------------

const TIER1_TARGETED_KNOWN_FAIL: &[(&str, &str)] = &[];

const TIER2_TARGETED_KNOWN_FAIL: &[(&str, &str)] = &[];

const TIER1_FIXTURE_KNOWN_FAIL: &[(&str, &str)] = &[];

const TIER2_FIXTURE_KNOWN_FAIL: &[(&str, &str)] = &[
    // annotation.lex no longer fails on the #696 annotation edges; its residual
    // Tier-2 gap is a paragraph merge — two sibling paragraphs distinguished only
    // by an irregular extra indent level collapse into one when re-indented. That
    // is the untriaged paragraph-merge edge tracked under the #681 umbrella.
    ("annotation.lex", "#681"),
    ("data.lex", "#681"),
    ("label.lex", "#681"),
    ("parameter.lex", "#681"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn elements_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../comms/specs/elements")
    }

    /// All top-level element fixtures (`*.lex` directly under `elements/`).
    /// These are real, maintained documents that exercise every element kind,
    /// including the newer table/footnote/reference features.
    fn element_fixtures() -> Vec<(String, String)> {
        let dir = elements_dir();
        let mut out = Vec::new();
        let entries =
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
        for entry in entries {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) == Some("lex") {
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                let src = std::fs::read_to_string(&path).unwrap();
                out.push((name, src));
            }
        }
        out.sort();
        assert!(
            !out.is_empty(),
            "no element fixtures found in {}",
            dir.display()
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
    fn tier1_idempotence_element_fixtures() {
        run_sweep(
            "tier1/fixtures",
            &element_fixtures(),
            TIER1_FIXTURE_KNOWN_FAIL,
            check_idempotent,
        );
    }

    #[test]
    fn tier2_semantic_preservation_element_fixtures() {
        run_sweep(
            "tier2/fixtures",
            &element_fixtures(),
            TIER2_FIXTURE_KNOWN_FAIL,
            check_semantic_preserved,
        );
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
//   - random *leading* indent widths  (untriaged paragraph-merge edge, #681)
// Widen this generator as those are fixed.
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
