; Highlight queries for Lex
; See: https://tree-sitter.github.io/tree-sitter/syntax-highlighting
;
; These captures map tree-sitter CST nodes to TextMate scopes. The LSP's
; semantic tokens override these in editors, but the scopes here must be
; structurally correct — a session title is a heading, a list item is a
; list item, a definition subject is a definition term.
;
; Reference: lex-analysis/src/semantic_tokens.rs defines the authoritative
; LSP token types. This file mirrors that mapping at CST granularity.
;
; PRECEDENCE: In tree-sitter queries, LATER patterns override earlier ones
; when multiple patterns match the same node. Specific overrides (e.g.
; verbatim closing markers) must appear AFTER their generic counterparts.

; === Sessions ===
; Session titles are headings (LSP: SessionTitleText)
(session
  title: (line_content) @markup.heading)

; === Definitions ===
; Definition subjects are terms being defined (LSP: DefinitionSubject)
; NOT headings — they are variable/term definitions
(definition
  subject: (subject_content) @variable.other.definition)

; === Verbatim Blocks ===
; Verbatim subject line (LSP: VerbatimSubject)
(verbatim_block
  subject: (subject_content) @markup.raw.block)

; Verbatim block body content is raw/preformatted (LSP: VerbatimContent)
(verbatim_block
  (paragraph) @markup.raw)
(verbatim_block
  (definition) @markup.raw)
(verbatim_block
  (list) @markup.raw)

; === Lists ===
; List item lines — ONLY inside list_item nodes (LSP: ListMarker + ListItemText)
; list_item_line also appears as line_content in session titles, where it
; should NOT be tagged as a list item (it's a heading in that context).
(list_item
  (list_item_line) @markup.list)

; === Annotations (generic) ===
; Annotation delimiters (LSP: part of AnnotationLabel)
(annotation_marker) @punctuation.special
(annotation_end_marker) @punctuation.special

; Annotation header — the label between :: markers (LSP: AnnotationLabel)
(annotation_header) @comment

; Annotation inline text (LSP: AnnotationContent)
(annotation_inline_text) @comment

; Annotation block body content (LSP: AnnotationContent)
(annotation_block
  (_) @comment)

; === Verbatim closing metadata (overrides generic annotation captures) ===
; Annotation nodes inside verbatim_block are the closing `:: label ::` line
; (LSP: VerbatimLanguage/VerbatimAttribute). These MUST appear AFTER generic
; annotation captures so they take priority.
(verbatim_block
  (annotation_marker) @markup.raw.block)
(verbatim_block
  (annotation_header) @markup.raw.block)

; === Inline formatting ===
(strong) @markup.bold
(emphasis) @markup.italic
(code_span) @markup.raw.inline
(math_span) @markup.math
(reference) @markup.link
(escape_sequence) @string.escape
