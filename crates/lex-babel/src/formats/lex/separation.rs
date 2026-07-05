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
//! Slice #781 is the walking skeleton: it wires ONLY the paragraph→paragraph
//! cell. Every other pair returns 0 ("no structural minimum" — that pair's
//! behavior is exactly as today) and is filled in by slice #782, which also
//! deletes the per-block band-aids (lex#505 verbatim, lex#682 annotation) the
//! general rule subsumes. Filling the matrix is a one-function edit here; no
//! serializer restructuring is needed.

/// A sibling Block that can appear in a Lex container body — the units that must
/// be *separated* from one another in surface syntax (CONTEXT.md, "Block").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockKind {
    Paragraph,
    List,
    Session,
    Verbatim,
    Definition,
    Annotation,
    Table,
}

/// Minimum blank lines the grammar requires between an ordered pair of adjacent
/// sibling blocks `(prev, next)`.
///
/// This is the separation matrix. Slice #781 wires only `(Paragraph, Paragraph)`
/// = 1; every other pair returns 0. Slice #782 fills the remaining cells.
pub(super) fn min_blank_lines(prev: BlockKind, next: BlockKind) -> usize {
    use BlockKind::*;
    match (prev, next) {
        (Paragraph, Paragraph) => 1,
        // TODO(#782): fill in the remaining cells (paragraph→list, list→paragraph,
        // session→body, paragraph→verbatim, paragraph→definition, …) and delete
        // the per-block blank emitters (lex#505 verbatim, lex#682 annotation)
        // this rule subsumes.
        _ => 0,
    }
}
