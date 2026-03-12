; Text object queries for nvim-treesitter
; See: https://github.com/nvim-treesitter/nvim-treesitter-textobjects
;
; These enable structural selection in neovim:
;   vaf — select around function/block
;   vif — select inside function/block
;   vac — select around class/section
;   vic — select inside class/section
;   etc.

; === Blocks (@block.outer / @block.inner) ===
; Sessions are the primary structural unit
(session) @block.outer
(session
  (_) @block.inner)

; Definitions
(definition) @block.outer
(definition
  (_) @block.inner)

; Verbatim blocks
(verbatim_block) @block.outer

; === Classes/Sections (@class.outer / @class.inner) ===
; Sessions map to "class" for section-level navigation
(session) @class.outer
(session
  (_) @class.inner)

; === Statements (@statement.outer) ===
; Individual content elements as "statements"
(paragraph) @statement.outer
(list) @statement.outer
(definition) @statement.outer
(verbatim_block) @statement.outer
(annotation_block) @statement.outer
(annotation_single) @statement.outer

; === Comments (@comment.outer) ===
; Annotations are metadata — map to "comment" for quick navigation
(annotation_block) @comment.outer
(annotation_single) @comment.outer

; === Parameters (@parameter.outer) ===
; List items as "parameters" for item-level navigation
(list_item) @parameter.outer
