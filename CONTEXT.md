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
The optional first line of a Lex document, stored in `Document.title`
(`Option<DocumentTitle>`; absent = `None`), not a body Block. lex-core's parser assigns it by a purely syntactic rule: the title
is the first block **iff** it is a single-line Paragraph at the very start of
the document immediately followed by a blank line. A leading blank line
suppresses the rule (the first Paragraph then stays in the body). Markdown has
no distinct title concept; the Markdown Reader promotes a leading `# H1` to the
Document Title.

**Title escape**:
A leading blank line the Lex Serializer emits when the document has no title
(`Document.title` is `None` — the field is `Option<DocumentTitle>`) and the
first Block is a steal-able single-line Paragraph. It suppresses the grammar's
title rule (`<document-title>` requires the _first_ non-annotation line), keeping
the Paragraph in the body. Spec-sanctioned — it changes no grammar; it produces
valid Lex the existing parser reads as title-less. Note: `lexd format`'s blank
normalization currently strips it and re-promotes the title (a formatter-layer
concern, tracked separately, not this work).

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
> after it. The only subtlety is a **title-less** doc: Lex's title rule steals a
> lone first line, so the serializer emits a **Title escape** (a leading blank).
> That's spec-valid — it changes no grammar.
>
> **Dev:** Is any of this "lossy"?
>
> **Expert:** Blank _counts_ are — three blanks become one, that's **Declared
> Lossy**. Block _structure_ and the Title are not; losing those is a
> **Faithfulness** bug.
