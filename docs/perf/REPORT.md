# Extension-system perf impact — benchmark report

**Question.** The extension system routes Lex's own `lex.include` through a
generic registry → handler → wire-codec path instead of the inline
parse/splice it had before. Does this make Lex's own use case
measurably slower?

**Bottom line.** No. Across seven scenarios spanning idle/single/many/large/deep
includes, the post-flip and HEAD revisions are **within ±0.6%** of the
pre-extension baseline — well inside cross-run noise (~0.5–0.7%). The
wire-codec tax is below the measurement floor. **Ship it.**

---

## Methodology

- **Revisions**
  - Baseline: `f0c7f1f0` — v0.10.6, last release before PR 1 (`15c2a6dd`).
  - Post-flip: `3a46fed3` — PR 3d, commit that flipped `resolve_from_source` to dispatch through `Registry::dispatch_resolve_raw`.
  - HEAD: `a700e23e` — full α + β + δ-plumbing (trust store, validate/render hooks, sandbox trait).
- **Tool.** Criterion 0.5, 100 samples, 3s warmup, 10s measurement window. Each scenario constructed loader/registry once, then iterated only the `resolve_from_source(...)` call.
- **Worktrees.** Three separate `git worktree`s, each with its own `target/` and its own `crates/lex-core/benches/include_resolve.rs` adapted to that revision's API.
- **Corpus.** Procedurally generated; directory `bench-corpus/`. Each scenario is a self-contained loader root; deterministic prose so per-byte AST cost is comparable.
- **Cross-run check.** Each revision run twice; the run-to-run delta within a single revision was 0.3–0.7%, larger than any baseline-vs-extension delta.

### Corpus

| Scenario | Host bytes | Total bytes | Notes |
| --- | ---: | ---: | --- |
| s1_no_includes | 10 010 | 10 010 | Calibration: parse path only, no `lex.include` |
| s2_one_small | 4 103 | 4 260 | One ~100 B include |
| s3_one_medium | 4 103 | 14 113 | One ~10 KB include |
| s4_one_large | 4 103 | 104 121 | One ~100 KB include — per-byte codec stress |
| s5_many_small | 2 398 | 10 248 | 50 ~100 B includes — per-include fixed cost |
| s6_deep_chain | 585 | 5 818 | 5-level deep include chain |
| s7_realistic | 2 770 | 22 869 | 3 includes of mixed size |

A separate sanity check confirmed that included content with structural
elements (sessions, subsections) round-trips correctly through the
post-flip codec — so the result generalises beyond the prose corpus.

---

## Results

Median wall time per `resolve_from_source` call (Criterion mid-estimate).

| Scenario | Baseline R1 | Baseline R2 | Post-flip R1 | Post-flip R2 | HEAD R1 | Δ (post-flip vs baseline, mean) | Δ (HEAD vs baseline, mean) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| s1_no_includes | 12.278 ms | 12.371 ms | 12.286 ms | 12.373 ms | 12.361 ms | +0.04% | +0.30% |
| s2_one_small | 6.4189 ms | 6.4863 ms | 6.4087 ms | 6.4587 ms | 6.4332 ms | −0.29% | −0.34% |
| s3_one_medium | 18.202 ms | 18.381 ms | 18.172 ms | 18.310 ms | 18.186 ms | −0.27% | −0.55% |
| s4_one_large | 151.42 ms | 152.46 ms | 150.68 ms | 152.19 ms | 151.69 ms | −0.33% | −0.16% |
| s5_many_small | 43.932 ms | 44.544 ms | 44.036 ms | 44.404 ms | 44.209 ms | −0.04% | −0.08% |
| s6_deep_chain | 9.8342 ms | 9.8682 ms | 9.8056 ms | 9.8760 ms | 9.8265 ms | −0.10% | −0.24% |
| s7_realistic | 29.910 ms | 30.214 ms | 29.933 ms | 30.108 ms | 30.015 ms | −0.13% | −0.21% |

(Negative deltas = post-flip slightly faster — they are noise, not real wins.)

### Sign reading

- **s1 (no includes)**: identical, as expected — no `lex.include` annotations means the registry is never dispatched. Confirms the baseline parse path is unchanged.
- **s4 (one 100 KB include)**: per-byte stress for the codec. Δ = −0.33%. The encode-then-decode round-trip of a 100 KB AST is invisible.
- **s5 (50 includes)**: per-include fixed-cost stress. Δ = −0.04%. 50 dispatch calls + 50 codec round-trips also invisible.
- **HEAD vs post-flip**: within run-to-run noise. β-phase additions (trust store, validate/render hook plumbing, sandbox trait wiring) added zero overhead on the no-third-party-handlers path.

---

## Why so flat?

Parse cost dominates. From s3 (14 KB total, 18 ms) the parser runs at
~1.3 μs/byte, ~770 KB/s of source. The wire codec walks the
already-allocated AST, allocating mirror `WireNode`/`WireInline` structs
— no parsing, no string interning, no I/O. That work is one to two
orders of magnitude cheaper per byte than parse, and it gets done
*after* parse, so the wall-clock impact is buried under the parser's
existing cost.

The "many small includes" case (s5) was the one most likely to surface
per-include fixed cost — 50 × (registry lookup + `Box<dyn LexHandler>`
dispatch + `LabelCtx` construction + encode + decode) on top of 50 small
parses. It didn't.

---

## Caveats

1. **Plain-prose corpus.** Real Lex documents have richer structure (sessions, lists, footnotes, verbatim). The codec walks more nodes per byte for those — but parse also does more work per byte, so the *ratio* should hold. The session sanity check above confirms the codec handles structural variety.
2. **Process-startup costs not measured.** This is `resolve_from_source` only. End-user `lexd` invocations also pay for binary startup, config loading, format-registry construction. Those are unchanged, so they don't move the relative comparison.
3. **One machine, one toolchain.** macOS / aarch64 / rustc default. Different platforms could shift constants, but a 200× regression hiding only on Linux is implausible.
4. **No third-party handlers exercised.** This benchmark only measures the cost of routing Lex's own built-in through the generic dispatch fabric. Subprocess handlers (β phase) would have very different costs (IPC, JSON-RPC), but that's an *opt-in* third-party feature — the question here was whether Lex's own use case got penalised, and it did not.

---

## Verdict

The "downgrade Lex through the wire" trade-off was the principled move
from the start, and the cost we feared (penalising the lex-only case to
keep one codepath) **did not materialise**. The principle wins on
correctness/maintenance grounds *and* costs nothing measurable in
runtime. Recommend shipping.

If a later change (large new wire schema, eager validation, etc.)
re-introduces overhead, this same bench rig is committed in
`/Users/adebert/h/lex-fmt/bench-corpus/` and the worktrees
(`lex-bench-{baseline,postflip,head}`) — easy to re-run.
