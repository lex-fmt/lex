# Tree-Sitter Parser for Lex — Implementation Plan

## Feasibility

**Verdict: Feasible, requires an external scanner.**

Tree-sitter's `grammar.js` alone cannot handle Lex's indentation-based structure, verbatim block boundaries, or the 2-item-minimum list rule. An external scanner (C code) manages indentation state and emits synthetic tokens — the same proven approach used by `tree-sitter-python` and `tree-sitter-yaml`.

### Why it's hard

Lex's current parser has a 5-stage pipeline:

1. Tokenize (Logos) → raw tokens
2. Semantic indentation transform → INDENT/DEDENT tokens
3. Line grouping + classification → 8 line types
4. Regex matching on line-type sequences → IR nodes
5. AST building + inline parsing

Stages 2-3 are what make Lex unusual. The grammar doesn't operate on characters or tokens — it operates on **classified line types**. The engine literally prints line type names into a string and runs regex against it (see `engine.rs`). Tree-sitter has no equivalent; we must encode this logic differently.

### Why it's feasible

The grammar itself is **regular at each indent level**. Once indent/dedent boundaries are established, the patterns within each level are straightforward ordered choices. The external scanner handles the irregular parts (indentation, verbatim boundaries), and `grammar.js` handles the regular structure.

---

## Architecture

```
                    grammar.js                    scanner.c
                    ──────────                    ─────────
                    Block rules:                  Indent stack management
                      session                     INDENT / DEDENT emission
                      definition                  Blank line tracking
                      list (2+ items)             Verbatim content scanning
                      paragraph (fallback)        Verbatim closing detection
                      annotation (3 forms)        NEWLINE emission
                      verbatim_block
                    Inline rules:
                      bold, italic, code
                      math, references
                      escape sequences
```

### External Scanner Responsibilities

| What | How |
|------|-----|
| Indentation tracking | Stack of indent levels; 4 spaces or 1 tab = 1 level |
| INDENT token | Emitted when indent level increases |
| DEDENT token(s) | Emitted when indent level decreases (possibly multiple) |
| NEWLINE token | Emitted at line boundaries (gives grammar line-awareness) |
| Blank line detection | NEWLINE + only whitespace + NEWLINE |
| Verbatim mode | Flag + expected closing indent; consume raw content |
| Verbatim close | Detect `::` at correct indent level, exit verbatim mode |
| State serialization | Indent stack + verbatim flag serialized for incremental reparsing |

### grammar.js Node Hierarchy

```
document
├── metadata?                     (:: annotations before content)
├── document_title?               (title + blank, no indented content)
└── _block*
    ├── session                   (title + blank+ + INDENT + _block* + DEDENT)
    ├── definition                (subject_colon + INDENT + _block* + DEDENT)
    ├── list                      (list_item list_item+ trailing_blank?)
    │   └── list_item             (marker + text + (INDENT + _block* + DEDENT)?)
    ├── paragraph                 (_text_line+)
    ├── verbatim_block            (subject_colon + blank? + VERBATIM_CONTENT + closing_annotation)
    ├── annotation                (:: label params? :: content?)
    │   ├── annotation_marker     (:: label ::)
    │   ├── annotation_single     (:: label :: inline_text)
    │   └── annotation_block      (:: label :: INDENT _block* DEDENT ::?)
    └── blank_line_group          (blank_line+)
```

---

## Key Design Decisions

### D1: Session vs. Definition Disambiguation

Both start with a content line followed by indented content. The **only** difference is a blank line.

```
Definition:           ← subject line (ends with :)
    content           ← INDENT immediately follows

Session Title         ← any line type
                      ← blank line(s) required
    content           ← INDENT after blank
```

**Strategy:** Grammar ordering in `choice()`. Definition requires `INDENT` immediately after a colon-terminated line. Session requires `blank_line+ INDENT` after any line. Tree-sitter tries definition first; if no immediate INDENT, falls through to session.

Note: sessions don't require a colon — `Session Title` (no colon) is valid. Definitions always require a colon. This asymmetry helps: a non-colon title line can only be a session, never a definition.

### D2: 2-Item Minimum for Lists

A single `- text` is a paragraph, not a list.

**Strategy:** `list = seq(list_item, repeat1(list_item))` requires 2+. A lone `- text` fails the list rule and falls through to paragraph. Tree-sitter's ordered alternatives handle this naturally.

### D3: Context-Dependent List Blank Line Requirement

At document root, lists need a preceding blank line. Inside containers (sessions, definitions), they don't.

**Strategy:** Use a single permissive list rule (no blank required). The tree-sitter CST is intentionally more permissive than the AST — this matches Lex's philosophy that content should degrade gracefully. Consumers needing strict validation can check the blank-line-before-root-list rule themselves. This keeps the grammar simpler and avoids complex scanner state.

### D4: Error Recovery = Paragraph Fallback

Lex has no parse errors — unrecognizable content is a paragraph. This maps perfectly to tree-sitter's approach:

- In `_block`, `paragraph` is the **last** alternative in `choice()`
- A `_text_line` matches any non-blank, non-annotation line
- If nothing else matches, consecutive text lines become a paragraph

No `ERROR` nodes should appear in well-formed Lex. For malformed input, tree-sitter's built-in error recovery kicks in, but the paragraph fallback should catch most cases before that happens.

### D5: Annotation Attachment

In the current parser, annotations attach to their preceding element as metadata. In the tree-sitter CST, annotations will be **sibling nodes** — a preceding element followed by annotation nodes.

Consumers that need attachment semantics (editors, tooling) can implement a simple post-processing rule: annotations immediately following a block element attach to it. This is a trivial tree walk and keeps the grammar clean.

### D6: Inline Parsing — In-Grammar

Inline formatting (`*bold*`, `_italic_`, `` `code` ``, `#math#`, `[ref]`) will be parsed **within the grammar**, not deferred to a post-processing pass. This gives tree-sitter full incremental parsing of inline edits.

Inline nesting rules use `prec()`:
- Bold can contain italic, code, math, references
- Italic can contain bold, code, math, references
- Code, math, references are leaf nodes (no nesting)
- Same-type nesting blocked (no bold-inside-bold)

---

## Parity Testing Strategy

This is the critical infrastructure. Without it, we'd slowly drift into an uncanny valley of almost-correct parsing.

### Layer 1: Expand `ast-json` in lex-cli (prerequisite)

The current `ast_to_json()` is a stub (only outputs `children_count`). Expand it to recursively serialize the full AST:

```json
{
  "type": "Session",
  "title": "Introduction",
  "marker": null,
  "children": [
    {
      "type": "Paragraph",
      "lines": [
        {"type": "TextLine", "content": "First paragraph."}
      ]
    },
    {
      "type": "List",
      "items": [
        {"type": "ListItem", "marker": "-", "text": "Item one", "children": []},
        {"type": "ListItem", "marker": "-", "text": "Item two", "children": []}
      ]
    }
  ]
}
```

Location data omitted by default (add with `--ast-full`). This is the **canonical format** both parsers must produce.

### Layer 2: Generate Reference Snapshots

```sh
for f in comms/specs/elements/**/*.lex comms/specs/trifecta/*.lex comms/specs/benchmark/*.lex; do
  lex inspect "$f" ast-json > "${f%.lex}.ast-reference.json"
done
```

~50+ reference files, checked into the repo. These are the ground truth.

### Layer 3: Tree-Sitter-to-Canonical-JSON Bridge

A small Rust binary (`ts-lex-bridge`) that:

1. Parses `.lex` with the tree-sitter parser
2. Walks the CST
3. Applies the CST→AST lowering (annotation attachment, container type mapping)
4. Outputs JSON in the same canonical schema

This bridge is the **single point of translation** between tree-sitter's CST and Lex's AST. All parity bugs are fixed here or in the grammar.

### Layer 4: Automated Diff Harness

```sh
#!/bin/bash
# parity-check.sh — run against all fixtures
PASS=0; FAIL=0
for f in comms/specs/**/*.lex; do
  ref="${f%.lex}.ast-reference.json"
  [ -f "$ref" ] || continue
  actual=$(ts-lex-bridge "$f")
  if diff <(jq -S . "$ref") <(echo "$actual" | jq -S .) > /dev/null 2>&1; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
    echo "MISMATCH: $f"
    diff <(jq -S . "$ref") <(echo "$actual" | jq -S .) | head -20
  fi
done
echo "$PASS passed, $FAIL failed"
```

Integrate into CI alongside the existing test suite.

### Layer 5: Progressive Comparison (Phase-Gated)

Don't require full parity from day one. Compare progressively:

| Phase | Compare | Ignore |
|-------|---------|--------|
| M1-M2 | Node types + nesting depth | Inline content, locations, annotations |
| M3-M4 | + List structure, annotation forms | Inline content, locations |
| M5 | + Verbatim raw content | Inline content, locations |
| M6 | + Inline formatting | Locations |
| M7 | Everything | Nothing |

The diff harness takes a `--level` flag controlling comparison depth.

---

## Implementation Phases

### Phase 0: Scaffold

- `tree-sitter init` in a new directory (or subdirectory of this repo)
- Set up `grammar.js` skeleton, `src/scanner.c` with empty serialize/deserialize
- CI: `tree-sitter generate && tree-sitter test`
- Begin expanding `ast-json` in lex-cli (Layer 1 of parity testing)

### Phase 1: Indentation Engine + Paragraphs

**scanner.c:**
- Indent stack (array of levels, starting with 0)
- On NEWLINE: count spaces, compare to stack top
- Emit INDENT / DEDENT tokens accordingly
- Blank lines: emit NEWLINE without changing indent state

**grammar.js:**
- `document = repeat(_block)`
- `_block = choice(paragraph, blank_line_group)`
- `paragraph = repeat1(_text_line)`
- `_text_line = /[^\n]+/` (anything non-empty)

**Validation:** `comms/specs/trifecta/000-paragraphs.lex` parses correctly.

### Phase 2: Sessions + Definitions

**grammar.js additions:**
- `session = seq(_title_line, repeat1(blank_line), $.INDENT, repeat(_block), $.DEDENT)`
- `definition = seq(subject_line, $.INDENT, repeat(_block), $.DEDENT)`
- `subject_line = seq(/[^\n]+/, token.immediate(':'))`
- `_block` choice expanded: `choice(session, definition, paragraph, blank_line_group)`

**Key:** definition must have higher precedence than session in the choice, and its subject line requires the trailing colon. Session title accepts any line type.

**Validation:** `comms/specs/trifecta/010` through `060` fixtures. All `comms/specs/elements/session.docs/` and `comms/specs/elements/definition.docs/`.

### Phase 3: Lists

**grammar.js additions:**
- `list = seq(list_item, repeat1(list_item), optional(blank_line))`
- `list_item = seq(list_marker, _text_content, optional(seq($.INDENT, repeat(_block), $.DEDENT)))`
- `list_marker = choice(plain_marker, ordered_marker)`
- `plain_marker = '- '`
- `ordered_marker = seq(choice(/\d+/, /[a-z]/, /[A-Z]+/), choice('.', ')'), ' ')`

**Disambiguation:** `subject_or_list_item` lines (have both marker and trailing colon) — let them match as list items when inside a list context (2+ items), fall through to definition/session otherwise.

**Validation:** All `comms/specs/elements/list.docs/` fixtures. `comms/specs/trifecta/070-trifecta-flat-simple.lex`.

### Phase 4: Annotations

**grammar.js additions:**
- `annotation_marker = seq('::', label, optional(params), '::')`
- `annotation_single = seq('::', label, optional(params), '::', _text_content)`
- `annotation_block = seq('::', label, optional(params), '::', $.INDENT, repeat(_block), $.DEDENT, optional('::'))`
- `annotation = choice(annotation_block, annotation_single, annotation_marker)`
- `params = repeat1(param)` where `param = seq(identifier, '=', value)`

**Validation:** All `comms/specs/elements/annotation.docs/` fixtures.

### Phase 5: Verbatim Blocks — COMPLETE

**Actual implementation** (differs from original plan — no scanner verbatim mode needed):

A verbatim_block is structurally a definition with a closing `:: label params ::` annotation. GLR explores both definition and verbatim paths; the closing annotation disambiguates. Dynamic precedence 4 (higher than definition=2, session=1) ensures verbatim wins when the closing annotation is present.

**grammar.js:**
- `verbatim_block = prec.dynamic(4, seq(field("subject", line_content), _newline, repeat(blank_line), optional(seq(_indent, repeat1(_block), _dedent)), annotation_marker, annotation_header, annotation_marker, _newline))`
- Content inside is parsed as regular Lex blocks (not raw) — acceptable for highlighting
- Five new GLR conflict declarations for verbatim vs definition/session/text_line

**Status:** 27/27 tests pass. `verbatim.lex` fixture parses error-free. 9/14 element fixtures clean.

**Known limitation — verbatim groups NOT implemented:**
Multiple subject/content pairs sharing one closing annotation (e.g., grouped shell transcripts) require scanner-level group detection. Three grammar-only approaches were attempted:
1. `subject_content` restriction + group repeat → GLR explosion on real files (list.lex: 0→13 errors)
2. Separate `verbatim_content` rule excluding nested verbatim → tree-sitter can't distinguish inlined hidden rules sharing subrules
3. `prec.dynamic(5)` per entry → nested interpretation accumulates higher total precedence

Groups need a scanner-assisted approach: the scanner would look ahead for the closing `:: label ::` and emit a "group continuation" token, eliminating the grammar-level ambiguity. Deferred to a future phase.

### Phase 6: Inline Formatting

**grammar.js additions:**
- `bold = seq('*', repeat1(_inline_content), '*')`
- `italic = seq('_', repeat1(_inline_content), '_')`
- `code = seq('`', /[^`]+/, '`')`
- `math = seq('#', /[^#]+/, '#')`
- `reference = seq('[', _reference_content, ']')`
- `escape_sequence = seq('\\', /[^a-zA-Z0-9]/)`
- `_inline_content = choice(bold, italic, code, math, reference, escape_sequence, _text)`

**Reference sub-classification** (by content pattern):
- `footnote_ref = /\d+/`
- `labeled_footnote_ref = seq('^', identifier)`
- `citation_ref = seq('@', identifier, optional(locator))`
- `session_ref = seq('#', /[\d.]+/)`
- `url_ref = /(https?|mailto):.+/`
- `file_ref = /[./].+/`
- `general_ref = /[a-zA-Z].*/`

**Validation:** Inline-specific fixtures + full document fixtures with inline content.

### Phase 7: Full Parity + Polish

- Run full parity check against all ~50+ fixtures
- Fix edge cases found by diff harness
- Document title handling
- Dialog line detection (lines ending with `..`)
- Extended sequence markers (`1.2.3`)
- Multiple verbatim groups sharing one closing annotation
- Highlight queries (`highlights.scm`) for syntax highlighting
- Injection queries for verbatim blocks (language-specific highlighting)

### Phase 8: Editor Integration

- Publish `tree-sitter-lex` grammar
- Write `highlights.scm`, `indents.scm`, `folds.scm`
- Neovim integration via `nvim-treesitter`
- VSCode integration via tree-sitter WASM
- Evaluate whether tree-sitter can **replace** the current parser for LSP features, or serve as a complement (incremental re-parse → feed to existing analysis)

---

## Risk Register

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Verbatim block scanning too complex for external scanner | High | Medium | Prototype early (Phase 5). Fallback: mark verbatim as opaque node, use current parser for content |
| Session/definition ambiguity causes parse conflicts | Medium | Low | Tree-sitter's GLR + precedence handles this. Proven pattern from other indentation-sensitive grammars |
| Inline parsing too complex in-grammar | Medium | Medium | Fallback: parse inlines as flat text in grammar, run inline pass in consumer. Loses incremental benefit but unblocks |
| Incremental reparsing breaks on large indent changes | Medium | Low | Tree-sitter handles this via scanner state serialization. Test with large files + targeted edits |
| Parity testing reveals fundamental grammar mismatch | High | Low | The grammar is regular at each level — tree-sitter can express this. If a specific rule can't be expressed, move it to the bridge layer |
| 2-item list minimum causes excessive backtracking | Low | Medium | Monitor parse times. If slow, accept 1-item lists in grammar and filter in bridge |

---

## Success Criteria

1. **All fixture files** in `comms/specs/` produce structurally identical ASTs via both parsers (verified by automated diff harness)
2. **Incremental parsing** works: editing a line mid-document re-parses in <10ms
3. **No ERROR nodes** on valid Lex input — everything parses as some valid node (paragraph fallback)
4. **Highlight queries** produce correct syntax highlighting in Neovim and VSCode
5. **The parity test suite runs in CI** and blocks merges on regression
