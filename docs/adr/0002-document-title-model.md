# Document title model: first content line is the title; explicit `:: doc.untitled ::` opts out

## Status

accepted (supersedes the title-handling portions of ADR-0001)

## Context

Lex's document-title rule has been the murkiest corner of the grammar, and the
Markdown round-trip work forced it into the open. Three things collided:

- The parser assigned a title only when the first line was a lone paragraph
  followed by a blank, and a **leading blank line suppressed** it. That made a
  leading blank *semantically load-bearing*, which in turn made `lexd format`'s
  "strip leading/trailing blank lines" normalization **unsound**: formatting a
  title-less document silently promoted its first line to a title
  (`title=None` → `title="One"`).
- Fixtures encoded intent the parser didn't implement (`document-06-title-empty`
  is named title-less but parses titled; `document-05-title-session-hoist`
  expects hoisting that doesn't happen).
- The whole tracked corpus (`trifecta/`, `benchmark/`) is 100% title-first; no
  document uses a leading blank or is title-less. The idiom is "documents have a
  title", but there was no honest way to express "this one doesn't" — needed
  because formats with formal titles (Markdown) legitimately lack one, and that
  must round-trip.

## Decision

**1. The title is the first content element, when that element is a paragraph.**
Leading blank lines are irrelevant — they no longer suppress the title. A title
is one line, or two lines when the first line ends with a colon (title +
subtitle; the colon is structural and stripped, per the existing
`<subtitle-line>` rule). A first line without a trailing colon that spans
multiple lines is a paragraph, not a title — the colon is the explicit signal
that disambiguates a two-line title from a two-line paragraph.

**2. If the first content element is anything other than a paragraph** — session,
list, definition, verbatim, annotation — there is **no title**; the document
starts with that element. (Already the parser's behavior.)

**3. A document may explicitly declare no title with `:: doc.untitled ::`.** This
is a registered `doc.*` builtin, honored by the **parser** (not just babel): when
present among the leading document-level annotations, title promotion is
suppressed and all content stays in the body. It is how a Reader represents a
titled-format source (e.g. Markdown) that has no title, so the absence
round-trips faithfully.

Consequences of (1): the parser drops the leading-blank suppression special case
(a simplification), and `lexd format`'s blank-stripping becomes meaning-preserving
because leading blanks no longer carry title semantics.

## Considered alternatives

- **Leading blank = "no title" (the "title escape").** Rejected: a whitespace
  trick for a semantic distinction — surprising, invisible, non-idiomatic (no
  corpus doc uses it), and `lexd format` strips it. Replaced by the explicit
  `:: doc.untitled ::` annotation, which survives formatting and is discoverable.
- **Any number of lines can be a title (no colon required).** Rejected: it can't
  be distinguished from a multi-line paragraph. The colon-introduced subtitle
  (option b) is the principled disambiguator and matches how publishers and
  library cataloging separate a main title from its subtitle into distinct
  fields.
- **Empty-valued `:: doc.title: ::` to mean "no title".** Rejected in favor of a
  dedicated `doc.untitled` flag: "empty title" vs "no title" reads ambiguously; a
  boolean opt-out states intent clearly.

## Consequences

- lex-core parser change (title rule) + a new registered `doc.untitled` builtin
  label. Blast radius on the tracked corpus is ~zero (no doc starts with a
  leading blank). `document-06-title-empty` is renamed/repurposed to match the
  settled behavior; `document-05-session-hoist`'s hoisting question is tracked
  separately.
- The Markdown Reader emits `:: doc.untitled ::` for a heading-less source; the
  Markdown Serializer maps a Lex title to `# H1`. A leading `# H1` maps to the
  title. This replaces the "leading-blank title escape" that an earlier plan
  (issue for slice T2) carried.
- Open, non-blocking: how a Lex **subtitle** maps to Markdown (which has no
  subtitle concept) — a conversion-mapping convention or a Declared-Lossy call,
  decided separately.
- ADR-0001 stands for block separation; its "no grammar change" note applied to
  the block-separation fix and is now qualified by this ADR for the title model.
