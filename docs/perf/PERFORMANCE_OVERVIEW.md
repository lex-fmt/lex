# Lex performance — complete picture

Three measurements taken together answer "where does Lex sit in the perf landscape?"

1. **Extension-system tax** (PR #552, merged) — does routing `lex.include` through the wire codec slow things down? **No, < 0.6%.**
2. **Parse vs comrak** (PR #553, open) — how does lex-core's parser compare to a battle-tested Markdown parser? **~450× slower.**
3. **End-to-end user operations** (this report, hyperfine) — does that translate into slow `lexd format` / `lexd convert` for users? **No — comparable to or faster than pandoc, well within interactive budgets.**

---

## End-to-end: lexd vs pandoc

Hyperfine, `--warmup 3 --runs 20 --shell=none`, on a freshly-built `target/release/lexd` (v0.11.0) and the system `pandoc`.

**Startup baselines** (process boot + arg parsing, no work):

| Tool | Mean | Notes |
|---|---:|---|
| `lexd --version` | **1.7 ms** | small Rust binary |
| `pandoc --version` | **24.8 ms** | Haskell runtime |

**Per-fixture** (mean ± σ, 20 runs each):

| Fixture | Size (KB) | `lexd format` | `lexd → md` | `lexd → html` | `pandoc md → html` | lexd vs pandoc |
|---|---:|---:|---:|---:|---:|---:|
| 010-kitchensink | 2.4 | 10.5 ms ± 0.2 | 10.7 ± 0.3 | 10.7 ± 0.2 | 200.0 ± 3.2 | **19× faster** |
| 20-ideas-naked | 9.8 | 23.2 ± 0.3 | 23.3 ± 0.2 | 23.5 ± 0.3 | 223.2 ± 4.2 | **9.6× faster** |
| 040-on-parsing | 12.3 | 27.6 ± 0.2 | 27.8 ± 0.2 | 27.9 ± 0.4 | 225.1 ± 4.0 | **8.2× faster** |
| 080-gentle-introduction | 24.1 | 59.9 ± 0.3 | 60.1 ± 0.4 | 60.3 ± 0.3 | 242.3 ± 5.5 | **4.0× faster** |

### Three things to notice

1. **`format` ≈ `→ md` ≈ `→ html` within 1–2%.** Parse cost dominates entirely. Whatever you ask `lexd` to produce, it's the same job — once parsed, IR transform + serializer is < 10% on top.

2. **Subtracting 1.7 ms startup gives parse + transform + serialize ≈ bench parse number**:
   - `010-kitchensink` end-to-end 10.5 − 1.7 = 8.8 ms vs bench parse 7.35 ms → ~1.4 ms for transform+serialize
   - `080-gentle-introduction` end-to-end 59.9 − 1.7 = 58.2 ms vs bench parse 53.7 ms → ~4.5 ms for transform+serialize

   The earlier "parse-only is 450× slower than comrak" carries through to the user-facing operation almost unchanged (because everything else is small).

3. **lexd outruns pandoc end-to-end** by 4×–19×, and the gap is widest on small files (where pandoc's 25 ms startup tax dominates) and narrowest on large files (where actual processing matters). Even on the largest fixture (24 KB), lexd is 4× faster than pandoc — despite Lex's parser being 450× slower than comrak per byte.

   Why: pandoc's pipeline (parse to pandoc AST → filters → serialize) is doing a lot more per byte than `comrak::parse_document` alone. Lex tooling stays lean — small startup, no filter framework, direct AST→IR→output path. The parser is the slow link, but the rest of the pipeline is taut.

---

## Where Lex sits

| Layer | What we measured | Verdict |
|---|---|---|
| **Wire-codec dispatch tax** | Include resolution through the extension registry | < 0.6% — invisible. |
| **Raw parser throughput** | parse_document on 2–25 KB docs | ~0.3–0.5 MB/s. Slow end of parsers; comrak is ~500× faster. |
| **End-to-end CLI ops** (`format`, `convert`) | hyperfine `target/release/lexd …` | 10–60 ms across 2.4–24 KB. 4–19× faster than pandoc. |
| **LSP re-parse budget** | parse_document on the LSP debounce path | Largest fixture (24 KB) parses in 60 ms — under the 100 ms snappy threshold. Tree-sitter handles the per-keystroke layer; lex-core runs async. |

### Reading

- For **interactive tools (LSP, lexed, vscode)**: the parse cost is fine. Tree-sitter handles the keystroke loop in sub-millisecond; lex-core's full re-parse on save / debounce stays well under the 100 ms "feels instant" threshold even for medium-large docs. **Not a problem.**
- For **CLI batch use (`lexd convert *.lex`)**: a 100-document corpus at the 080-gentle-introduction size would take ~6 seconds. Comparable to running pandoc (which would take ~25 s on the same corpus). **Faster than the obvious alternative.**
- For **WASM / browser editor (`lex-web`)**: WASM is 1.5–3× slower than native. The 24 KB doc could push toward 100–180 ms, which starts to be felt on save. **Worth watching as `lex-web` matures; would benefit most from parser optimization.**
- For **massive single docs (book-length, 250+ KB)**: full reparse pushes toward 500 ms+, which is noticeable. **Plausible future motivation for parser perf work.**

### If parser perf ever becomes a priority

The 450× gap to comrak suggests room. Likely fruit, in rough order of bang-for-buck:

1. **Regex compilation amortization** — if grammar regexes are re-compiled per parse (not once at module load), that's an easy 2–10× win.
2. **Arena-allocated AST** — what comrak uses; typically 2–5× on allocation-heavy parsers.
3. **Pipeline collapse** — five passes mean five traversals; folding stages cuts the constant factor.

Each is a meaningful chunk of work, none are blocking.

---

## Reproducing

Bench harness (in repo, after PR #553 merges):

```sh
git submodule update --init
python3 crates/lex-core/benches/corpus/gen.py
python3 crates/lex-core/benches/md_corpus/prep.py
cargo bench -p lex-core --bench include_resolve
cargo bench -p lex-core --bench parse_vs_markdown
```

End-to-end matrix (one-shot, not in repo):

```sh
cargo build --release -p lexd
BIN=target/release/lexd
BENCH=comms/specs/benchmark
MD_AUTO=crates/lex-core/benches/md_corpus/auto

for fx in 010-kitchensink 20-ideas-naked 040-on-parsing 080-gentle-introduction; do
  case $fx in
    010-kitchensink|20-ideas-naked) MDFILE="$BENCH/$fx.md" ;;
    *) MDFILE="$MD_AUTO/$fx.md" ;;
  esac
  hyperfine --warmup 3 --runs 20 --shell=none \
    -n "lexd format"     "$BIN format $BENCH/$fx.lex" \
    -n "lexd ->md"       "$BIN $BENCH/$fx.lex --to markdown" \
    -n "lexd ->html"     "$BIN $BENCH/$fx.lex --to html" \
    -n "pandoc md->html" "pandoc $MDFILE -t html -o /dev/null"
done
```
