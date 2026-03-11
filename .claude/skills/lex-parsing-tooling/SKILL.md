---
name: lex-parsing-tooling
description: |
  Guide for inspecting Lex parse trees and debugging the parser using the CLI. Use when:
  (1) Debugging parser output or verifying correctness
  (2) Comparing actual parse trees against expected structure
  (3) Understanding how a .lex file is tokenized, classified, and parsed
  (4) Investigating parser bugs or regressions
---

# Lex Parsing Tooling

The `lex` CLI (in `crates/lex-cli/`) is the primary tool for inspecting parser output at every stage. Build it with:

```sh
cargo build -p lex-cli
```

## Inspect Commands

### AST Visualization (default)

```sh
lex inspect file.lex
# or explicitly:
lex inspect file.lex ast-treeviz
```

Output uses Unicode symbols for element types:
- `⧉` Document
- `§` Session
- `≔` Definition
- `☰` List (shows item count)
- `¶` Paragraph (shows line count)
- `⎯` Blank line group
- `⌸` Verbatim block
- `⊜` Annotation

Line numbers appear on the left. Indentation shows nesting.

### Full AST (with all properties)

```sh
lex inspect file.lex --extra-ast-full
```

Shows session titles, definition subjects, annotation labels/parameters, list markers.

### AST as XML Tags

```sh
lex inspect file.lex ast-tag
```

### AST as JSON

```sh
lex inspect file.lex ast-json
```

### Line-level AST

```sh
lex inspect file.lex ast-linetreeviz
```

## Token Inspection

```sh
lex inspect file.lex token-line-simple   # classified lines
lex inspect file.lex token-core-simple   # raw tokens
lex inspect file.lex token-line-json     # classified as JSON
lex inspect file.lex token-core-json     # core as JSON
```

## Intermediate Representation

```sh
lex inspect file.lex ir-json
```

## Debugging Workflow

### 1. Verify tokenization
```sh
lex inspect problem.lex token-core-simple   # raw tokens
lex inspect problem.lex token-line-simple   # classified lines
```

Check that:
- Indentation produces correct Indent/Dedent tokens
- Lines are classified correctly (SubjectLine vs ParagraphLine, ListLine vs SubjectOrListItemLine)
- Blank lines are detected

### 2. Check parse tree structure
```sh
lex inspect problem.lex ast-treeviz         # overview
lex inspect problem.lex --extra-ast-full    # with labels/subjects
lex inspect problem.lex ast-tag             # precise structure
```

### 3. Compare expected vs actual
```sh
lex inspect file.lex ast-json > actual.json
diff expected.json actual.json
```

### 4. Test fixtures
The spec fixtures in `comms/specs/` are the source of truth:
- `comms/specs/elements/*.lex` — isolated tests per element type
- `comms/specs/elements/*.docs` — documentation for what each fixture tests
- `comms/specs/trifecta/` — edge case combinations
- `comms/specs/benchmark/*.lex` — real-world documents

## Parser Pipeline Reference

```
Source text
  → Lexing (tokenization + semantic indentation)
    → Tree Building (hierarchical LineContainer)
      → Parsing (pattern matching → IR nodes)
        → Building (AST construction + locations)
          → Assembly (annotations + reference resolution)
```

Key source files:
- `crates/lex-core/src/lex/lexing.rs` — Tokenizer and line classification
- `crates/lex-core/src/lex/parsing.rs` — Pattern-based parser
- `crates/lex-core/src/lex/ast.rs` — AST node definitions
- `crates/lex-core/src/lex/building.rs` — AST builder
- `crates/lex-core/src/lex/testing.rs` — Test helpers and fixture loading
