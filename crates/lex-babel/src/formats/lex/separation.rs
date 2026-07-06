//! Separation matrix — the single, named home for "how many blank lines must
//! sit between two adjacent sibling Blocks" in serialized Lex.
//!
//! In Lex a blank line is *load-bearing*: two adjacent text lines with no blank
//! between them parse as ONE two-line Paragraph, while the same lines with a
//! blank between them parse as TWO Paragraphs. Block Separation is therefore a
//! structural property of the grammar, not decoration (see CONTEXT.md and
//! `docs/adr/0001-lex-serializer-structural-block-separation.md`).
//!
//! The serializer emits this separation *structurally* — derived from the block
//! structure — rather than reading it out of `BlankLineGroup` nodes. Readers
//! other than lex-core (Markdown, RFC-XML, …) build ASTs with no
//! `BlankLineGroup`s, so a serializer that separated blocks only when it saw one
//! jammed their blocks together and re-parsed them wrong. Emitting from the
//! matrix fixes every reader at the one shared choke point.
//!
//! Composition is **max-composing** (`ensure_blank_lines`, an ensure-at-least-N,
//! never additive): a structural minimum of `n` alongside a `BlankLineGroup(k)`
//! yields `max(n, k)`. Because a lex-sourced AST already carries a
//! `BlankLineGroup` ≥ the structural minimum between blocks, `lex → lex` output
//! is byte-identical — the matrix only ever adds separation a reader omitted.
//!
//! # How the matrix was derived (lex#782)
//!
//! Every cell was derived from — and is verified against — the lex-core parser's
//! actual behavior, not the prose grammar (where the two disagree, the parser
//! wins; the disagreements are called out below). The verification lives in
//! `tests/lex_separation/matrix.rs::every_ordered_pair_reparses_as_two_blocks`,
//! which builds a minimal reader-shaped document for each ordered pair, serializes
//! it, re-parses, and asserts both blocks survive with the right types.
//!
//! The requirement is governed by two independent parser mechanisms:
//!
//! 1. **Forward absorption** — does `next`'s opening line merge into `prev`?
//!    `prev` is *absorbing* only when it ends with an open line at the sibling
//!    indent: a Paragraph (a bare text line) or a List (an item line). Blocks that
//!    end with a dedent (Session, Definition body) or a `:: label ::` closer
//!    (Verbatim, Table) present a hard boundary and absorb nothing. `next` is
//!    *absorbable* when it opens with a plain line the paragraph look-ahead does
//!    NOT yield before: a Paragraph (text), a Session (title line), or a
//!    **marker-form Verbatim** (`subject:` with no indented body — the look-ahead
//!    only yields before `subject:` *followed by an indent*). A List, a Definition,
//!    a Table, and a block-form Verbatim all open with a construct the look-ahead
//!    detects structurally, so they are NOT absorbed and need no blank.
//!
//! 2. **Multi-group verbatim absorption (intended group semantics, NOT a
//!    separation gap)** — the verbatim/table matcher runs at the highest precedence
//!    and pairs the *first* `subject:` line it sees with the *next* `:: label ::`
//!    closer, spanning blanks and dedents. This is the multi-group verbatim
//!    production (`comms/specs/grammar-core.lex` §4.5c): a verbatim block is
//!    `<verbatim-group-with-content>+` sharing ONE closing annotation. A
//!    **Definition** (`subject:` + indented body, no closer of its own) placed
//!    immediately before any block that presents a `:: label ::` line — a Verbatim,
//!    a Table, or even an Annotation (`:: label ::` is a valid verbatim closer) — is
//!    structurally identical to a `<verbatim-group-with-content>`, and verbatim is
//!    tried before definition (§4.7), so it becomes that block's FIRST group. No
//!    blank count changes this: blank lines do not separate groups. The three cells
//!    (`Definition → Verbatim`, `Definition → Table`, `Definition → Annotation`) are
//!    marked below; their value is the structural boundary minimum (0, the
//!    definition's dedent), and the merge is intended (lex#814 §4, resolved as
//!    intended), not a bug. The verification test characterizes the merge as a
//!    regression guard rather than asserting faithfulness for those cells.
//!
//! ## Spec/parser disagreements found
//!
//! - `grammar-core.lex` says a blank before a list is *optional* (0). The parser
//!   agrees: every `* → List` cell is 0 **except `List → List`**, where two
//!   list blocks with no blank between them merge into one list (a blank
//!   terminates the first list, per the grammar's own "blank between items
//!   terminates the list" rule), so `List → List` = 1.
//! - `grammar-core.lex` frames the session title→body blank as the session's
//!   internal separator. Independently, the parser needs a blank *before* a
//!   session title when the preceding sibling ends with a `:: label ::` closer
//!   (Verbatim, Table) or is an open Paragraph — otherwise the title line is not
//!   recognized as a session start (it degrades to a paragraph). Hence
//!   `Paragraph → Session`, `Verbatim → Session`, and `Table → Session` = 1,
//!   while `List/Session/Definition → Session` = 0 (a dedent or item-line boundary
//!   is enough).
//!
//! ## Annotations (two kinds by shape)
//!
//! Standalone block Annotations are a special case the matrix cannot fully own:
//! the parser attaches a floating annotation to a neighboring element (or, at the
//! document head, promotes it to a document-level annotation), so an Annotation
//! placed as a bare body sibling does not round-trip as a sibling regardless of
//! separation. What the matrix *does* own is the boundary the lex#682 band-aid
//! used to patch — but only for the shape that has it: an annotation with an
//! indented body needs a trailing blank so the following sibling is not pulled
//! into that body. A **marker-form** annotation (no body) ends with a closed
//! `:: label ::` and separates exactly like a Verbatim/Table closer. The two
//! shapes are therefore distinct `BlockKind`s: `AnnotationBody → *` = 1 (the
//! lex#682 boundary), while marker `Annotation → *` mirrors the Verbatim row
//! (only a following Session needs a blank). Getting this split right is what
//! keeps `lex → lex` byte-identical for documents that list several marker
//! annotations back-to-back (e.g. `elements/annotation.lex`, where the old
//! band-aid never fired because those annotations have no body).

/// A sibling Block that can appear in a Lex container body — the units that must
/// be *separated* from one another in surface syntax (CONTEXT.md, "Block").
///
/// Annotations split by shape — `Annotation` is the marker form (`:: label ::`,
/// no body) and `AnnotationBody` is the block form with an indented body —
/// because their *trailing* separation requirement differs (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockKind {
    Paragraph,
    List,
    Session,
    Verbatim,
    Definition,
    Annotation,
    AnnotationBody,
    Table,
}

/// Minimum blank lines the grammar requires between an ordered pair of adjacent
/// sibling blocks `(prev, next)`, derived from and verified against the lex-core
/// parser (see the module docs for the derivation and the two governing
/// mechanisms). This is the separation matrix; every ordered pair of the eight
/// `BlockKind`s has an explicit, tested entry.
pub(super) fn min_blank_lines(prev: BlockKind, next: BlockKind) -> usize {
    use BlockKind::*;
    match (prev, next) {
        // ── Paragraph → * ────────────────────────────────────────────────────
        // A Paragraph ends with an open text line, so any absorbable `next`
        // merges into it: another Paragraph (→ one 2-line paragraph), a Session
        // title, or a marker-form Verbatim subject.
        (Paragraph, Paragraph) => 1,
        (Paragraph, Session) => 1,
        (Paragraph, Verbatim) => 1,
        (Paragraph, List) => 0,
        (Paragraph, Definition) => 0,
        (Paragraph, Annotation) => 0,
        (Paragraph, AnnotationBody) => 0,
        (Paragraph, Table) => 0,

        // ── List → * ─────────────────────────────────────────────────────────
        // A List ends with an item line. Only a following List merges (its items
        // append to the first); every other `next` starts its own block.
        (List, List) => 1,
        (List, _) => 0,

        // ── Session → * ──────────────────────────────────────────────────────
        // A Session ends with a dedent — a hard boundary that absorbs nothing.
        (Session, _) => 0,

        // ── Verbatim → * ─────────────────────────────────────────────────────
        // A Verbatim ends with a `:: label ::` closer. That closes the block, but
        // the parser does not treat the post-closer position as a session-start
        // boundary, so a following Session title still needs a blank.
        (Verbatim, Session) => 1,
        (Verbatim, _) => 0,

        // ── Definition → * ───────────────────────────────────────────────────
        // A Definition ends with a dedent (boundary minimum 0). Its `subject:` +
        // indented body IS a `<verbatim-group-with-content>`, so when it sits
        // immediately above a closer-terminated block the shared `:: label ::`
        // closer makes it that verbatim's first group (see module docs). Per grammar
        // §4.5c this is intended multi-group verbatim, not a separation gap —
        // Definition → Verbatim / Table / Annotation merge by design and the value
        // stays the boundary minimum.
        (Definition, Verbatim) => 0, // merges as the verbatim's first group (multi-group verbatim)
        (Definition, Table) => 0,    // merges into the table under the shared closer
        // Both annotation shapes present a `:: label ::` line (the marker *is* one;
        // the block form opens with one), and either serves as the shared verbatim closer.
        (Definition, Annotation) | (Definition, AnnotationBody) => 0, // merges: `:: label ::` is the shared verbatim closer
        (Definition, _) => 0,

        // ── Annotation (marker form) → * ─────────────────────────────────────
        // A marker annotation ends with a closed `:: label ::`, the same boundary
        // shape as a Verbatim/Table closer: only a following Session needs a
        // blank. (The old lex#682 band-aid never fired for this shape — it has no
        // body — so keeping this row at the Verbatim values preserves the
        // byte-for-byte output for back-to-back marker annotations.)
        (Annotation, Session) => 1,
        (Annotation, _) => 0,

        // ── AnnotationBody (block form) → * ──────────────────────────────────
        // A block annotation's indented body would pull the next sibling in
        // without a trailing blank (subsumes the lex#682 band-aid, which fired
        // exactly for this shape).
        (AnnotationBody, _) => 1,

        // ── Table → * ────────────────────────────────────────────────────────
        // A Table ends with a `:: label ::` closer, same boundary shape as a
        // Verbatim: only a following Session title needs a blank.
        (Table, Session) => 1,
        (Table, _) => 0,
    }
}
