# Issue #617 — Sub D: Retire Markdown HACK

**Title:** babel/markdown: retire the HACK at serializer.rs:484 — adopt format-passthrough splice helper

**Filed:** <https://github.com/lex-fmt/lex/issues/617>

---

Part of the interop architecture work-stream — see umbrella #613.

## Context

The HACK in `crates/lex-babel/src/formats/markdown/serializer.rs:441-503` (peak at :484) exists because the markdown layer needs to wrap metadata-annotation body content in HTML comments, but the event stream doesn't let it peek ahead. The author's inline comment proposed emitting a typed event from `tree_to_events` to solve this. After reviewing the design (see PR #618 discussion), we're taking a slightly different path that **preserves `tree_to_events` as-is**: the splice logic lives in a small consumption-layer helper, and each format provides a one-line callback for raw passthrough.

## Why a consumption-layer splice instead of a new event variant

Every serialization-oriented format on our horizon already has a native concept of "raw passthrough content":

| Format | Native raw-passthrough mechanism |
|---|---|
| Markdown (comrak) | `NodeValue::HtmlBlock { literal }` |
| HTML (rcdom) | Raw text node (or today's splice-sentinel) |
| Pandoc (planned, pandoc_ast) | `Block::RawBlock { format, content }` |
| RFC XML | Raw `<artwork>` / `<sourcecode>` |
| LaTeX (planned) | Verbatim string append |

This isn't lex-specific — it's the standard escape hatch every multi-format toolchain uses. With Sub C landed, `dispatch_render(ir, registry, format)` produces a `RenderPlan`; consuming it via a shared helper means each format adapter just maps the helper's "emit raw passthrough" callback to its native mechanism.

## Fix sketch

1. **New file `crates/lex-babel/src/common/splice.rs`** with a small `SpliceState` helper (~30 LOC) that owns the annotation-indexing, depth-tracking, and skip-until-EndAnnotation logic. Its API surface:

```rust
pub struct SpliceState<'a> { /* plan, annotation_idx, skip_depth */ }

impl<'a> SpliceState<'a> {
    pub fn new(plan: &'a RenderPlan) -> Self;

    /// Call on StartAnnotation. Returns Some(rendered) if this annotation
    /// should be replaced by raw passthrough content. None otherwise.
    pub fn advance_at_start(&mut self, label: &str) -> Option<&str>;

    /// Returns true if the current event should be skipped (inside a
    /// spliced annotation's children).
    pub fn should_skip(&self) -> bool;

    /// Call on EndAnnotation.
    pub fn advance_at_end(&mut self);
}
```

1. **Each format adapter wires `SpliceState` into its event walk** (~5 LOC of wiring) and provides a 1-line `emit_raw_passthrough(content)` callback:

| Format | `emit_raw_passthrough` implementation |
|---|---|
| Markdown | `push_child(parent, NodeValue::HtmlBlock { literal: content.to_string(), .. })` |
| HTML | `append_text_node(parent, content, ContentType::Raw)` |
| (Future) Pandoc | `push_child(parent, Block::RawBlock("markdown".into(), content.to_string()))` |
| (Future) LaTeX | `output.push_str(content)` |

1. **`tree_to_events` and `crates/lex-babel/src/ir/events.rs` are not modified.** The IR/events layer stays exactly as it is today. The splice concern lives entirely in the consumption layer (`common/splice.rs` + each format's walker).

2. **The HACK comment block (`serializer.rs:441-503`) is deleted.**

3. **The hardcoded metadata-label whitelist** (`author, note, title, date, tags, category, template`) is removed. Document-level labels are registered as `doc.title`, `doc.author`, `doc.date`, `doc.tags`, `doc.category`, `doc.template` (per Sub B) with `on_render` hooks emitting YAML frontmatter; `note` is content-level and handled via the generic annotation path.

4. **HTML's current splice-sentinel mechanism migrates to `SpliceState` in the same PR.** The post-DOM string-replacement code in `formats/html/serializer.rs` (around `replace_splice_sentinels`) retires. We end with one splice mechanism, not two.

## Why this is better than the original "typed event" proposal

- `tree_to_events` stays bit-for-bit unchanged. No IR-layer surprise.
- The splice concern is plan-aware, but plan-awareness lives in a dedicated helper, not in the IR/events module.
- The per-format ask is 1-2 lines (the `emit_raw_passthrough` callback) plus ~5 lines of wiring.
- HTML's splice-sentinel mechanism retires in the same step, so we end with one splice mechanism instead of two.
- Future formats (LaTeX, Pandoc) inherit the pattern for free.

## Acceptance criteria

- `tree_to_events` and `crates/lex-babel/src/ir/events.rs` are unchanged.
- `crates/lex-babel/src/common/splice.rs` exists with `SpliceState` helper + tests.
- Markdown serializer integrates `SpliceState`; HACK at `serializer.rs:441-503` is deleted; metadata whitelist removed.
- HTML serializer migrates from splice-sentinels to `SpliceState`; `replace_splice_sentinels` retires.
- A metadata annotation with rich body content (paragraphs, lists, nested annotations) round-trips lex → markdown → lex without loss. Test in `tests/markdown/annotations.rs`.
- A non-metadata annotation with a registered markdown render hook produces handler-defined output in the markdown export.
- A non-metadata annotation *without* a render hook produces the default `<!-- lex:label -->` comment pair (matching today's behavior).
- The `doc.*` schemas registered in Sub B fire correctly during markdown serialization.

## Out of scope

- New event variants in `crates/lex-babel/src/ir/events.rs`. (Earlier draft proposed `Event::FormatNative`; this revision keeps the IR/events module untouched.)
- LaTeX/Pandoc adapters (out of scope per #612).

## Dependencies

Part of work-stream #613. Depends on Sub A (#614), Sub B (#615), Sub C (#616).
