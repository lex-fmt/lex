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
//! Intended group semantics are characterized rather than asserted faithful
//! (see `separation.rs` module docs and `definition_before_closer_led_block_is_a_
//! known_hijack`): per `comms/specs/grammar-core.lex` §4.5c, a Definition
//! immediately before a closer-terminated Verbatim (or a Table/Annotation whose
//! `:: label ::` marker doubles as a verbatim closer) is that block's FIRST group.
//! A definition's `subject:` + indented body is structurally identical to a
//! `<verbatim-group-with-content>`, and verbatim is tried before definition (§4.7),
//! so the two cannot be authored as separate adjacent blocks — blank lines do not
//! separate groups. This is the multi-group verbatim feature working as designed
//! (lex#814 §4, resolved as intended), not a bug. Bare Annotation siblings are also not
//! round-trippable as siblings (the parser attaches a floating annotation to a
//! neighbor or the document head), so they are excluded from the sibling-sequence
//! properties; the matrix still owns the lex#682 trailing-blank boundary, tested
//! directly.

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

// ─── The matrix, made executable ────────────────────────────────────────────

/// The ordered pairs that merge by design, not by a separation gap. Per
/// `comms/specs/grammar-core.lex` §4.5c, a verbatim block is
/// `<verbatim-group-with-content>+` sharing ONE closing annotation (multi-group
/// verbatim). A `<definition>` (`<subject-line> <indent> content`) authored as a
/// peer immediately above a closer-terminated verbatim run is structurally
/// identical to a `<verbatim-group-with-content>` and is therefore that verbatim's
/// FIRST group — verbatim is tried before definition (parse order §4.7), and blank
/// lines do not separate groups, so the two cannot be authored as separate adjacent
/// blocks. The same closer re-anchoring absorbs the Definition ahead of a Table or
/// an Annotation closer (`:: label ::` is a valid verbatim closer), so all three
/// `Definition → …` pairs merge. This is the multi-group verbatim feature working
/// AS DESIGNED (lex#814 §4, resolved as intended), NOT a bug to fix. Documented in
/// `separation.rs`.
fn is_known_hijack(prev: Kind, next: Kind) -> bool {
    matches!(
        (prev, next),
        (Kind::Definition, Kind::Verbatim)
            | (Kind::Definition, Kind::Table)
            | (Kind::Definition, Kind::Annotation)
    )
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
                // Regression guard for the multi-group verbatim feature: the pair
                // MUST still merge (the Definition is the verbatim's first group per
                // grammar §4.5c). If it ever separates, multi-group verbatim has
                // regressed — the opposite of a fix.
                assert_ne!(
                    kinds,
                    vec![Kind::Paragraph, a, b],
                    "({a:?} -> {b:?}) must still merge — a definition adjacent to a \
                     closer-terminated verbatim is that verbatim's first group per the \
                     multi-group grammar (§4.5c); if this separates, multi-group verbatim \
                     has regressed.\n{out}"
                );
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
    // Explicit regression guard for the intended group semantics (grammar §4.5c):
    // a Definition immediately above a closer-terminated block is that block's
    // first verbatim group. A Verbatim and a Table supply the shared `:: label ::`
    // closer directly; an Annotation's `:: label ::` marker is *also* a valid
    // verbatim closer, so it absorbs the definition too. This is multi-group
    // verbatim working as designed — no blank count in the matrix changes it, and
    // the pair MUST stay merged. If it ever separates, multi-group verbatim has
    // regressed and the matrix docs must be revisited.
    for next in [Kind::Verbatim, Kind::Table, Kind::Annotation] {
        let doc = doc_with(vec![
            lead(),
            block(Kind::Definition, Role::Prev),
            block(next, Role::Next),
        ]);
        let out = serialize(&doc);
        let reparsed = parse_document(&out).expect("reparse");
        let kinds = body_kinds(&reparsed);
        // The Definition never survives: its subject is consumed as the subject of
        // the verbatim the matcher synthesizes around the re-anchored closer.
        assert!(
            !kinds.contains(&Kind::Definition),
            "Definition -> {next:?} must stay merged — the definition subject is the \
             verbatim's first group per grammar §4.5c; if it survives as a sibling, \
             multi-group verbatim has regressed; got {kinds:?}\n{out}"
        );
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
    /// annotation siblings re-attach, an orthogonal parser behavior), and the
    /// intended Definition→Verbatim / Definition→Table merge adjacencies (grammar
    /// §4.5c) are skipped. #784 will grow this into the full reader-content proptest.
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

        // Skip the intended merge adjacencies (multi-group verbatim, grammar §4.5c).
        for pair in kinds.windows(2) {
            if is_known_hijack(pair[0], pair[1]) {
                return Ok(());
            }
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
