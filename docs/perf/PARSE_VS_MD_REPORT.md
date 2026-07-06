# Lex parser vs. Markdown parser — order-of-magnitude comparison

**Question.** When parsing equivalent-content documents, is `lex_core::lex::parsing::parse_document` 2×, 200×, or 2000× slower than `comrak::parse_document` (the Markdown parser lex-babel already depends on)?

**Bottom line.** ~450× slower, remarkably stable across four documents spanning 2 KB–25 KB. Lex parses at ~0.3–0.5 MB/s; comrak at ~130–270 MB/s. Not pathological for what Lex's parser is doing (indentation-significant, regex-driven, multi-stage with range tracking) — but plenty of room if parser perf ever becomes a priority.

---

## Methodology

- **Parsers**: `lex_core::lex::parsing::parse_document` (the public entry behind `lexd inspect` / lex-lsp) vs `comrak::parse_document` with `ComrakOptions::default()` (CommonMark only, no GFM extensions).
- **Allocation parity**: each iteration allocates fresh state — Lex returns an owned `Document`, comrak gets a fresh `Arena::new()` (its arena is a `Vec`-backed allocator, equivalent shape to what Lex does).
- **Tool**: Criterion 0.5, 100 samples, 3 s warmup, 10 s measurement window per bench.
- **Corpus**: four documents from `comms/specs/benchmark/`. Two pairs are hand-authored in both formats (tier A — fairest); two have the `.md` produced by `lexd … --to markdown` (tier B — production lex-babel converter, well exercised).

| # | Fixture | Lex bytes | MD bytes | MD source |
| --- | --- | ---: | ---: | --- |
| 1 | `010-kitchensink` | 2 456 | 2 189 | hand-authored |
| 2 | `20-ideas-naked` | 10 057 | 9 453 | hand-authored |
| 3 | `040-on-parsing` | 12 646 | 11 541 | auto-converted |
| 4 | `080-gentle-introduction` | 24 655 | 23 462 | auto-converted |

The Lex source is consistently ~5–10% larger than the Markdown encoding, but the ratio holds whether you look at per-call wall time or per-byte throughput.

---

## Results

| Fixture | Lex parse | MD parse | **Ratio** | Lex MB/s | MD MB/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| 010-kitchensink | 7.35 ms | 17.0 µs | **432×** | 0.33 | 129 |
| 20-ideas-naked | 18.63 ms | 34.6 µs | **538×** | 0.54 | 273 |
| 040-on-parsing | 22.94 ms | 51.1 µs | **449×** | 0.55 | 226 |
| 080-gentle-introduction | 53.72 ms | 122 µs | **440×** | 0.46 | 192 |

Tier A (hand-authored) ratios: 432×, 538×.
Tier B (auto-converted) ratios: 449×, 440×.

The tier A vs tier B numbers agree closely, so the converter's choices aren't biasing the result.

---

## Honest reading

**Why is comrak so fast?** ~200 MB/s on a single core is the upper end of the parser performance band. comrak is event-driven, largely single-pass, has been optimized for years, and CommonMark's grammar is relatively flat (no indentation context, no annotation system, no extension hooks). It's a battle-tested ceiling, not a typical baseline.

**Why is Lex slow?** Lex's parser is a five-stage pipeline (semantic indentation → line grouping → tree building → context injection → parse-by-level → assembly), driven by regex patterns rather than a hand-rolled state machine. It tracks `Range` info on every node (line/col + origin path), collects annotations and attaches them in a separate pass, and pays for indentation-significant parsing that flat-grammar parsers don't. ~0.3–0.5 MB/s is in the band typical for regex-heavy multi-pass parsers.

**Does this matter?**

- **Interactive editing (LSP)**: the largest fixture (25 KB) parses in 54 ms — well under the ~100 ms threshold for "feels instant" on a re-parse-per-keystroke workflow. *Not a problem.*
- **Batch / CI / build**: a 250 KB document (e.g. a multi-include book) would take ~500 ms to parse on each rebuild. *Possibly annoying for large projects, not blocking.*
- **WASM / browser editor**: WASM is 1.5–3× slower than native. The 25 KB doc could push toward 100–150 ms, which starts to be felt. *Worth watching as `lex-web` matures.*

**If parser perf ever becomes a priority**, likely fruit:

1. Regex compilation amortization — `Regex::new` is expensive; if patterns are rebuilt per parse rather than compiled once, that's a quick win.
2. Reducing AST allocation pressure — if every node is a separate `Box`, switching to an arena (the same trick comrak uses) typically yields 2–5×.
3. Collapsing the multi-stage pipeline — five passes mean five traversals of the input; a tighter design could fold steps together.

These are "if/when" suggestions, not a backlog item. The current parser is fine for the use cases it serves.

---

## Caveats

- comrak with default options is CommonMark-only. Enabling all GFM extensions (tables, footnotes, etc.) would shift comrak's number, but not by an order of magnitude.
- This is parse-only — no include resolution, no semantic analysis, no formatting. The include-resolve bench (`include_resolve`) covers the resolution layer separately.
- Comparing parsers across formats is inherently a category error (different grammars do different work). The number that matters is "is Lex unreasonably slow vs a known-fast baseline?" — answer: it's slower by a clear factor, but the factor is reasonable given what the parser is doing.

---

## Reproducing

```sh
git submodule update --init                       # pulls comms/specs/benchmark/
python3 crates/lex-core/benches/md_corpus/prep.py # auto-converts tier B fixtures
cargo bench -p lex-core --bench parse_vs_markdown
```
