Tree-sitter Grammar for Lex
===========================

This directory contains the tree-sitter grammar for the Lex document format.
It lives in the main lex-fmt/lex repo because it is tightly coupled to the
grammar specs in comms/specs/ — spec changes that affect parsing must be
validated against the tree-sitter grammar in the same PR.

What this is
------------

Tree-sitter provides CST-level (concrete syntax tree) features that run
synchronously in the editor:

  - Syntax highlighting (highlights.scm)
  - Embedded language injection in verbatim blocks (injections.scm)
  - Structural selection / textobjects for nvim (textobjects.scm)
  - Support for bat, GitHub linguist, difftastic

It does NOT replace lex-core or lex-lsp. The LSP provides semantic features
(diagnostics, go-to-definition, hover, completion, formatting) via lex-core's
full parser. LSP semantic tokens override tree-sitter tokens in both VSCode
and nvim — this is built-in editor behavior.

Architecture: two parsers, different jobs
-----------------------------------------

  Tree-sitter (sync, in editor)       lex-core via lex-lsp (async)
  - Syntax highlighting               - Semantic tokens (overrides TS)
  - Embedded language injection        - Diagnostics
  - nvim textobjects                   - Go-to-definition
  - bat / GitHub / difftastic          - Hover, completion, formatting

No converter between them. Each serves its own purpose.

Files
-----

  grammar.js          Grammar rules (block structure, inline elements)
  src/scanner.c       External scanner (indentation, emphasis flanking)
  src/parser.c        Generated parser (do not edit — run `npx tree-sitter generate`)
  queries/
    highlights.scm    Highlight capture groups
    injections.scm    Embedded language injection (verbatim blocks)
    textobjects.scm   nvim-treesitter structural selection
  scripts/
    error-check.sh    Validates all spec fixtures parse without ERROR nodes
  test/corpus/        Tree-sitter corpus tests

How editor UIs consume this
---------------------------

On tagged releases, CI builds a tree-sitter.tar.gz artifact containing:

  grammar.js, package.json, tree-sitter.json
  src/parser.c, src/scanner.c, src/tree_sitter/parser.h
  queries/*.scm
  tree-sitter-lex.wasm (pre-built, for VSCode/Electron)

Editor UI repos (lex-fmt/vscode, lex-fmt/nvim, lex-fmt/lexed) download this
artifact from the GitHub release, same pattern as lex-lsp binaries. The
version is pinned in each editor's shared/lex-deps.json.

  VSCode / lexed:  Use the .wasm file + query files
  nvim:            Compile src/parser.c + src/scanner.c locally, use query files

Development
-----------

  npm install                     # install tree-sitter CLI (one time)
  npx tree-sitter generate       # regenerate parser.c from grammar.js
  npx tree-sitter test            # run corpus tests (test/corpus/*.txt)
  ./scripts/error-check.sh        # parse all spec fixtures, check for errors

Both `tree-sitter test` and `error-check.sh` run in the pre-commit hook
and CI (scripts/rust-pre-commit, .github/workflows/test-rust.yml).

See also: https://github.com/lex-fmt/lex/issues/407
