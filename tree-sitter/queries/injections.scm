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
; definition, list, etc.) since tree-sitter doesn't know it's verbatim content.
; The injection overrides this parsing with the target language's grammar.

; Match content blocks (paragraphs) inside verbatim
((verbatim_block
  (paragraph) @injection.content
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#set! injection.combined))

; Match content blocks (definitions) inside verbatim
((verbatim_block
  (definition) @injection.content
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#set! injection.combined))

; Match content blocks (lists) inside verbatim
((verbatim_block
  (list) @injection.content
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#set! injection.combined))

; Match content blocks (sessions) inside verbatim
((verbatim_block
  (session) @injection.content
  (annotation_header) @injection.language)
 (#gsub! @injection.language "^%s*(%S+).*$" "%1")
 (#set! injection.combined))
