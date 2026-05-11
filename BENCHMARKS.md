# Lex Performance Baselines

This document tracks Lex's performance metrics against established Markdown tools. Because Lex is fundamentally more expressive (hierarchical sessions, robust include system, structural annotation), a "perfect" implementation would still be several times slower than pure Markdown parsers. 

The goal here is not parity, but to **establish a baseline**, track regressions, and understand how parser throughput affects end-user latency (interactive editing vs. batch processing).

## 1. Methodology

We measure performance across three dimensions:

1. **End-to-End CLI Latency (`hyperfine`)**: Measures the user-visible time to boot, parse, transform, and emit a document. Tools compared: `lexd`, `cmark` (C reference implementation), and `pandoc`.
2. **Raw Parser Throughput (`criterion`)**: Isolates pure parsing time. Compares `lex_core::lex::parsing::parse_document` against `comrak::parse_document`.
3. **Extension-System Tax (`criterion`)**: Measures the overhead of routing `lex.include` resolutions through the extension host and wire codec.

### Corpus and Payloads

- **Tier A (Human-Authored)**: Documents written manually in both formats (e.g., `010-kitchensink`).
- **Tier B (Auto-Converted)**: Documents converted from `.lex` to `.md` via `lexd` (e.g., `040-on-parsing`).
- **Tier C (Synthetic Scaling)**: Pure prose payloads strictly for testing throughput scaling bounds (10 KB, 100 KB, 1 MB).

---

## 2. Baselines & Measurements

*(Note: Data reflects a 2026-05-11 dev workstation on macOS aarch64. Regenerate with instructions below.)*

### 2.1. CLI Startup Tax

Startup baseline (process boot + arg parsing, no actual work):

| Tool | Mean Latency |
|---|---:|
| `cmark --version` | **1.5 ms** |
| `lexd --version` | **1.7 ms** |
| `pandoc --version` | **24.8 ms** |

**Observation**: `lexd` has virtually no startup tax compared to the bare-metal C `cmark`, whereas `pandoc` (Haskell) pays a ~25ms initialization cost.

### 2.2. End-to-End CLI Processing

Measuring the time to read a file, parse it, and emit HTML.

| Fixture | Size | `lexd → html` | `cmark md → html` | `pandoc md → html` |
|---|---:|---:|---:|---:|
| `010-kitchensink` (Tier A) | 2.4 KB | 10.6 ms | 1.5 ms | 200 ms |
| `20-ideas-naked` (Tier A) | 9.8 KB | 23.2 ms | 1.6 ms | 222 ms |
| `040-on-parsing` (Tier B) | 12.3 KB | 27.7 ms | 1.7 ms | 224 ms |
| `080-gentle-introduction` (Tier B) | 24.1 KB | 60.6 ms | 1.8 ms | 241 ms |

**Observation**: 
- `cmark` demonstrates the theoretical floor (~0.3 ms of real work). 
- `lexd` sits in the middle tier. It is 4–19× faster than `pandoc` end-to-end, making it excellent for CLI batch workflows.

### 2.3. Raw Parser Throughput & Scaling

Isolating the AST construction cost between `lex-core` and `comrak`.

| Payload / Size | `lex-core` Parse | `comrak` Parse | Multiplier Gap |
|---|---:|---:|---:|
| Realistic Docs (~2–24 KB) | ~0.3–0.5 MB/s | ~130–270 MB/s | **~450× slower** |
| Synthetic 10 KB (Tier C) | 13.9 ms | 0.041 ms | **~339× slower** |
| Synthetic 100 KB (Tier C) | 171.0 ms | 0.406 ms | **~421× slower** |
| Synthetic 1 MB (Tier C) | 4755.9 ms | 4.15 ms | **~1145× slower** |

**Observation**: The gap is substantial but acceptable. If parser performance ever becomes a priority, allocating an Arena-based AST and amortizing regex compilations are likely to bridge this.

### 2.4. Extension System Tax

Does routing `lex.include` through the wire codec slow things down?

**Verdict:** **No, < 0.6%**. The overhead is invisible against the measurement floor across 7 synthetic scenarios (from single inclusions to deep chains).

---

## 3. Analysis by Use Case

- **Interactive Editing (LSP, VSCode, Neovim)**: Parse cost is fine. `tree-sitter` handles the keystroke loop in sub-millisecond time. A full `lex-core` re-parse on save/debounce sits comfortably under the 100ms "feels instant" threshold for medium-large docs (up to ~40KB). **Not a problem.**
- **CLI Batch (`lexd convert *.lex`)**: Faster than `pandoc` by an order of magnitude. **Excellent.**
- **WASM / Browser (`lex-web`)**: WASM is 1.5–3× slower than native. A 24 KB document may approach 100–180ms, making latency noticeable on save. **Worth monitoring as `lex-web` matures.**
- **Book-length Single Doc (250+ KB)**: A full reparse pushes toward 500ms+, causing noticeable stutter. **This represents the primary future motivation for parser perf work.**

---

## 4. Reproducing

The benchmarking suite uses `criterion` for micro-benchmarks and `hyperfine` for end-to-end testing.

### Running Parser & Resolver Benchmarks

```sh
# 1. Pull realistic payload submodules
git submodule update --init

# 2. Generate synthetic tier C scaling payloads & include rig
python3 crates/lex-core/benches/corpus/gen.py

# 3. Auto-convert tier B markdown fixtures via lexd
python3 crates/lex-core/benches/md_corpus/prep.py

# 4. Execute benchmarks
cargo bench -p lex-core --bench include_resolve
cargo bench -p lex-core --bench parse_vs_markdown
```

### Running E2E Matrix

*(Requires `hyperfine`, and optionally `cmark` and `pandoc` installed via homebrew/apt).*

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

> **Note:** Neither bench runs in CI, as runner noise dwarfs the deltas needed to detect regressions. Run locally on PRs that touch the resolver, the parser, or the wire codec.
