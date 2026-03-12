; Highlight queries for Lex
; See: https://tree-sitter.github.io/tree-sitter/syntax-highlighting

; === Sessions ===
; Session titles are headings
(session
  title: (line_content) @markup.heading)

; === Definitions ===
; Definition subjects
(definition
  subject: (subject_content) @markup.heading)

; === Verbatim Blocks ===
; Verbatim subject line
(verbatim_block
  subject: (subject_content) @markup.heading)

; Verbatim block content (paragraphs inside verbatim are raw)
(verbatim_block
  (paragraph) @markup.raw)

; === Lists ===
(list_item_line) @markup.list

; === Annotations ===
(annotation_marker) @punctuation.special
(annotation_end_marker) @punctuation.special
(annotation_header) @attribute
(annotation_inline_text) @string

; === Inline formatting ===
(strong) @markup.bold
(emphasis) @markup.italic
(code_span) @markup.raw.inline
(math_span) @markup.math
(reference) @markup.link
(escape_sequence) @string.escape
