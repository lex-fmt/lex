# Issue #617 — Sub D: Retire Markdown HACK

**Title:** babel/markdown: retire the HACK at serializer.rs:484 — emit typed event from tree_to_events for handler-rendered annotations

**Filed:** https://github.com/lex-fmt/lex/issues/617

---

Part of the interop architecture work-stream — see umbrella #613.

## Context

The HACK in `crates/lex-babel/src/formats/markdown/serializer.rs:441-503` (peak at :484) exists because the markdown layer needs to wrap metadata-annotation body content in HTML comments, but the event stream doesn't let it peek ahead. The author's inline comment identifies the fix:

> "If `tree_to_events` sees a metadata annotation, it could emit a `Event::HtmlBlock` containing the full comment? Instead of `StartAnnotation` / content / `EndAnnotation`. That seems cleaner!"

This is correct. With Sub A (symmetric IR), Sub B (unified registry with `doc.*` schemas), and Sub C (render_dispatch on IR), the fix becomes mechanical:

- A render hook produces the format-rendered string for the annotation.
- `tree_to_events` emits a typed `Event::FormatNative { format, content }` carrying the handler output.
- The markdown serializer treats it as an HTML block; nested content events are skipped (the handler already consumed them).

## Fix sketch

1. Define `Event::FormatNative { format: String, content: String }` in `crates/lex-babel/src/ir/events.rs`.
2. `tree_to_events` (in `crates/lex-babel/src/common/nested_to_flat.rs`) checks each IR `Annotation` against the render dispatch plan (from Sub C). If a handler is registered for the current format, the plan's output is emitted as `FormatNative` and the annotation's children are skipped.
3. `build_comrak_ast` in `formats/markdown/serializer.rs` handles `Event::FormatNative` by emitting `NodeValue::HtmlBlock` with the handler-rendered content.
4. The HACK comment block (`serializer.rs:441-503`) is deleted.
5. The hardcoded metadata-label whitelist (`author, note, title, date, tags, category, template` in the same file) is removed. The document-level labels are now registered as `doc.title`, `doc.author`, `doc.date`, `doc.tags`, `doc.category`, `doc.template` under the reserved `doc.*` namespace (per Sub B), with `on_render` hooks that emit YAML frontmatter; `note` is a content-level annotation and is handled through the generic annotation path.

## Acceptance criteria

- The HACK comment and the associated branch are gone.
- A metadata annotation with rich body content (paragraphs, lists, nested annotations) round-trips lex → markdown → lex without loss. Test in `tests/markdown/annotations.rs`.
- A non-metadata annotation with a registered markdown render hook produces handler-defined output in the markdown export.
- A non-metadata annotation *without* a render hook produces the default `<!-- lex:label -->` comment pair, matching today's behavior for compatibility.
- The hardcoded label whitelist is deleted.
- The `doc.*` schemas registered in Sub B fire correctly during markdown serialization, emitting YAML frontmatter for the document-level metadata.

## Out of scope

- Generalizing `FormatNative` to other formats (HTML uses splice sentinels and doesn't need it; LaTeX/Pandoc are out of scope per #612).

## Dependencies

Part of work-stream #613. Depends on Sub A (#614), Sub B (#615), Sub C (#616).
