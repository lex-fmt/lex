# Lex ↔ Format Conversion

The vocabulary of converting Lex documents to and from other document formats
(Markdown, HTML, PDF, …) via lex-babel. This glossary covers the concepts that
govern round-trip fidelity — the terms that are easy to conflate when reasoning
about what survives a conversion.

## Language

**Conversion**:
Transforming a document between two formats. A **Reader** parses a foreign
format into a Lex AST (`Format::parse`); a **Serializer** emits a Lex AST into a
foreign format (`Format::serialize`). "Markdown → Lex" runs the Markdown Reader
then the Lex Serializer.
_Avoid_: import/export (use only for user-facing CLI text).

**Round-trip**:
Converting a document out to another format and back. Fidelity claims are always
about a specific pair and direction (e.g. "Lex → Markdown → Lex").
_Avoid_: reversible, symmetric.

**Equivalence Set**:
The subset of constructs that both Lex and a given foreign format can represent
natively, and which are therefore _required_ to round-trip losslessly. Lex is
strictly more expressive than Markdown, so constructs outside the set (e.g.
annotations, references) are **Declared Lossy** rather than bugs.
_Avoid_: supported, compatible.

**Faithfulness** (the primary invariant):
A Serializer is faithful when the text it emits re-parses to the same AST it was
given. For conversion: the Lex text produced from Markdown must re-parse to the
same **Skeleton** the Markdown Reader built —
`skeleton(lex_parse(lex_serialize(md_read(src)))) == skeleton(md_read(src))`.
Format-agnostic (protects every Reader) and robust to a foreign format's cosmetic
re-normalization. The primary property under test; "equivalent constructs
round-trip losslessly" = Faithfulness + the Equivalence Set list.

**Skeleton**:
An AST reduced to what Faithfulness compares: Document Title + Block structure +
inline content, with all **BlankLineGroup** nodes removed. Two ASTs are
faithful-equal iff their Skeletons match. Blank _counts_ are decoration (Declared
Lossy — "multiple blanks → single"); Block _structure_ and the Title are not.
Re-parsing serialized Lex necessarily re-introduces BlankLineGroups the Reader
never emitted, so equality must be taken over Skeletons, not raw ASTs. (This is
the same reduction `ir::from_lex` already performs when it strips BlankLineGroups.)

**Declared Lossy**:
A conversion that is knowingly, documentedly non-round-trippable because one
format cannot represent a construct of the other. Distinct from a fidelity
_bug_, where both formats _can_ represent the construct but the pipeline
mangles it.

**Block**:
A structural sibling element in a Lex document body — Paragraph, List, Session,
Verbatim, Definition, Annotation, Table. Blocks are the units that must be
_separated_ from one another in surface syntax.

**Block Separation**:
The blank line(s) that must sit between two sibling Blocks in Lex surface
syntax. In Lex a blank line is _semantically load-bearing_: two adjacent text
lines with no blank between them are ONE two-line Paragraph, while the same two
lines with a blank between them are TWO Paragraphs. Separation is therefore a
structural requirement of the grammar, not decoration.

**BlankLineGroup**:
An AST node recording blank lines that were present in _parsed Lex source_.
lex-core's parser emits them; foreign-format Readers do not. Today the Lex
Serializer emits Block Separation _only_ when it visits a BlankLineGroup — so
ASTs produced by non-Lex Readers serialize with no separation and re-parse
wrong. (This is the systemic defect under investigation.)

**Document Title**:
The optional title of a Lex document, stored in `Document.title`
(`Option<DocumentTitle>`; absent = `None`), not a body Block. Settled model
(ADR-0002): the title is the **first content element when it is a paragraph** —
one line, or two lines when the first line ends with a colon (title + subtitle;
the colon is structural and stripped). Leading blank lines are irrelevant. If the
first content element is anything else (Session, List, Definition, Verbatim,
Table, Annotation), there is **no title** and the document starts with that
element. A
multi-line first paragraph _without_ a leading-line colon is a paragraph, not a
title — the colon is the explicit signal that distinguishes a two-line title from
a two-line paragraph. Markdown has no distinct title concept; the Markdown Reader
maps a leading `# H1` to the Document Title.

**No-title marker** (`:: doc.untitled ::`):
A registered `doc.*` builtin annotation that explicitly declares a document has
no title. Honored by the **parser** (not just babel): when present among the
leading document-level annotations, it suppresses title promotion so the first
paragraph stays in the body. It is how a Reader represents a titled-format source
(e.g. Markdown with no leading heading) whose missing title must round-trip
faithfully. Replaces the rejected "leading-blank title escape" — it survives
`lexd format` (it is real content, not strippable whitespace) and is
discoverable. See ADR-0002.

## Faithfulness status — what round-trips today

The Markdown↔Lex faithfulness epic (#781–#785, #798, #795) is the current state
of the world. This table is the durable "what survives the trip" summary; the
live, per-fixture truth is the anti-rot known-fail lists in
`crates/lex-babel/tests/markdown/faithfulness_fixtures.rs` and
`crates/lex-babel/tests/format_invariants/mod.rs` — keep this in sync with them.

**Round-trips faithfully today** (Markdown → Lex → re-parse, Skeleton-equal):

- Paragraphs, and Block Separation between **every** ordered sibling pair (the
  separation matrix, ADR-0001 / #782).
- Document title: a leading `# H1` ↔ `Document.title`, and a heading-less source
  via the `:: doc.untitled ::` no-title marker (ADR-0002 / #783).
- Lists — unordered, ordered (markers preserved), nested, and multi-block items
  (reader-built loose lists hoist to tight form, #798).
- Definitions.
- Verbatim / fenced code blocks (as blocks).
- Inline emphasis and inline code.
- Marker-like session titles (`## 1. X` → `1\. X` guard, re-parses `style: None`,
  #795).

**Deferred — tracked bugs that do not round-trip yet** (listed in the test
known-fail sweeps; fixing the bug flips the fixture to faithful and forces its
removal from the list):

- Nested verbatim/definition **body de-indent**: a colon-terminated paragraph
  before a fenced block is absorbed as a verbatim subject / becomes a Definition,
  and multi-line or nested table-cell bodies de-indent — **#790** (blocks
  kitchensink, comrak-readme, both CommonMark references).
- Leading document-level **annotation reorder** around the title / subtitle —
  **#791** (blocks `20-ideas-naked`; also `document-09`, `annotation-27`).
- Ragged / mismatched-row **table normalization** — padding + a separator row are
  injected, adding cells — **#792** (`table-13`).

**Declared Lossy** — knowingly non-round-trippable by construction, degrades
predictably (never a bug to "fix"):

- A **backtick inside a code span** (Markdown's ``` `` a`b `` ```): a Lex code
  span is single-backtick and literal, so it cannot hold its own delimiter. The
  markup is dropped; the text degrades to well-formed prose (no corrupt re-parse).
- **Blank-line counts**: a run of blanks collapses to the single structural
  separator (Skeleton ignores blank _count_).
- **Single-item lists**: Lex requires ≥ 2 items, so a one-item Markdown list
  degrades to a Paragraph (content preserved, not a faithful list).
- **Lex-only constructs** outside the Equivalence Set: annotations, citation /
  reference syntax, and sessions deeper than Markdown's six heading levels.

## Flagged ambiguities

**"Lossless"** — always scope it: lossless _for the Equivalence Set_. A bare
"round-trips losslessly" is false for Lex ↔ Markdown because Lex is more
expressive.

**"Round-trip"** — always name the pair and direction. "Lex → Markdown → Lex"
and "Markdown → Lex → Markdown" are different claims with different failure
modes. Faithfulness (`parse∘serialize`) is a serializer property; a full
foreign round-trip additionally depends on the Reader.

## Example dialogue

> **Dev:** The Markdown converter drops blank lines — two paragraphs come out as one.
>
> **Expert:** It doesn't drop _blank lines_, it drops **Block Separation**. In Lex
> a blank line isn't whitespace, it's the boundary between two Blocks. Without it
> those two Paragraphs are one two-line Paragraph.
>
> **Dev:** So the Markdown Reader should insert the blanks?
>
> **Expert:** That's the shallow fix. Separation is a property of the _grammar_,
> so the **Lex Serializer** should emit it structurally — then RFC-XML and every
> future Reader are fixed too. `BlankLineGroup` stays, but only to carry blanks
> _beyond_ the structural minimum.
>
> **Dev:** And the lost title on `# H1` docs?
>
> **Expert:** Same rule at the title boundary — a **Document Title** needs a blank
> after it. And the model is settled now (ADR-0002): the first content line _is_
> the title if it's a paragraph. A Markdown doc with no heading genuinely has no
> title, so the Reader writes an explicit **No-title marker** (`:: doc.untitled ::`)
> that the parser honors — no whitespace tricks.
>
> **Dev:** Is any of this "lossy"?
>
> **Expert:** Blank _counts_ are — three blanks become one, that's **Declared
> Lossy**. Block _structure_ and the Title are not; losing those is a
> **Faithfulness** bug.
