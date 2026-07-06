//! Full separation-matrix verification (lex#782).
//!
//! Slice #781 wired only the paragraph→paragraph cell. This module verifies the
//! completed matrix: every ordered pair of sibling block kinds, plus the
//! faithfulness the matrix buys for reader-shaped documents, the two per-block
//! band-aids it replaces, the composition with `BlankLineGroup`, and the
//! `lex → lex` byte-stability guarantee.
//!
//! The central test (`every_ordered_pair_reparses_as_two_blocks`) is the
//! "derived from and verified against the parser" criterion made executable: for
//! each ordered pair it builds a minimal reader-shaped document, serializes it
//! through the real matrix-driven serializer, re-parses, and asserts both blocks
//! survive with the correct types.
//!
//! Group semantics are characterized rather than asserted faithful (see the
//! `separation.rs` module docs and the
//! `definition_before_closer_led_block_is_a_known_hijack` test): per
//! `comms/specs/grammar-core.lex` §4.5c, a Definition
//! immediately before a closer-terminated Verbatim (or before an Annotation whose
//! `:: label ::` marker doubles as a verbatim closer) is that block's FIRST group.
//! A definition's `subject:` + indented body is structurally identical to a
//! `<verbatim-group-with-content>`, and verbatim is tried before definition (§4.7),
//! so the two cannot be authored as separate adjacent blocks — blank lines do not
//! separate groups. That is the multi-group verbatim feature working as designed
//! (lex#814 §4, resolved as intended), not a bug: the definition's subject AND body
//! survive as the verbatim's first group.
//!
//! Definition → Table is the ONE exception and is NOT intended: a table is
//! single-group, so the definition cannot become a group. The parser now requires
//! a `:: table ::` span to start with pipe-row table content, so this pair
//! separates into Definition + Table instead of collapsing to a degenerate table
//! that drops both the definition body and the table's rows (lex#819).
//!
//! Bare Annotation siblings are also not round-trippable as siblings (the parser
//! attaches a floating annotation to a neighbor or the document head), so they are
//! excluded from the sibling-sequence properties; the matrix still owns the lex#682
//! trailing-blank boundary, tested directly.

use lex_babel::formats::lex::export;
use lex_core::lex::ast::elements::container::SessionContainer;
use lex_core::lex::ast::elements::data::Data;
use lex_core::lex::ast::elements::label::{Label, LabelForm};
use lex_core::lex::ast::elements::sequence_marker::SequenceMarker;
use lex_core::lex::ast::elements::typed_content::{ContentElement, SessionContent};
use lex_core::lex::ast::elements::verbatim::VerbatimBlockMode;
use lex_core::lex::ast::elements::{VerbatimContent, VerbatimLine};
use lex_core::lex::ast::{
    Annotation, ContentItem, Definition, Document, List, ListItem, Paragraph, Session, Table,
    TableCell, TableRow, TextContent,
};
use lex_core::lex::parsing::parse_document;

use crate::skeleton::canon;

// ─── Block kinds and builders ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Paragraph,
    List,
    Session,
    Verbatim,
    Definition,
    Annotation,
    Table,
}

const ALL_KINDS: [Kind; 7] = [
    Kind::Paragraph,
    Kind::List,
    Kind::Session,
    Kind::Verbatim,
    Kind::Definition,
    Kind::Annotation,
    Kind::Table,
];

/// The role a block plays in an ordered pair. `Next` selects the most
/// *absorbable* shape of a block (the shape that stresses the boundary, e.g. a
/// marker-form Verbatim whose subject can merge into a preceding paragraph), so
/// the matrix is verified against its hardest case.
#[derive(Clone, Copy)]
enum Role {
    Prev,
    Next,
}

fn tc(s: &str) -> TextContent {
    TextContent::from_string(s.to_string(), None)
}

fn plain_list() -> List {
    let mut list = List::new(vec![
        ListItem::new("-".into(), "item one".into()),
        ListItem::new("-".into(), "item two".into()),
    ]);
    list.marker = SequenceMarker::parse("-", None);
    list
}

fn closed_table() -> Table {
    let header = TableRow::new(vec![
        TableCell::new(tc("A")).with_header(true),
        TableCell::new(tc("B")).with_header(true),
    ]);
    let body = TableRow::new(vec![TableCell::new(tc("1")), TableCell::new(tc("2"))]);
    let mut table = Table::new(
        tc("Tbl"),
        vec![header],
        vec![body],
        VerbatimBlockMode::Inflow,
    );
    // The `:: table ::` closer lives in `table.annotations`; without it the
    // serialized table is "open" and a later `::` closer re-anchors to it. The
    // canonical value round-trips (canon compares `.value`); the Shortcut form
    // makes `source_spelling` emit `:: table ::`, which the parser recognizes as
    // a table closer (`:: lex.tabular.table ::` would parse as a *verbatim*).
    table.annotations.push(Annotation::new(
        Label::new("lex.tabular.table".to_string()).with_form(LabelForm::Shortcut),
        vec![],
        vec![],
    ));
    table
}

/// A block of `kind` in `role`, as a top-level `SessionContent` (reader-shaped:
/// no `BlankLineGroup`s anywhere inside).
fn block(kind: Kind, role: Role) -> SessionContent {
    let element = match kind {
        Kind::Paragraph => ContentElement::Paragraph(Paragraph::from_line("Alpha para.".into())),
        Kind::List => ContentElement::List(plain_list()),
        Kind::Session => {
            return SessionContent::Session(Session::new(
                tc("Sub Title"),
                vec![SessionContent::Element(ContentElement::Paragraph(
                    Paragraph::from_line("Nested body.".into()),
                ))],
            ));
        }
        Kind::Verbatim => {
            let data = Data::new(Label::from_string("verbatim"), vec![]);
            let v = match role {
                // Marker form (subject, no indented body) is the absorbable shape.
                Role::Next => lex_core::lex::ast::Verbatim::new(
                    tc("Code2"),
                    vec![],
                    data,
                    VerbatimBlockMode::Inflow,
                ),
                Role::Prev => lex_core::lex::ast::Verbatim::new(
                    tc("Codeblk"),
                    vec![VerbatimContent::VerbatimLine(VerbatimLine::new(
                        "code line".into(),
                    ))],
                    data,
                    VerbatimBlockMode::Inflow,
                ),
            };
            ContentElement::VerbatimBlock(Box::new(v))
        }
        Kind::Definition => ContentElement::Definition(Definition::new(
            tc("Term"),
            vec![ContentElement::Paragraph(Paragraph::from_line(
                "Def body.".into(),
            ))],
        )),
        Kind::Annotation => ContentElement::Annotation(Annotation::new(
            Label::from_string("note"),
            vec![],
            vec![ContentElement::Paragraph(Paragraph::from_line(
                "Ann body.".into(),
            ))],
        )),
        Kind::Table => ContentElement::Table(Box::new(closed_table())),
    };
    SessionContent::Element(element)
}

/// A two-line paragraph — never promoted to the document title (the title-steal
/// rule only takes a *single*-line first paragraph), so it neutralizes the
/// title boundary and lets the pair under test be measured as plain siblings.
fn lead() -> SessionContent {
    SessionContent::Element(ContentElement::Paragraph(Paragraph::new(vec![
        ContentItem::TextLine(lex_core::lex::ast::elements::paragraph::TextLine::new(tc(
            "Lead line one.",
        ))),
        ContentItem::TextLine(lex_core::lex::ast::elements::paragraph::TextLine::new(tc(
            "Lead line two.",
        ))),
    ])))
}

fn doc_with(children: Vec<SessionContent>) -> Document {
    let mut doc = Document::new();
    doc.root.children = SessionContainer::from_typed(children);
    doc
}

fn serialize(doc: &Document) -> String {
    export(doc).expect("serialization should not fail")
}

fn kind_of(item: &ContentItem) -> Option<Kind> {
    Some(match item {
        ContentItem::Paragraph(_) => Kind::Paragraph,
        ContentItem::List(_) => Kind::List,
        ContentItem::Session(_) => Kind::Session,
        ContentItem::VerbatimBlock(_) => Kind::Verbatim,
        ContentItem::Definition(_) => Kind::Definition,
        ContentItem::Annotation(_) => Kind::Annotation,
        ContentItem::Table(_) => Kind::Table,
        ContentItem::BlankLineGroup(_) => return None,
        _ => return None,
    })
}

/// Non-blank block kinds directly under the document root.
fn body_kinds(doc: &Document) -> Vec<Kind> {
    doc.root.children.iter().filter_map(kind_of).collect()
}

/// The non-blank block *items* directly under the document root (parallel to
/// `body_kinds`, but keeping the nodes so a merged shape can be asserted).
fn body_items(doc: &Document) -> Vec<&ContentItem> {
    doc.root
        .children
        .iter()
        .filter(|item| kind_of(item).is_some())
        .collect()
}

// ─── The matrix, made executable ────────────────────────────────────────────

/// The ordered pairs where the Definition does not survive as a sibling but is
/// absorbed into the following closer-terminated block. Per
/// `comms/specs/grammar-core.lex` §4.5c, a verbatim block is
/// `<verbatim-group-with-content>+` sharing ONE closing annotation (multi-group
/// verbatim). A `<definition>` (`<subject-line> <indent> content`) authored as a
/// peer immediately above a closer-terminated verbatim run is structurally
/// identical to a `<verbatim-group-with-content>` and is therefore that verbatim's
/// FIRST group — verbatim is tried before definition (parse order §4.7), and blank
/// lines do not separate groups, so the two cannot be authored as separate adjacent
/// blocks. This holds for `Definition → Verbatim` and, because `:: label ::` also
/// closes a verbatim, for `Definition → Annotation`; both are the multi-group
/// verbatim feature working AS DESIGNED (lex#814 §4, resolved as intended) and lose
/// no content.
///
/// `Definition → Table` used to be the known lex#819 content-loss exception, but
/// tables are single-group and now separate rather than merging lossily.
fn is_known_hijack(prev: Kind, next: Kind) -> bool {
    matches!(
        (prev, next),
        (Kind::Definition, Kind::Verbatim) | (Kind::Definition, Kind::Annotation)
    )
}

/// Content strings of a verbatim group's `VerbatimLine` children, in order.
fn verbatim_group_text<'a>(children: impl IntoIterator<Item = &'a ContentItem>) -> Vec<&'a str> {
    children
        .into_iter()
        .filter_map(|c| match c {
            ContentItem::VerbatimLine(line) => Some(line.content.as_string()),
            _ => None,
        })
        .collect()
}

/// Assert the exact intended merged shape for a `Definition -> next` hijack.
///
/// The definition never survives as a sibling; per grammar §4.5c it is absorbed
/// into the following closer-terminated block. Pinning the *positive* shape makes
/// the guard fail not only when the pair SEPARATES into two siblings, but also
/// when the parser drops content, produces the wrong merged kind, or absorbs the
/// wrong neighbor — the weakness codex flagged in the old negative assertions.
fn assert_definition_hijack_shape(next: Kind, reparsed: &Document, out: &str) {
    let items = body_items(reparsed);
    match next {
        Kind::Verbatim => {
            // The definition and the following marker-verbatim collapse into ONE
            // two-group verbatim sharing the following verbatim's `:: verbatim ::`
            // closer: group 0 IS the definition (subject "Term" / content
            // "Def body."), group 1 is the following verbatim's subject ("Code2").
            assert_eq!(
                body_kinds(reparsed),
                vec![Kind::Paragraph, Kind::Verbatim],
                "(Definition -> Verbatim) must merge into lead + one verbatim; got {:?}\n{out}",
                body_kinds(reparsed),
            );
            let v = match items[1] {
                ContentItem::VerbatimBlock(v) => v.as_ref(),
                other => panic!("expected a VerbatimBlock; got {other:?}\n{out}"),
            };
            let groups: Vec<_> = v.group().collect();
            assert_eq!(
                v.group_len(),
                2,
                "(Definition -> Verbatim) the definition must be the verbatim's FIRST of two \
                 groups (§4.5c); got {} group(s)\n{out}",
                v.group_len(),
            );
            assert_eq!(
                groups[0].subject.as_string(),
                "Term",
                "(Definition -> Verbatim) group 0 subject must be the definition subject\n{out}",
            );
            assert_eq!(
                verbatim_group_text(groups[0].children.iter()),
                vec!["Def body."],
                "(Definition -> Verbatim) group 0 content must be the definition body — a drop \
                 here is exactly the content loss the positive assertion guards\n{out}",
            );
            assert_eq!(
                groups[1].subject.as_string(),
                "Code2",
                "(Definition -> Verbatim) group 1 subject must be the following verbatim\n{out}",
            );
            assert_eq!(
                v.closing_data.label.value, "verbatim",
                "(Definition -> Verbatim) the shared closer must be the following verbatim's\n{out}",
            );
        }
        Kind::Annotation => {
            // The following annotation's `:: note ::` marker doubles as a verbatim
            // closer, so the definition becomes a single-group verbatim closed by
            // `:: note ::`; the annotation's own indented body is left behind as a
            // trailing paragraph sibling. Nothing is lost — content is re-homed.
            assert_eq!(
                body_kinds(reparsed),
                vec![Kind::Paragraph, Kind::Verbatim, Kind::Paragraph],
                "(Definition -> Annotation) must merge into lead + verbatim (the definition, \
                 closed by `:: note ::`) + the annotation body as a trailing paragraph; got \
                 {:?}\n{out}",
                body_kinds(reparsed),
            );
            let v = match items[1] {
                ContentItem::VerbatimBlock(v) => v.as_ref(),
                other => panic!("expected a VerbatimBlock; got {other:?}\n{out}"),
            };
            assert_eq!(
                v.group_len(),
                1,
                "(Definition -> Annotation) the definition is the verbatim's only group\n{out}",
            );
            assert_eq!(
                v.subject.as_string(),
                "Term",
                "(Definition -> Annotation) the verbatim subject must be the definition subject\n{out}",
            );
            assert_eq!(
                verbatim_group_text(v.children.iter()),
                vec!["Def body."],
                "(Definition -> Annotation) the verbatim content must be the definition body\n{out}",
            );
            assert_eq!(
                v.closing_data.label.value, "note",
                "(Definition -> Annotation) the shared closer must be the annotation's `:: note ::`\n{out}",
            );
        }
        Kind::Table => {
            panic!(
                "Definition -> Table is no longer a known hijack after lex#819; got {:?}\n{out}",
                body_kinds(reparsed),
            );
        }
        other => panic!("assert_definition_hijack_shape called with non-hijack next {other:?}"),
    }
}

#[test]
fn every_ordered_pair_reparses_as_two_blocks() {
    for &a in &ALL_KINDS {
        for &b in &ALL_KINDS {
            // A reader-shaped pair with a title-neutralizing lead, NO blank lines
            // between the blocks: the matrix alone must supply the separation.
            let doc = doc_with(vec![lead(), block(a, Role::Prev), block(b, Role::Next)]);
            let out = serialize(&doc);
            let reparsed = parse_document(&out).unwrap_or_else(|e| {
                panic!("({a:?} -> {b:?}) serialized Lex did not re-parse: {e}\n{out}")
            });
            let kinds = body_kinds(&reparsed);

            if is_known_hijack(a, b) {
                // Regression guard for the Definition-absorbing adjacencies. Rather
                // than the weak "does not equal the separated shape" check (which
                // codex flagged — it would pass even if the parser dropped content
                // or produced the wrong merged block), assert the POSITIVE merged
                // shape. `assert_definition_hijack_shape` pins the lossless §4.5c
                // merges (Verbatim/Annotation). If either separates OR the merged
                // shape is wrong, the guard fails.
                assert_definition_hijack_shape(b, &reparsed, &out);
                continue;
            }

            // Bare Annotation siblings do not round-trip as siblings (the parser
            // attaches or hoists a floating annotation) — an orthogonal parser
            // behavior the matrix cannot own. Assert only that the *other* block
            // and the lead survive; the annotation boundary is tested separately.
            if a == Kind::Annotation || b == Kind::Annotation {
                let non_annotation = if a == Kind::Annotation { b } else { a };
                assert!(
                    kinds.contains(&non_annotation) || non_annotation == Kind::Annotation,
                    "({a:?} -> {b:?}) the non-annotation block must survive; got {kinds:?}\n{out}"
                );
                continue;
            }

            assert_eq!(
                kinds,
                vec![Kind::Paragraph, a, b],
                "({a:?} -> {b:?}) must re-parse as two separate blocks after the lead; got {kinds:?}\n\
                 --- serialized Lex ---\n{out}"
            );
        }
    }
}

#[test]
fn definition_before_closer_led_block_is_a_known_hijack() {
    // Explicit regression guard that a Definition immediately above a
    // closer-terminated Verbatim/Annotation is absorbed rather than kept as a
    // sibling. Both are intended multi-group behavior (grammar §4.5c):
    // Definition → Verbatim makes the definition the verbatim's first group under
    // the shared closer; Definition → Annotation uses the `:: label ::` marker as
    // a verbatim closer, so the definition becomes a verbatim closed by it and the
    // annotation body spills out as a trailing paragraph. Definition → Table used
    // to live here as lex#819's known content-loss bug, but a table is
    // single-group and now separates instead.
    for next in [Kind::Verbatim, Kind::Annotation] {
        let doc = doc_with(vec![
            lead(),
            block(Kind::Definition, Role::Prev),
            block(next, Role::Next),
        ]);
        let out = serialize(&doc);
        let reparsed = parse_document(&out).expect("reparse");
        assert_definition_hijack_shape(next, &reparsed, &out);
    }
}

// ─── Faithfulness of reader-shaped mixed documents ──────────────────────────

/// Serialize a reader-shaped document, re-parse it, and assert the Skeleton is
/// unchanged (the Faithfulness invariant, applied to a hand-built AST rather than
/// via a foreign Reader).
fn assert_faithful(doc: &Document, label: &str) {
    let out = serialize(doc);
    let reparsed = parse_document(&out)
        .unwrap_or_else(|e| panic!("[{label}] serialized Lex did not re-parse: {e}\n{out}"));
    let want = canon(doc);
    let got = canon(&reparsed);
    assert_eq!(
        want, got,
        "[{label}] not faithful\n--- serialized Lex ---\n{out}\n--- want ---\n{want:#?}\n--- got ---\n{got:#?}"
    );
}

fn para(text: &str) -> SessionContent {
    SessionContent::Element(ContentElement::Paragraph(Paragraph::from_line(text.into())))
}

fn ordered_list() -> SessionContent {
    let mut list = List::new(vec![
        ListItem::new("1.".into(), "first".into()),
        ListItem::new("2.".into(), "second".into()),
    ]);
    list.marker = SequenceMarker::parse("1.", None);
    SessionContent::Element(ContentElement::List(list))
}

fn nested_list() -> SessionContent {
    let mut inner = List::new(vec![
        ListItem::new("-".into(), "child a".into()),
        ListItem::new("-".into(), "child b".into()),
    ]);
    inner.marker = SequenceMarker::parse("-", None);
    let outer_item = ListItem::with_content(
        "-".into(),
        "parent".into(),
        vec![ContentElement::List(inner)],
    );
    let mut outer = List::new(vec![
        outer_item,
        ListItem::new("-".into(), "sibling".into()),
    ]);
    outer.marker = SequenceMarker::parse("-", None);
    SessionContent::Element(ContentElement::List(outer))
}

fn verbatim_with_trailing() -> SessionContent {
    let data = Data::new(Label::from_string("rust"), vec![]);
    let v = lex_core::lex::ast::Verbatim::new(
        tc("Example"),
        vec![
            VerbatimContent::VerbatimLine(VerbatimLine::new("fn main() {}".into())),
            VerbatimContent::VerbatimLine(VerbatimLine::new("// trailing content".into())),
        ],
        data,
        VerbatimBlockMode::Inflow,
    );
    SessionContent::Element(ContentElement::VerbatimBlock(Box::new(v)))
}

fn definition(subject: &str, body: &str) -> SessionContent {
    SessionContent::Element(ContentElement::Definition(Definition::new(
        tc(subject),
        vec![ContentElement::Paragraph(Paragraph::from_line(body.into()))],
    )))
}

fn table_with_footnotes() -> SessionContent {
    let mut table = closed_table();
    // A footnote list inside the table block (the interior-scope catch from #781).
    let mut footnotes = List::new(vec![
        ListItem::new("1.".into(), "first note".into()),
        ListItem::new("2.".into(), "second note".into()),
    ]);
    footnotes.marker = SequenceMarker::parse("1.", None);
    table.footnotes = Some(Box::new(footnotes));
    SessionContent::Element(ContentElement::Table(Box::new(table)))
}

#[test]
fn faithful_paragraphs_lists_verbatim_definitions() {
    // A reader-shaped document mixing the faithful block kinds, no BlankLineGroups.
    let doc = doc_with(vec![
        lead(),
        para("A body paragraph."),
        block(Kind::List, Role::Prev),
        ordered_list(),
        para("Between the list and the verbatim."),
        verbatim_with_trailing(),
        definition("Term", "Definition body."),
        para("Trailing paragraph."),
    ]);
    assert_faithful(&doc, "mixed faithful blocks");
}

#[test]
fn faithful_nested_lists() {
    let doc = doc_with(vec![lead(), nested_list(), para("After the nested list.")]);
    assert_faithful(&doc, "nested lists");
}

#[test]
fn faithful_blocks_nested_in_a_session_body() {
    // Sibling scopes must isolate: the pair lives inside a session body, indented.
    let session = Session::new(
        tc("Container"),
        vec![
            para("First in body."),
            verbatim_with_trailing(),
            para("Second in body."),
            ordered_list(),
        ],
    );
    let doc = doc_with(vec![SessionContent::Session(session)]);
    assert_faithful(&doc, "blocks in session body");
}

#[test]
fn faithful_blocks_nested_in_a_list_item_body() {
    // A list item whose body carries its own sibling blocks (paragraph + verbatim).
    let item = ListItem::with_content(
        "-".into(),
        "item with body".into(),
        vec![ContentElement::Paragraph(Paragraph::from_line(
            "Body paragraph.".into(),
        ))],
    );
    let mut list = List::new(vec![item, ListItem::new("-".into(), "plain item".into())]);
    list.marker = SequenceMarker::parse("-", None);
    let doc = doc_with(vec![
        lead(),
        SessionContent::Element(ContentElement::List(list)),
        para("After the list."),
    ]);
    assert_faithful(&doc, "blocks in list-item body");
}

#[test]
fn faithful_table_with_footnotes() {
    let doc = doc_with(vec![
        lead(),
        table_with_footnotes(),
        para("After the table."),
    ]);
    assert_faithful(&doc, "table with footnotes");
}

#[test]
fn faithful_definition_followed_by_each_non_hijack_block() {
    // A definition immediately followed by every block type EXCEPT Verbatim/Table
    // (the documented hijacks). Each adjacency must round-trip.
    for next in [Kind::Paragraph, Kind::List, Kind::Session, Kind::Definition] {
        let doc = doc_with(vec![
            lead(),
            definition("Term", "Definition body."),
            block(next, Role::Next),
        ]);
        assert_faithful(&doc, &format!("definition -> {next:?}"));
    }
}

// ─── Annotation boundaries (the lex#682 band-aid, now matrix-owned) ──────────

#[test]
fn annotation_with_body_does_not_swallow_the_following_block() {
    // The boundary the removed lex#682 band-aid protected: a block annotation's
    // indented body must not pull the next sibling in. The matrix's
    // `AnnotationBody -> *` = 1 emits the trailing blank. (The annotation itself may
    // be re-attached by the parser — that is the orthogonal attachment behavior;
    // here we assert only that the following paragraph survives as its own block.)
    let doc = doc_with(vec![
        lead(),
        block(Kind::Annotation, Role::Prev), // annotation WITH body
        para("Following paragraph."),
    ]);
    let out = serialize(&doc);
    // The trailing blank after the annotation body must be present so a re-parse
    // cannot fold "Following paragraph." into the annotation's indented body.
    assert!(
        out.contains("Ann body.\n\nFollowing paragraph."),
        "expected a blank line after the annotation body; got:\n{out}"
    );
    let reparsed = parse_document(&out).expect("reparse");
    assert!(
        body_kinds(&reparsed).contains(&Kind::Paragraph),
        "the following paragraph must survive as its own block; got {:?}\n{out}",
        body_kinds(&reparsed)
    );
}

#[test]
fn marker_annotation_without_body_needs_no_indent_dance() {
    // A marker-form annotation (no body) round-trips its label as a leading
    // document annotation; assert serialization is stable and re-parses.
    let doc = doc_with(vec![SessionContent::Element(ContentElement::Annotation(
        Annotation::new(Label::from_string("note"), vec![], vec![]),
    ))]);
    let out = serialize(&doc);
    assert_eq!(out, ":: note ::\n");
    parse_document(&out).expect("marker annotation must re-parse");
}

// ─── Composition with BlankLineGroup (max, never additive) ──────────────────

fn blanks(count: usize) -> SessionContent {
    SessionContent::Element(ContentElement::BlankLineGroup(
        lex_core::lex::ast::elements::BlankLineGroup {
            count,
            source_tokens: vec![],
            location: Default::default(),
        },
    ))
}

/// Blank lines between the first occurrence of `before` and the next `after`.
fn blanks_between(s: &str, before: &str, after: &str) -> usize {
    let start = s.find(before).expect("`before` present") + before.len();
    let end = s[start..].find(after).expect("`after` present") + start;
    s[start..end].matches('\n').count().saturating_sub(1)
}

#[test]
fn blank_line_group_composes_max_against_min_one_cell() {
    // Paragraph -> Paragraph has a structural minimum of 1. max(1, k), clamped to
    // the formatter's max_blank_lines = 2. k = 1 is the additive tell.
    for (k, want) in [(0usize, 1usize), (1, 1), (2, 2), (5, 2)] {
        let doc = doc_with(vec![para("A."), blanks(k), para("B.")]);
        let out = serialize(&doc);
        assert_eq!(
            blanks_between(&out, "A.", "B."),
            want,
            "min-1 cell with BlankLineGroup({k}) should yield {want} blank(s); got:\n{out}"
        );
    }
}

#[test]
fn blank_line_group_composes_max_against_min_zero_cell() {
    // List -> Paragraph has a structural minimum of 0, so the BlankLineGroup is
    // the sole source of separation: the output carries exactly k (clamped to 2).
    for (k, want) in [(0usize, 0usize), (1, 1), (2, 2), (5, 2)] {
        let doc = doc_with(vec![
            lead(),
            block(Kind::List, Role::Prev),
            blanks(k),
            para("After list."),
        ]);
        let out = serialize(&doc);
        assert_eq!(
            blanks_between(&out, "item two", "After list."),
            want,
            "min-0 cell with BlankLineGroup({k}) should yield {want} blank(s); got:\n{out}"
        );
    }
}

// ─── Property: reader-shaped sibling sequences keep their block-type order ───

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(128))]

    /// An arbitrary sequence of reader-shaped sibling blocks (NO BlankLineGroups)
    /// serializes and re-parses with the same block-type sequence. Drawn from the
    /// six kinds that round-trip as bare siblings (Annotation is excluded — bare
    /// annotation siblings re-attach, an orthogonal parser behavior). The
    /// `is_known_hijack` merge adjacency is skipped: Definition→Verbatim, the
    /// intended multi-group behavior (§4.5c). Definition→Table used to be a
    /// lossy member of this family, but tables are single-group and now separate
    /// normally (lex#819). #784 will grow this into the full reader-content proptest.
    #[test]
    fn reader_shaped_sibling_sequence_preserves_block_types(
        indices in proptest::collection::vec(0usize..6, 1..7),
    ) {
        let kinds: Vec<Kind> = indices
            .iter()
            .map(|&i| [
                Kind::Paragraph,
                Kind::List,
                Kind::Session,
                Kind::Verbatim,
                Kind::Definition,
                Kind::Table,
            ][i])
            .collect();

        // Skip the intended merge adjacencies (multi-group verbatim, grammar §4.5c)
        // via prop_assume! rather than a silent early return, so a rejected draw is
        // recorded as a proptest rejection instead of counting as a passing case.
        for pair in kinds.windows(2) {
            proptest::prop_assume!(!is_known_hijack(pair[0], pair[1]));
        }

        // Lead paragraph neutralizes the document-title boundary (slice #783).
        let mut children = vec![lead()];
        children.extend(kinds.iter().map(|&k| block(k, Role::Next)));
        let doc = doc_with(children);

        let out = serialize(&doc);
        let reparsed = parse_document(&out)
            .unwrap_or_else(|e| panic!("did not re-parse: {e}\n{out}"));

        let mut expected = vec![Kind::Paragraph];
        expected.extend(kinds.iter().copied());
        proptest::prop_assert_eq!(
            body_kinds(&reparsed),
            expected,
            "block-type sequence must survive serialize -> reparse; Lex was:\n{}",
            out
        );
    }
}
