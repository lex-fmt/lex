# Issue #613 — Umbrella

**Title:** babel: interop architecture work-stream — symmetrize the IR, unify label dispatch, retire the markdown HACK

**Filed:** <https://github.com/lex-fmt/lex/issues/613>

---

## Context

The lex-babel layer was built when the language spec was smaller and the extension story didn't yet exist. Several recent additions — typed tables, document title/subtitle, document-scope annotations (#570 Phase 3a), the label-extension registry, and the render-hook dispatch (#524 / #541) — landed cleanly as additions, but the underlying conversion model now has visible seams. This issue is the umbrella for the architectural cleanup that closes those seams. See #612 for v1 interop scope.

## The four seams

**(A) The IR is one-way in important places.** `Document::document_annotations` is populated lex→IR but ignored on the way back (`crates/lex-babel/src/ir/to_lex.rs:88-100`, contract documented at `crates/lex-babel/src/ir/nodes.rs:33-44`); a legacy `frontmatter` synthesis in `crates/lex-babel/src/common/nested_to_flat.rs:239-290` papers over it. Reference sub-types collapse from 8 lex-core variants to 2 IR variants. Heading levels and inline nesting are similarly lossy. Any consumer that wants to round-trip data through the IR can't fully trust it.

**(B) There are two parallel label-dispatch surfaces.** Verbatim dispatch runs at IR construction (`from_lex_verbatim` in `crates/lex-babel/src/ir/from_lex.rs:314-397`) and produces typed IR nodes for `lex.tabular.table`, `lex.media.*`. Render dispatch runs at serialization (`crates/lex-babel/src/render_dispatch.rs`, ~883 lines) and produces format-specific output. Both gate on the same `Registry` schema; together they make "what does this label do in interop?" a three-step answer (does it have an IR hydration mapping → does it have a render hook → fallback). Extension authors register against two surfaces with different shapes. Coupling extensions to parsing/IR-build the way verbatim dispatch does today is the wrong split — the split should be on **lifecycle** (IR-build vs render), not on what the handler is "for."

**(C) Render dispatch walks lex-core AST, not the IR.** `render_dispatch.rs` is a second walk of the document after `to_ir` already built one. The "centralize the hard logic in the IR" principle stated in `crates/lex-babel/src/lib.rs:60-64` now has two centers.

**(D) The markdown HACK at `crates/lex-babel/src/formats/markdown/serializer.rs:484` is the visible symptom.** The author's inline comment identifies the fix: `tree_to_events` should emit a typed event for annotation bodies that have a registered render hook, so the markdown layer doesn't have to peek ahead. The HACK can't land until (A), (B), and (C) land.

## Why this is one project

These are not four independent issues. The dependency order is forced:

```text
(A) Symmetric IR
       ↓
       ├──→ enables (B) Unified dispatch surface (extensions can register against
       │    IR operations because IR is now a reliable substrate)
       │
       └──→ enables (C) Migrate render-dispatch to IR
                          ↓
                          └──→ enables (D) Retire markdown HACK
```

Filing them as independent issues without ordering guarantees PRs that break each other's tests. The four sub-issues exist to scope work, but they ship in this order behind one design pass.

## Sub-issues

- **(A)** #614 — Symmetric IR: Phase 3b for `document_annotations`, reference sub-type promotion, audit every DocNode for round-trip parity.
- **(B)** #615 — Unified label-dispatch surface: single Registry shape for IR-hydration and render hooks. Includes `doc.*` namespace migration for document-level metadata.
- **(C)** #616 — Migrate `render_dispatch` from lex-core AST to IR.
- **(D)** #617 — Retire markdown HACK via typed event from `tree_to_events`.

## Acceptance bar (work-stream as a whole)

- `to_ir(doc)` followed by `from_ir(ir)` is structurally lossless for all v1 elements, with explicitly-documented exceptions (heading-level reconstruction, inline-format nesting) that have proptest coverage.
- Extension authors register one type of handler against one registry; the same handler is invoked at IR-build and at serialization through clear lifecycle hooks.
- `render_dispatch.rs` no longer walks `lex_core::lex::ast::Document`. It operates on the IR.
- The HACK comment in `formats/markdown/serializer.rs:484` is deleted; metadata annotations round-trip lex→markdown→lex without loss.
- The "metadata label whitelist" (currently hardcoded as `author, note, title, date, tags, category, template`) is sourced from the Registry schema, not a constant. The document-level subset migrates to the reserved `doc.*` namespace (`doc.title`, `doc.author`, `doc.date`, `doc.tags`, `doc.category`, `doc.template`); `note` is content-level and stays in its natural namespace.

## Out of scope

- Implementing Pandoc, LaTeX, or HTML import (see #612).
- HTML wrapper bloat (#604, #610) — parallel.
- The non-architectural cleanups (diagnostic-format namespace, PDF/PNG chrome.rs extraction, html_escape crate, etc.) — parallel.

## Sequencing note

Sub-issue (A) #614 is the mini-project. Realistically 1–2 weeks of focused work for the IR audit + Phase 3b + reference sub-types alone. (B) #615, (C) #616, (D) #617 are each ~3-5 days once (A) is done. Work-stream owner should plan for ~4-6 weeks calendar time.
