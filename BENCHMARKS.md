# Lex performance — snapshot

Where Lex sits today, in three measurements taken together. All numbers from a 2026-05-11 dev workstation (macOS aarch64); regenerate with the commands in [Reproducing](#reproducing) below if you want fresh ones.

## Headline

| Layer | What we asked | Verdict |
|---|---|---|
| **Extension-system tax** | Does routing `lex.include` through the wire codec slow things down? | **No, < 0.6%** — invisible. |
| **Raw parser throughput** | How does `lex_core::lex::parsing::parse_document` compare to `comrak::parse_document`? | **~450× slower per byte** — Lex parses at ~0.3–0.5 MB/s; comrak at ~130–270 MB/s. |
| **End-to-end CLI** | Does that translate into slow `lexd format` / `convert` for users? | **No** — comparable to or faster than pandoc, well within interactive budgets. |

## End-to-end positioning

Hyperfine (`--warmup 3 --runs 30 --shell=none`) on a freshly-built `target/release/lexd` (v0.11.0), the homebrew `cmark` 0.31.2 (CommonMark reference C implementation), and the homebrew `pandoc`.

**Startup baselines** (process boot + arg parsing, no work):

| Tool | Mean |
|---|---:|
| `cmark --version` | **1.5 ms** |
| `lexd --version` | **1.7 ms** |
| `pandoc --version` | **24.8 ms** |

**Per-fixture** (`<fixture>.lex` for lexd; `<fixture>.md` — equivalent content — for cmark/pandoc):

| Fixture | Size | `lexd → html` | `cmark md → html` | `pandoc md → html` |
|---|---:|---:|---:|---:|
| 010-kitchensink | 2.4 KB | 10.6 ms | 1.5 ms | 200 ms |
| 20-ideas-naked | 9.8 KB | 23.2 ms | 1.6 ms | 222 ms |
| 040-on-parsing | 12.3 KB | 27.7 ms | 1.7 ms | 224 ms |
| 080-gentle-introduction | 24.1 KB | 60.6 ms | 1.8 ms | 241 ms |

Three tiers visible:

1. **cmark — basically just startup.** ~0.3 ms of actual work on a 24 KB doc. C reference impl, single-pass, no extensions. The "what's possible" floor.
2. **lexd — middle tier.** 7–34× slower than cmark end-to-end (more on larger docs, where parse dominates over startup), 4–19× faster than pandoc.
3. **pandoc — slow startup + slow processing.** 25 ms startup tax + ~1.5 KB/ms processing through the pandoc-AST + filter pipeline.

`lexd format`, `lexd → md`, and `lexd → html` agree within 1–2% on every fixture — parse cost dominates entirely. Whatever you ask `lexd` to produce, it's the same job; once parsed, IR transform + serializer is < 10% on top.

## Reading by use case

- **Interactive editing (LSP, lexed, vscode, nvim)**: parse cost is fine. Tree-sitter handles the keystroke loop in sub-millisecond; lex-core's full re-parse on save / debounce stays well under the 100 ms "feels instant" threshold even for medium-large docs (largest fixture: 60 ms). **Not a problem.**
- **CLI batch (`lexd convert *.lex`)**: faster than pandoc on the same corpus by 4–19×. **Faster than the obvious alternative.**
- **WASM / browser editor (`lex-web`)**: WASM is 1.5–3× slower than native. The 24 KB doc could push toward 100–180 ms, which starts to be felt on save. **Worth watching as `lex-web` matures.**
- **Book-length single doc (250+ KB)**: full reparse pushes toward 500 ms+, which is noticeable. **Plausible future motivation for parser perf work.**

## If parser perf ever becomes a priority

The 450× per-byte gap to comrak suggests room. Likely fruit, in rough order of bang-for-buck:

1. **Regex compilation amortization** — if grammar regexes are re-compiled per parse rather than once at module load, that's an easy 2–10× win.
2. **Arena-allocated AST** — what comrak uses; typically 2–5× on allocation-heavy parsers.
3. **Pipeline collapse** — five passes mean five traversals; folding stages cuts the constant factor.

Each is a meaningful chunk of work, none are blocking.

## Reproducing

The two Criterion benches live in `crates/lex-core/benches/`:

```sh
git submodule update --init                       # pulls comms/specs/benchmark/
python3 crates/lex-core/benches/corpus/gen.py     # synthetic include corpus
python3 crates/lex-core/benches/md_corpus/prep.py # auto-converts tier-B md fixtures via lexd

cargo bench -p lex-core --bench include_resolve   # extension-system tax
cargo bench -p lex-core --bench parse_vs_markdown # parser throughput vs comrak
```

End-to-end matrix (one-shot, requires `hyperfine`, optional `cmark` + `pandoc`):

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
  hyperfine --warmup 3 --runs 30 --shell=none \
    -n "lexd ->html"     "$BIN $BENCH/$fx.lex --to html" \
    -n "cmark md->html"  "cmark $MDFILE" \
    -n "pandoc md->html" "pandoc $MDFILE -t html -o /dev/null"
done
```

Neither bench runs in CI — runner noise dwarfs the deltas these would need to detect. Run by hand on PRs that touch the resolver, the parser, or the wire codec.
