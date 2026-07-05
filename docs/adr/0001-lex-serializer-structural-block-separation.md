# Lex Serializer derives block separation structurally

## Status

accepted

## Context

The Lex Serializer (`crates/lex-babel/src/formats/lex/serializer.rs`) turns a Lex AST
into Lex text. Blank lines between sibling blocks are load-bearing in Lex: two
adjacent text lines with no blank between them parse as **one** two-line
Paragraph, while the same lines with a blank between them parse as **two**
Paragraphs. Separation is a grammar requirement, not decoration.

The serializer emitted that separation **only** when it visited a
`BlankLineGroup` node. lex-core's parser produces those nodes, so `lex → lex`
round-tripped fine. But every other Reader (Markdown — the format
`interop-scope.lex` calls "Lex's lingua franca", where round-trip is the
release bar — and RFC-XML) builds an AST with **no** `BlankLineGroup` nodes. Its
output therefore serialized with blocks jammed together and re-parsed as merged
paragraphs, lost document titles, and invalid lists.

Two symptoms had already been patched per-block inside the serializer
(`visit_verbatim_block` for lex#505, `leave_annotation` for lex#682) — each an
ad-hoc structural-separation rule for one block type. They were the same root
cause surfacing twice.

## Decision

The serializer emits the grammar-mandated block separation **itself**, derived
from the structure (a separation matrix over ordered sibling block-type pairs),
independent of any `BlankLineGroup`. `BlankLineGroup`, when present, requests
*additional* blanks beyond the structural minimum.

This composes non-destructively because `ensure_blank_lines(n)` is
**max-composing** (it pushes newlines up to `n`, never additively): a structural
minimum of 1 and a `BlankLineGroup(k)` yield `max(1, k)`. For lex-sourced ASTs
the `BlankLineGroup` already carries ≥ the minimum, so **`lex → lex` output is
byte-identical to before** — zero regression — while Reader-built ASTs finally
get their separators.

The document-title blank (a title must be followed by a blank or it merges into
the first body block — the same boundary a prior lex#687 patch addressed
point-wise) and the title-escape for title-less documents (a leading
blank suppresses Lex's title-steal rule — `<document-title>` requires the *first*
line) are the same structural rule applied to the title boundary. **No grammar
or lexer change:** the title-steal rule in `grammar-core.lex` is intentional and
untouched; the serializer merely emits spec-valid output.

## Considered alternatives

- **Readers synthesize `BlankLineGroup`s.** Rejected: it is a per-Reader patch
  that duplicates separation logic into every current and future Reader, leaves
  the lex#505/#682 band-aids in place, and re-introduces the defect each time a
  Reader is added. It fixes Markdown, not the shared choke point.

## Consequences

- The lex#505 and lex#682 special cases are subsumed by the general rule and
  deleted.
- The round-trip proptest's `session_strategy`, which pre-inserted
  `BlankLineGroup`s between generated blocks specifically because "paragraphs
  merge without blank lines", no longer needs to — reader-shaped ASTs (no
  separators) become first-class test inputs, which is exactly what regression
  coverage for this bug requires.
- The separation matrix becomes the single, explicit home for "what separates
  block A from block B", replacing knowledge that was implicit in parser
  behavior and scattered patches.
