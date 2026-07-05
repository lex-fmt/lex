# PRD: Markdown ↔ Lex lossless round-trip for equivalent constructs

> Status: ready-for-agent (unpublished — file only)
> Related: `docs/adr/0001-lex-serializer-structural-block-separation.md`, `CONTEXT.md`, [interop-scope.lex](https://github.com/lex-fmt/comms/blob/main/docs/interop-scope.lex) (`lex-fmt/comms` repo)

## Problem Statement

I convert a Markdown document to Lex and the structure falls apart. Two
paragraphs become one. A document that starts with a `# Heading` loses the
heading — it silently merges into the following text. A bulleted list comes out
as a bare `-` line with the item text stranded on the next line. When I convert
that generated Lex back to Markdown, or just re-open it, it is not the document I
started with.

[`interop-scope.lex`](https://github.com/lex-fmt/comms/blob/main/docs/interop-scope.lex)
(in the `lex-fmt/comms` repo, mounted here as the `comms/` submodule) states that
Markdown is Lex's lingua franca and
that "round-trip discipline is the bar, and is what we measure regressions
against." Today that bar is not met: a Markdown document built from ordinary,
Lex-representable constructs does not survive the trip through Lex.

## Solution

Markdown → Lex produces Lex text that, when re-parsed, is the *same document* the
Markdown reader understood — for every construct both formats can represent
(paragraphs, headings, lists, definitions, code blocks, and inline emphasis /
code). Blocks stay separate. A leading heading becomes and remains the document
title. Lists stay lists. Round-tripping a document composed of these shared
constructs changes nothing structural.

Constructs that only Lex has (annotations, citation references, sessions deeper
than Markdown's six heading levels) remain **Declared Lossy** — Lex is more
expressive than Markdown, so a perfect round-trip there is out of scope by
definition. The guarantee is scoped to the shared vocabulary (the Equivalence
Set), and within that vocabulary it is exact.

## User Stories

1. As an author converting Markdown to Lex, I want two blank-line-separated
   paragraphs to remain two paragraphs, so that my document's structure is
   preserved.
2. As an author, I want a Markdown document that begins with `# Title` to produce
   a Lex document whose title is "Title", so that the heading is not lost.
3. As an author, I want that title to survive re-parsing (a blank line follows
   it), so that converting and re-opening does not silently merge the title into
   the first paragraph.
4. As an author of a Markdown document with no leading heading, I want the
   converted Lex to record that the document has no title (via `:: doc.untitled ::`),
   so that my first paragraph stays a body paragraph and is never turned into a
   heading on the way back to Markdown.
5. As an author, I want an unordered list to convert to a valid Lex list whose
   items round-trip, so that my list is not flattened into a bare marker plus
   orphaned text.
6. As an author, I want an ordered list to keep its ordering and markers, so that
   numbered steps stay numbered.
7. As an author, I want nested lists to keep their nesting, so that sub-points
   stay under their parents.
8. As an author, I want a fenced code block to become a Lex verbatim block and
   back, so that my code samples are preserved verbatim.
9. As an author, I want inline code (`` `x` ``), bold, and italic to survive the
   round-trip, so that inline emphasis is not lost or corrupted.
10. As an author, I want a definition-style construct to round-trip as a Lex
    definition, so that term/description pairs keep their meaning.
11. As an author, I want a paragraph immediately followed by a list (no blank
    line in my Markdown) to still convert correctly, so that tight Markdown is
    handled.
12. As an author, I want a heading followed by body content to keep the content
    nested under the resulting session, so that document hierarchy is preserved.
13. As an author converting a large real-world Markdown file (README, reference
    doc), I want the whole document to round-trip structurally, so that I can
    trust the converter on non-trivial input.
14. As a Lex author exporting to Markdown, I want a titled Lex document to become
    a `# H1`-led Markdown document, so that the title reads as a heading.
15. As a Lex author, I want a document composed of Equivalence-Set constructs to
    survive Lex → Markdown → Lex unchanged, so that exporting for collaboration
    and re-importing is safe.
16. As a contributor using the RFC-XML importer, I want its output to serialize
    to valid, separated Lex too, so that the fix is not Markdown-specific.
17. As a maintainer, I want a single Faithfulness invariant that fails loudly when
    any reader-built document serializes to Lex that re-parses differently, so
    that this class of bug cannot regress silently.
18. As a maintainer, I want the property test to generate reader-shaped documents
    (blocks with no pre-inserted blank-line separators), so that the test
    exercises the real failure mode instead of masking it.
19. As a maintainer, I want existing `lex → lex` output to be byte-for-byte
    unchanged by this fix, so that the formatter and all Lex-sourced pipelines
    are not disturbed.
20. As a maintainer, I want the per-block blank-line special cases (verbatim
    lex#505, annotation lex#682) removed once the general rule subsumes them, so
    that the serializer has one separation rule rather than scattered patches.
21. As an author, I want multiple consecutive blank lines to be acceptably
    normalized to a single separator, so that blank-run collapsing is understood
    and expected rather than surprising.
22. As an author, I want inline code that contains a backtick (Markdown's
    ``double-backtick`` form) to degrade predictably rather than emit Lex that
    re-parses into corrupted text, so that an unrepresentable case fails cleanly.

## Implementation Decisions

- **Deep fix in the Lex Serializer (`lex-babel` lex format serializer).** Block
  separation is emitted by the serializer as a structural property of the
  grammar, not read out of `BlankLineGroup` nodes. This fixes every reader
  (Markdown, RFC-XML, future) at the one shared choke point. See ADR-0001.

- **Separation matrix.** Introduce an explicit rule for the minimum blank-line
  separation required between each ordered pair of sibling block types
  (paragraph→paragraph = 1; paragraph→list = 0; title→first-block = 1; etc.).
  The matrix is the single, named home for "what separates block A from block B",
  derived from and verified against the lex-core parser's actual behavior.

- **Max-composition with `BlankLineGroup`.** The serializer's blank emission is
  max-composing (ensure-at-least-N semantics, not additive). Applying a
  structural minimum alongside an existing `BlankLineGroup(count = k)` yields
  `max(minimum, k)`. Because Lex-sourced ASTs already carry a `BlankLineGroup`
  ≥ the minimum between blocks, **`lex → lex` output is byte-identical** to
  today. `BlankLineGroup` is retained, re-scoped to mean "extra blanks beyond the
  structural minimum".

- **Document title blank.** A `Document.title` is followed by the structural
  separator (the title→first-block cell of the matrix), so an `# H1`-led
  Markdown document round-trips with its title intact.

- **Document title model (ADR-0002).** The title is the first content element
  when it is a paragraph (one line, or two lines when the first ends with a colon
  — title + subtitle); leading blank lines are irrelevant. A titled-format source
  (Markdown) that has **no** title round-trips via an explicit **no-title marker**
  `:: doc.untitled ::` — a registered `doc.*` builtin the **parser** honors to
  suppress title promotion. The Markdown Reader emits it for a heading-less
  source; the parser and serializer both respect it. This replaces the earlier
  "leading-blank title escape" idea (a whitespace trick that `lexd format` would
  strip). See ADR-0002.

- **Delete the band-aids.** The verbatim (lex#505) and annotation (lex#682)
  special-case blank emitters are removed; the general separation rule subsumes
  them.

- **Scope note on lex-core.** The block-separation fix lives entirely in the babel
  Lex serializer (no lex-core change). The title model (ADR-0002) *does* change
  lex-core — the parser drops leading-blank title suppression and honors the new
  `doc.untitled` builtin — and is tracked as its own slice.

- **Formatter soundness (resolved by ADR-0002).** An earlier plan carried a
  concern that `lexd format` strips a leading blank and re-promotes a title-less
  document's first line to a title. Under the settled title model that concern
  dissolves: leading blanks no longer carry title meaning (so stripping them is
  meaning-preserving), and "no title" is expressed by `:: doc.untitled ::`, which
  the formatter preserves as real content. No separate formatter follow-up is
  needed.

## Testing Decisions

- **What a good test asserts here:** external behavior — that a document read by
  any reader, serialized to Lex, and re-parsed, is the *same document*
  (structurally). Tests compare **Skeletons** (title + block structure + inline
  content, ignoring blank-*count* decoration), never serializer internals or
  exact byte output (except the one deliberate byte-stability check on
  `lex → lex`).

- **Single seam — reuse `canon()`.** The Faithfulness invariant is expressed with
  the existing `canon(&Document) -> Canon` skeleton reducer from the
  `format_invariants` test module (it already captures title, subtitle,
  annotations, and children). The invariant under test:
  `canon(md_read(src)) == canon(parse(serialize(md_read(src))))`. This is the
  direct sibling of the existing `check_semantic_preserved`
  (`canon(parse(D)) == canon(parse(format(D)))`). `canon()` is promoted to a
  shared test-support location so both the format-invariant tests and the new
  conversion-faithfulness tests use one comparator.

- **Modules tested:** the Lex serializer (via the invariant) and the
  Markdown→Lex path end-to-end (via the real reader over fixtures). RFC-XML is
  covered opportunistically by the same invariant since the fix is reader-agnostic.

- **Reader-shaped property test.** Extend the round-trip proptest to generate
  **reader-shaped** documents — sibling blocks with *no* pre-inserted
  `BlankLineGroup` separators — then assert Skeleton equality after
  serialize→parse. Every generator strategy that currently pre-inserts
  `BlankLineGroup`s between blocks — `session_strategy`, `definition_strategy`,
  and `nested_session_strategy`, each carrying the same comment "Paragraphs merge
  without blank lines — always separate them" — encodes the serializer's weakness
  into the test inputs and masks the bug; that pre-insertion is removed from all
  of them, making reader-shaped ASTs first-class inputs.

- **Fixture / corpus tests.** Drive the real Markdown reader over the existing
  fixtures (`kitchensink.md`, `comrak-readme.md`, `markdown-reference-commonmark.md`)
  and assert the Faithfulness invariant end-to-end. Add targeted small cases:
  H1-led document, title-less document, each block-type adjacency pair
  (paragraph→paragraph, paragraph→list, list→paragraph, heading→body,
  paragraph→verbatim, paragraph→definition), and the backtick-in-code-span
  degrade case.

- **Prior art:** `format_invariants/mod.rs` (`canon`, `check_idempotent`,
  `check_semantic_preserved`); `round_trip_proptest/mod.rs` (`assert_ast_equiv`,
  the strategy generators); `markdown/import.rs` (`md_to_lex`,
  `assert_lex_output_valid` — whose "reparsed doc is non-empty" check is upgraded
  to the full Faithfulness invariant).

- **Regression guard for `lex → lex`:** a check that a representative set of
  Lex-sourced documents serialize byte-identically before and after the change,
  encoding the "zero regression" guarantee.

## Out of Scope

- Any lex-core change *beyond* the title model (ADR-0002): the block-separation
  fix touches only the babel serializer. The only sanctioned lex-core changes here
  are the title-rule simplification and the `doc.untitled` builtin from ADR-0002.
- Session-title hoisting (the `document-05` fixture's aspiration) — tracked
  separately, not part of this work.
- Perfect preservation of blank-line *counts* — collapsing a run of blank lines
  to a single structural separator is Declared Lossy and accepted.
- Round-tripping Lex-only constructs that Markdown cannot represent: annotations
  (Markdown import does not parse them back), citation references (rendered as
  plain text), sessions deeper than heading level 6, and verbatim post-wall
  indentation (lex#276).
- Representing a backtick *inside* inline code: Lex code spans are literal and
  cannot contain their own delimiter. In scope is only that this case degrades
  predictably (no corrupted re-parse), not that it round-trips.
- New foreign formats (Pandoc, LaTeX, HTML import) — see
  [interop-scope.lex](https://github.com/lex-fmt/comms/blob/main/docs/interop-scope.lex).

## Further Notes

- The root cause is singular and systemic: the serializer treated block
  separation as *data* (present only when a `BlankLineGroup` node existed) rather
  than as a *structural property of the grammar*. Every non-Lex reader produces
  ASTs without those nodes, so all of them were latently broken; Markdown is
  simply where it was noticed. The prior point fixes (verbatim lex#505,
  annotation lex#682, and the title-boundary lex#687) were the same bug patched
  one block/boundary at a time.
- The vocabulary for this area (Faithfulness, Skeleton, Block Separation,
  BlankLineGroup, Document Title, No-title marker, Equivalence Set, Declared
  Lossy) is defined in `CONTEXT.md`.
- The architectural choice (structural separation in the serializer vs. readers
  synthesizing `BlankLineGroup`s) and its rejected alternative are recorded in
  ADR-0001. The document title model (first content line is the title;
  `:: doc.untitled ::` opts out) is recorded in ADR-0002.
