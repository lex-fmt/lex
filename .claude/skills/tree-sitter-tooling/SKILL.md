---
name: tree-sitter-tooling
description: |
  Guide for working with the Lex tree-sitter grammar, running highlight queries,
  and comparing tree-sitter output against LSP semantic tokens. Use when:
  (1) Modifying or debugging the tree-sitter grammar (grammar.js, scanner.c)
  (2) Editing highlight queries (highlights.scm)
  (3) Comparing tree-sitter captures against LSP semantic tokens
  (4) Running tree-sitter tests or error-checking .lex files
---

# Tree-sitter Tooling for Lex

The tree-sitter grammar lives at `tree-sitter/` in the repo root. It is a separate parser from lex-core — tree-sitter provides fast, synchronous CST parsing for editors, while lex-core provides the authoritative AST via the LSP.

## Setup and Configuration

The tree-sitter CLI requires the grammar directory to be discoverable. The grammar is at `tree-sitter/` (not `tree-sitter-lex/`), so the CLI's `parser-directories` config won't find it by default.

**Use a symlink + temp config for all CLI commands:**

```sh
# One-time setup (persists until reboot)
ln -sfn /Users/adebert/h/lex/core/tree-sitter /tmp/tree-sitter-lex
echo '{"parser-directories":["/tmp"]}' > /tmp/ts-config.json
```

Then all tree-sitter commands use `--config-path /tmp/ts-config.json`. Always run from the `tree-sitter/` directory:

```sh
cd /Users/adebert/h/lex/core/tree-sitter
```

## Key Commands

### Parse a file (CST dump)

```sh
npx tree-sitter parse ../comms/specs/benchmark/010-kitchensink.lex
```

No config needed — `parse` uses the local grammar directly.

### Run highlight queries (captures with positions)

```sh
npx tree-sitter query queries/highlights.scm ../path/to/file.lex \
  --config-path /tmp/ts-config.json --captures
```

This shows every capture with its pattern number, scope name, position, and matched text. Use this to verify which scope wins when multiple patterns match.

### Run highlight (colored output)

```sh
npx tree-sitter highlight --config-path /tmp/ts-config.json ../path/to/file.lex
```

Shows the file with ANSI colors applied by the highlight queries.

### Run grammar tests

```sh
npx tree-sitter test
```

Runs all corpus tests in `test/corpus/*.txt`. No config needed.

### Error-check .lex files

```sh
bash scripts/error-check.sh ../comms/specs/benchmark/010-kitchensink.lex
# or check all fixtures:
bash scripts/error-check.sh
```

Validates no ERROR nodes in the CST.

### Parity check (CST structure vs lex-core AST)

```sh
bash scripts/parity-check.sh ../path/to/file.lex --verbose
```

## Comparing Tree-sitter Highlights vs LSP Semantic Tokens

This is the core verification workflow. Both commands should be run from the repo root (`/Users/adebert/h/lex/core`).

### Step 1: Get LSP semantic tokens

```sh
cargo run -q -p lex-cli -- inspect comms/specs/benchmark/010-kitchensink.lex semantic-tokens
```

Output format: `line:col-line:col  TokenType  "text"`

### Step 2: Get tree-sitter highlight captures

```sh
cd tree-sitter
npx tree-sitter query queries/highlights.scm ../comms/specs/benchmark/010-kitchensink.lex \
  --config-path /tmp/ts-config.json --captures
```

Output format: `pattern: N, capture: N - scope.name, start: (row, col), end: (row, col), text: ...`

Note: tree-sitter uses 0-based lines, LSP output uses 1-based lines.

### Step 3: Compare using the mapping table

The canonical mapping from tree-sitter scopes to LSP token types:

| Tree-sitter scope | LSP token types |
|---|---|
| `markup.heading` | `SessionTitleText`, `SessionMarker` |
| `variable.other.definition` | `DefinitionSubject` |
| `markup.raw.block` | `VerbatimSubject`, `VerbatimLanguage`, `VerbatimAttribute` |
| `markup.raw` | `VerbatimContent` |
| `markup.bold` | `InlineStrong` |
| `markup.italic` | `InlineEmphasis` |
| `markup.raw.inline` | `InlineCode` |
| `markup.math` | `InlineMath` |
| `markup.link` | `Reference`, `ReferenceCitation`, `ReferenceFootnote` |
| `markup.list` | `ListMarker`, `ListItemText` |
| `punctuation.special` | `AnnotationLabel` |
| `comment` | `AnnotationLabel`, `AnnotationParameter`, `AnnotationContent` |
| `string.escape` | (no LSP equivalent) |

This mapping is also defined in the VSCode integration test at:
`/Users/adebert/h/lex/vscode/test/integration/treesitter_parity.test.ts`

## File Locations

| What | Where |
|---|---|
| Grammar definition | `tree-sitter/grammar.js` |
| External scanner | `tree-sitter/src/scanner.c` |
| Highlight queries | `tree-sitter/queries/highlights.scm` |
| Corpus tests | `tree-sitter/test/corpus/*.txt` |
| Error-check script | `tree-sitter/scripts/error-check.sh` |
| Parity-check script | `tree-sitter/scripts/parity-check.sh` |
| Generated parser | `tree-sitter/src/parser.c` (do not edit) |
| Node types | `tree-sitter/src/node-types.json` (do not edit) |
| VSCode parity test | `/Users/adebert/h/lex/vscode/test/integration/treesitter_parity.test.ts` |
| LSP semantic tokens | `crates/lex-analysis/src/semantic_tokens.rs` |
| Kitchen-sink benchmark | `comms/specs/benchmark/010-kitchensink.lex` |

## highlights.scm Precedence Rules

In tree-sitter queries, **LATER patterns override earlier ones** when multiple patterns match the same node. This means:

1. Put generic patterns first (e.g., `(annotation_marker) @punctuation.special`)
2. Put specific overrides after (e.g., `(verbatim_block (annotation_marker)) @markup.raw.block`)

The specific pattern wins because it appears later in the file.

## Known Limitations

These are grammar-level limitations tracked in GitHub issues:

- **#416**: `list_item_line` and `annotation_inline_text` are leaf nodes — inline formatting (`*bold*`, `[ref]`, `` `code` ``) inside them is not parsed
- **#417**: List markers and session markers are not separated from content text — `- Item` is one `markup.list` span, `1. Title` is one `markup.heading` span

## Regenerating the Parser

After editing `grammar.js`:

```sh
npx tree-sitter generate
npx tree-sitter test
```

After editing `src/scanner.c`, just rebuild and test:

```sh
npx tree-sitter test
```
