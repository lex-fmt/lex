# Issue #615 — Sub B: Unified Registry

**Title:** babel/extensions: unify the two label-dispatch surfaces (IR-build dispatch + render dispatch) into one registry shape

**Filed:** https://github.com/lex-fmt/lex/issues/615

---

Part of the interop architecture work-stream — see umbrella #613.

## Context

Two parallel label-dispatch surfaces exist today:

| Surface | Runs when | Operates on | Produces |
|---|---|---|---|
| **Verbatim/IR-build dispatch** | `from_lex` (IR construction) | lex-core AST `VerbatimBlock` via `from_lex_verbatim` (`crates/lex-babel/src/ir/from_lex.rs:314-397`) | Typed `DocNode` (`Table`, `Image`, `Video`, `Audio`) |
| **Render dispatch** | Pre-serialization | lex-core AST via `crates/lex-babel/src/render_dispatch.rs` | Format-specific output spliced into serializer |

Both gate on the same `Registry` schema. Both are correct in isolation. Together they create three seams:

1. **Extension authors register against two different shapes.** A handler that wants to participate in both IR-hydration and render needs two registrations against two surfaces with two callbacks. The Lex extension story should be: "register one schema, get hydration + rendering."
2. **The split is wrong-axis.** "IR-build vs render" is a lifecycle split. "Verbatim-typed vs free-form" is a content split. The current code splits on the *latter* and uses lifecycle as an accident-of-history. The right axis is lifecycle.
3. **Coupling extensions to parsing/IR-build is fragile.** Verbatim dispatch runs as part of building the IR — close enough to parsing that a buggy or slow handler can corrupt IR construction. Render dispatch runs at serialization time, which is the right phase. Both should land at the right lifecycle phase under a uniform contract.

## Fix sketch

**Single Registry surface** with two lifecycle hook kinds, both registered alongside one schema:

```
LabelSchema {
    name: "lex.tabular.table",
    on_ir_build: Some(table_from_verbatim),   // IR-construction hook
    on_render:   Some(table_to_format),       // serialization hook (per format)
    parameters: [...],
    ...
}
```

- A schema can register zero, one, or both kinds.
- IR-build hooks operate on a parsed-verbatim + parameters input and produce an `IrNode` or a fallback `Verbatim`. They do *not* receive the lex-core AST.
- Render hooks operate on the IR (depends on Sub C migration).
- Schema is the single registration point. Extension authors write one type of handler per kind, against one stable surface.

The existing `Registry` (in `lex-extension-host`) is mostly fine; this issue is about consolidating the *consumer* sides (`from_lex_verbatim` + `render_dispatch.rs`) onto a unified hook-firing pattern with consistent lifecycle naming.

## Naming guidance for the metadata replacement

The hardcoded markdown whitelist (`author, note, title, date, tags, category, template` in `crates/lex-babel/src/formats/markdown/serializer.rs`) gets replaced by registered schemas under this unified surface. Allocation:

- **Document-level metadata** belongs in the reserved `doc.*` namespace: `doc.title`, `doc.author`, `doc.date`, `doc.tags`, `doc.category`, `doc.template`. Register these as built-in `LabelSchema` entries with `on_render` hooks per format:
  - Markdown: emit into YAML frontmatter
  - HTML: emit `<title>`, `<meta name="author">`, `<meta name="keywords">`, etc.
  - (Future) LaTeX: emit `\title{}`, `\author{}`, `\date{}`
- **`note`** is content-level, not document-level metadata. It does **not** migrate to `doc.*`; it stays as a content annotation in its natural namespace (likely the built-in `lex.*` registry or unprefixed). The current code's lumping of `note` with the other six was always a category error.

This naming is what unblocks the whitelist deletion in Sub D.

## Acceptance criteria

- The `Registry` exposes exactly one registration path per schema; lifecycle hooks attach to that schema.
- `from_lex_verbatim` fires the `on_ir_build` hook through the unified surface and never touches lex-core AST internals beyond the verbatim payload.
- `render_dispatch.rs` fires the `on_render` hook through the unified surface (and after Sub C, operates on IR).
- Extension authors writing a new handler follow one pattern, documented in `lex-extension-host/src/lib.rs` doc-comment.
- The `doc.*` schemas (`doc.title`, `doc.author`, `doc.date`, `doc.tags`, `doc.category`, `doc.template`) are registered as built-in labels with per-format `on_render` hooks.
- No path in lex-babel hardcodes a label string and special-cases it; all label-aware behavior goes through the Registry.

## Out of scope

- Implementing new handlers beyond the `doc.*` set (this is a refactor of the dispatch surface).
- Schema versioning / migration story.
- Removing the metadata-label whitelist from markdown serializer — lands in Sub D, where the Registry-driven path replaces it.

## Dependencies

Part of work-stream #613. Depends on Sub A (#614); blocks Sub C (#616), Sub D (#617).
