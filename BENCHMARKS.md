# Lex Performance Baselines

This document tracks Lex's performance against established Markdown tools. Lex is fundamentally more expressive (hierarchical sessions, robust include system, structural annotation), so a "perfect" implementation would still be several times slower than pure Markdown parsers.

The goal isn't parity. It's to **establish a baseline**, track regressions, understand how parser throughput affects end-user latency (interactive editing vs. batch processing), and surface the load-bearing finding that the parser is super-linear in input size.

## 1. Methodology

We measure performance across three dimensions:

1. **End-to-End CLI Latency (`hyperfine`)**: User-visible time to boot, parse, transform, and emit a document. Tools compared: `lexd`, `cmark` (C reference implementation), and `pandoc`.
2. **Raw Parser Throughput (`criterion`)**: Pure parsing time. Compares `lex_core::lex::parsing::parse_document` against `comrak::parse_document`.
3. **Extension-System Tax (`criterion`)**: Overhead of routing `lex.include` resolutions through the extension host and wire codec.

### Corpus and Payloads

- **Tier A (Human-Authored)**: Documents written manually in both formats (`010-kitchensink`, `20-ideas-naked`).
- **Tier B (Auto-Converted)**: `.lex` documents converted to `.md` via `lexd … --to markdown` (`040-on-parsing`, `080-gentle-introduction`).
- **Tier C (Synthetic Scaling)**: Pure-prose payloads, byte-identical `.lex`/`.md`, sized for asymptotic behavior (10 KB, 100 KB, 1 MB).

---

## 2. Baselines & Measurements

*(Data from a 2026-05-11 dev workstation, macOS aarch64. Regenerate with the instructions in §4.)*

### 2.1. CLI Startup Tax

Process boot + arg parsing, no actual work:

| Tool | Mean Latency |
|---|---:|
| `cmark --version` | **1.5 ms** |
| `lexd --version` | **1.7 ms** |
| `pandoc --version` | **24.8 ms** |

**Observation**: `lexd` startup is on par with the bare-metal C `cmark`. `pandoc` (Haskell) pays a ~25 ms initialization tax per invocation — the dominant cost for small documents.

### 2.2. End-to-End CLI Processing

Time to read a file, parse it, and emit HTML.

| Fixture | Size | `lexd → html` | `cmark md → html` | `pandoc md → html` |
|---|---:|---:|---:|---:|
| `010-kitchensink` (Tier A) | 2.4 KB | 10.6 ms | 1.5 ms | 200 ms |
| `20-ideas-naked` (Tier A) | 9.8 KB | 23.2 ms | 1.6 ms | 222 ms |
| `040-on-parsing` (Tier B) | 12.3 KB | 27.7 ms | 1.7 ms | 224 ms |
| `080-gentle-introduction` (Tier B) | 24.1 KB | 60.6 ms | 1.8 ms | 241 ms |

**Observations**:
- `cmark` shows the floor: ~0.3 ms of real work even on 24 KB. The C reference impl is single-pass, no extensions.
- `lexd` sits in the middle tier — 7–34× slower than `cmark` end-to-end (gap widens with doc size as parse work dominates over equal startup cost), 4–19× faster than `pandoc` (gap narrows with doc size as `pandoc`'s processing catches up to its 25 ms startup).
- `lexd format`, `lexd → md`, and `lexd → html` agree within 1–2% on every fixture. Parse cost dominates entirely; once parsed, the IR transform + serializer is < 10% on top. **The only number that matters end-to-end is parse time.**

### 2.3. Raw Parser Throughput & Scaling

Isolated AST construction cost between `lex-core` and `comrak`. The `comrak` configuration matches `lex_babel::formats::markdown::parser::default_comrak_options` (CommonMark + GFM extensions: table, strikethrough, autolink, tasklist, superscript, front matter) so the comparison is against the same parser configuration Lex actually uses in production — not against bare `ComrakOptions::default()`, which would bias `comrak` favourably with a less-featureful parse.

Tier C exposes scaling behavior that the realistic-doc tier can't see (same content, controlled size).

| Payload | `lex-core` parse | `comrak` parse | **Multiplier gap** |
|---|---:|---:|---:|
| Realistic docs (Tier A/B, 2–24 KB) | ~0.3–0.5 MB/s | ~120–220 MB/s | **~270–380×** |
| Synthetic 10 KB (Tier C) | 12.79 ms | 47.5 µs | **~269×** |
| Synthetic 100 KB (Tier C) | 158.2 ms | 498.9 µs | **~317×** |
| Synthetic 1 MB (Tier C) | 3964 ms | 4.53 ms | **~875×** |

**Observation — the load-bearing finding**: Lex's parser is **super-linear in input size**. Throughput drops from ~782 KB/s at 10 KB to ~252 KB/s at 1 MB — a **3× slowdown** as input grows 100×. `comrak` stays flat at ~210 MB/s across the same range (essentially linear), so the gap *more than triples* (269× → 875×) as documents get larger.

Practically: a 1 MB document — book-length but not absurd — takes ~4 seconds to parse. Most of that is in some path that scales worse than O(n).

### 2.4. Extension System Tax

Does routing `lex.include` through the wire codec slow things down?

**Verdict**: **No, < 0.6%**. The overhead is invisible against the measurement floor across 7 synthetic scenarios (single inclusions, fan-out, deep chains).

---

## 3. Analysis by Use Case

- **Interactive Editing (LSP, lexed, vscode, nvim)**: Parse cost is fine. `tree-sitter` handles the keystroke loop in sub-millisecond time. A full `lex-core` re-parse on save/debounce sits well under the 100 ms "feels instant" threshold for medium-large docs (up to ~40 KB). **Not a problem.**
- **CLI Batch (`lexd convert *.lex`)**: 4–19× faster than `pandoc` on the same corpus. **Faster than the obvious alternative.**
- **WASM / Browser (`lex-web`)**: WASM is 1.5–3× slower than native. A 24 KB document may approach 100–180 ms, noticeable on save. **Worth monitoring as `lex-web` matures.**
- **Book-length single doc (250 KB+)**: extrapolating from the 100 KB → 1 MB super-linear curve, a 250 KB doc parses in ~400–500 ms; a 1 MB doc takes ~4 s. **This is the primary future motivation for parser perf work** — and the super-linear scaling means the cost grows faster than file size, so the problem worsens with adoption.

---

## 4. If Parser Perf Becomes a Priority

Tier C makes the levers visible: the gap to `comrak` more than triples between 10 KB and 1 MB, so something in the parser pipeline is doing work proportional to (n × log n) or worse. Likely fruit, in rough order of bang-for-buck:

1. **Find the super-linear path.** The 332× → 1069× progression points to one specific stage that scales badly — most likely a per-line scan that re-walks earlier state (annotation collection, range stamping, or context injection across the multi-stage pipeline). A profile of the 1 MB Tier C run should pinpoint it. Fixing one O(n²) → O(n) hot spot could reclaim most of the gap on large docs.
2. **Regex compilation amortization.** If grammar regexes are re-compiled per parse rather than once at module load, that's an easy 2–10× win on small docs (compilation cost is fixed; it dominates per-call when the actual work is small).
3. **Arena-allocated AST.** What `comrak` uses; typically 2–5× on allocation-heavy parsers. Each `Box` allocation hits the global allocator; an arena turns N allocations into one bump-pointer chain.
4. **Pipeline collapse.** Five passes mean five traversals; folding stages cuts the constant factor and can also eliminate intermediate allocations.

Each is a meaningful chunk of work. None are blocking — interactive use is already fine. Item 1 is the only one with leverage that grows with input size; the others are constant-factor wins.

---

## 5. Reproducing

The benchmarking suite uses `criterion` for micro-benchmarks and `hyperfine` for end-to-end.

### Parser & Resolver Benchmarks

```sh
# 1. Pull realistic payload submodules
git submodule update --init

# 2. Generate synthetic Tier C scaling payloads + include rig
python3 crates/lex-core/benches/corpus/gen.py

# 3. Auto-convert Tier B markdown fixtures via lexd
python3 crates/lex-core/benches/md_corpus/prep.py

# 4. Execute benchmarks
cargo bench -p lex-core --bench include_resolve
cargo bench -p lex-core --bench parse_vs_markdown
```

The 1 MB Tier C scenario takes ~4 s per iteration; expect the parse_vs_markdown bench to run for ~15 minutes including that fixture. Filter to e.g. `--bench parse_vs_markdown -- "/p1_"` to skip large fixtures during quick iterations.

### End-to-End Matrix

*(Requires `hyperfine`; `cmark` + `pandoc` optional, available via homebrew/apt.)*

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

> **Note:** Neither bench runs in CI — runner noise dwarfs the deltas these would need to detect. Run locally on PRs that touch the resolver, the parser, or the wire codec.
