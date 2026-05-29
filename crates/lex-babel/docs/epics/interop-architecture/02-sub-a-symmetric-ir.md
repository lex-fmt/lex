# Issue #614 — Sub A: Symmetric IR

**Title:** babel/ir: advance IR ↔ Lex symmetry — Phase 3b document annotations, reference sub-types, round-trip audit

**Filed:** <https://github.com/lex-fmt/lex/issues/614>

---

Part of the interop architecture work-stream — see umbrella #613. The IR is the substrate for nearly every interop path; it must be a reliable round-trip layer (with documented, proptest-anchored accepted losses) before the rest of the work-stream can build on it. The framing here is **advance** rather than "complete" — a small set of losses (heading levels, inline-format nesting, Video/Audio inline) are accepted with explicit documentation and proptest contracts rather than fixed in this work-stream.

## Three concrete gaps

### 1. `Document::document_annotations` is one-way

Contract documented at `crates/lex-babel/src/ir/nodes.rs:33-44`:

> "The slot is populated **one-way** from `from_lex_document` (lex → IR direction); `to_lex_document`, `tree_to_events`, and `events_to_tree` do **not** consume or emit it today."

The synthesis in `crates/lex-babel/src/common/nested_to_flat.rs:239-290` builds a `frontmatter` annotation from this slot on each event walk; the comment block at `crates/lex-babel/src/ir/to_lex.rs:88-100` documents Phase 3b as pending. `to_lex_annotation_raw` exists at `to_lex.rs:140` with `#[allow(dead_code)]` as the bridge.

**Phase 3b flips the source of truth atomically**: `to_lex_document` reads `document_annotations`, the legacy synthesis retires, the `#[allow(dead_code)]` comes off.

### 2. Reference sub-types collapse

lex-core distinguishes 8 reference variants at `crates/lex-core/src/lex/ast/elements/inlines/references.rs:25-44`:

```rust
ReferenceType: ToCome { id }, Citation(CitationData), AnnotationReference { label },
               FootnoteNumber { number }, Session { target }, Url { target },
               File { target }, General { target }, NotSure
```

The IR collapses them to two opaque variants in `crates/lex-babel/src/ir/nodes.rs`:

```rust
InlineContent::Reference(String)
InlineContent::Link { text, href }
```

Every format adapter then re-parses or guesses. The IR should carry the classification through.

### 3. Round-trip parity audit

| Element | Loss | Location |
|---|---|---|
| Heading | Level info reconstructed from parent context, not stored | `to_lex.rs:166-201` |
| Bold/Italic nesting | `Bold([Italic([Text])])` flattens to `*_text_*` text | `to_lex.rs:517-541` |
| Annotation rich body | OK structurally; semantics depend on dispatch (see umbrella) | — |
| Image (block + inline) | `DocNode::Image` + `InlineContent::Image` exist; round-trips | `nodes.rs` |
| Video / Audio inline | `DocNode::Video` / `DocNode::Audio` exist as block-level; no `InlineContent::Video` / `InlineContent::Audio` variants — inline-positioned video/audio are unrepresentable | `nodes.rs` |

Each needs either a fix or a *documented* accepted-loss with proptest coverage.

## Fix sketch

1. **Phase 3b flip** — single atomic PR. Wire `to_lex_document` to emit from `document_annotations`; retire `emit_frontmatter_event` in `nested_to_flat.rs:239-290`; drop `#[allow(dead_code)]` on `to_lex_annotation_raw`.

2. **Promote reference sub-types in IR** — replace `InlineContent::Reference(String)` with a structured variant carrying the same classification lex-core uses. Anchor-heuristic resolution (`common/links.rs`) continues to produce `Link { text, href }` for Url/File/General references where appropriate. Format adapters dispatch on the structured variant instead of re-parsing the raw string.

3. **Round-trip audit** — one DocNode at a time, add a proptest that runs `to_ir(from_ir(to_ir(ast)))` and asserts structural equivalence. Where a loss is intentional, document it in the IR node's doc-comment with the proptest as the contract.

## Acceptance criteria

- Phase 3b PR lands; legacy `frontmatter` synthesis is removed; `document_annotations` is the single source of truth.
- `InlineContent::Reference` is structured; every format adapter uses the typed variants; `common/links.rs` operates on the typed variants.
- For every DocNode variant: a round-trip proptest exists. Where the test asserts equivalence-modulo-X, X is named in the doc-comment. The documented-loss list (heading levels, inline-format nesting, Video/Audio inline) is committed text in the IR module docs, not just inferred from test failures.
- The "metadata label whitelist" in `formats/markdown/serializer.rs` is unblocked for removal (final removal lands in Sub D once dispatch is unified).

## Out of scope

- Heading-level reconstruction (accept as documented loss; proptest contract names the loss).
- Inline format nesting normalization (accept as documented loss; proptest contract names the loss).
- Video/Audio inline variants — accept as documented loss for v1. File a follow-up issue only if a concrete inline use case emerges.

## Dependencies

Part of work-stream #613. Blocks Sub B (#615), Sub C (#616), Sub D (#617).
