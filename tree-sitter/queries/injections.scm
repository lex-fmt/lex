; Injection queries for Lex
;
; Verbatim blocks with a closing annotation (:: python ::, :: json ::, etc.)
; inject the named language into the block's content. This enables syntax
; highlighting for embedded code in editors that support tree-sitter injections
; (nvim-treesitter, VSCode, Helix, etc.).
;
; The annotation_header text may contain parameters (e.g., "json format=pretty"),
; so we extract only the first word as the language name.
;
; Content inside verbatim blocks may be parsed as any block type (paragraph,
; definition, list, table_row, etc.) since tree-sitter doesn't know it's
; verbatim content until the closing annotation. The injection overrides
; this parsing with the target language's grammar.
;
; NOTE: Table blocks (annotation_header matching "table") are excluded from
; injection because their content is inline-parsed lex, not a foreign language.

; Match content blocks (paragraphs) inside verbatim
((verbatim_block
  (paragraph) @injection.content
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; Match content blocks (definitions) inside verbatim
((verbatim_block
  (definition) @injection.content
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; Match content blocks (lists) inside verbatim
((verbatim_block
  (list) @injection.content
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; Match content blocks (sessions) inside verbatim
((verbatim_block
  (session) @injection.content
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; Match table rows inside non-table verbatim blocks (e.g., | in Python code)
((verbatim_block
  (table_row) @injection.content
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; Match table separator rows inside non-table verbatim blocks
((verbatim_block
  (table_separator_row) @injection.content
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; === Verbatim group items ===
; Content inside group items also gets language injection from the
; shared closing annotation.

; Group item paragraphs
((verbatim_block
  (verbatim_group_item
    (paragraph) @injection.content)
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; Group item definitions
((verbatim_block
  (verbatim_group_item
    (definition) @injection.content)
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; Group item lists
((verbatim_block
  (verbatim_group_item
    (list) @injection.content)
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; Group item sessions
((verbatim_block
  (verbatim_group_item
    (session) @injection.content)
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; Group item table rows
((verbatim_block
  (verbatim_group_item
    (table_row) @injection.content)
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))

; Group item table separator rows
((verbatim_block
  (verbatim_group_item
    (table_separator_row) @injection.content)
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#not-match? @injection.language "^\\s*table")
 (#set! injection.combined))
