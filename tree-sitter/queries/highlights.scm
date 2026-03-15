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

; Session sequence marker (LSP: SessionMarker) — numbered titles like "1. Title"
; list_marker inside a session title is structural, not a list item
(session
  title: (line_content
    (list_marker) @punctuation.definition.heading))

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
(verbatim_block
  (verbatim_content) @markup.raw)
(verbatim_block
  (session) @markup.raw)

; Verbatim group item subjects and content
(verbatim_group_item
  subject: (subject_content) @markup.raw.block)
(verbatim_group_item
  (paragraph) @markup.raw)
(verbatim_group_item
  (definition) @markup.raw)
(verbatim_group_item
  (list) @markup.raw)
(verbatim_group_item
  (verbatim_content) @markup.raw)
(verbatim_group_item
  (session) @markup.raw)

; === Lists ===
; List marker (- , 1. , a) , etc.) — captures just the marker portion
; (LSP: ListMarker). Content is handled by inline captures below.
(list_item
  (list_marker) @markup.list)

; === Annotations (generic) ===
; Annotation delimiters (LSP: part of AnnotationLabel)
(annotation_marker) @punctuation.special

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

; === Table blocks (override verbatim captures) ===
; Table blocks are verbatim_blocks whose annotation_header starts with "table".
; Subject is a caption (heading), body is inline-parsed (not raw).
; These MUST appear AFTER generic verbatim captures to take priority.

; Table subject — highlighted as a heading/caption, not raw block
((verbatim_block
  subject: (subject_content) @markup.heading
  (annotation_header) @_lang)
 (#match? @_lang "^\\s*table"))

; Table closing annotation — highlighted as keyword, not raw block
((verbatim_block
  (annotation_header) @keyword)
 (#match? @keyword "^\\s*table"))

; === Inline formatting ===
(strong) @markup.bold
(emphasis) @markup.italic
(code_span) @markup.raw.inline
(math_span) @markup.math
(escape_sequence) @string.escape

; === References (typed) ===
; Generic fallback — all references are links
(reference) @markup.link

; Specific reference type overrides (later = higher priority)
(citation_reference) @markup.link
(footnote_reference) @markup.link
(url_reference) @markup.link.url
(file_reference) @markup.link.url
(session_reference) @markup.link
(tocome_reference) @constant.builtin
(number_reference) @markup.link
