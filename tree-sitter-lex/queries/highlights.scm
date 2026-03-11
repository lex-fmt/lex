; Highlight queries for Lex
; See: https://tree-sitter.github.io/tree-sitter/syntax-highlighting

; Block-level
(subject_content) @markup.heading
(annotation_header) @attribute
(annotation_inline_text) @string
(annotation_marker) @punctuation.special
(annotation_end_marker) @punctuation.special
(list_item_line) @markup.list
(verbatim_block) @markup.raw

; Inline formatting
(strong) @markup.bold
(emphasis) @markup.italic
(code_span) @markup.raw.inline
(math_span) @markup.math
(reference) @markup.link
(escape_sequence) @string.escape
