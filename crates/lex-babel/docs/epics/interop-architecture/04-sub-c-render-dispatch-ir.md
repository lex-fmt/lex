# Issue #616 — Sub C: render_dispatch on IR

**Title:** babel: migrate render_dispatch from lex-core AST to the IR

**Filed:** <https://github.com/lex-fmt/lex/issues/616>

---

Part of the interop architecture work-stream — see umbrella #613.

## Context

`crates/lex-babel/src/render_dispatch.rs` (~883 lines) is a second walk of the document, operating on `lex_core::lex::ast::Document` *after* `to_ir(doc)` already built one. This breaks the principle stated in `crates/lex-babel/src/lib.rs:60-64`:

> "the heavy lifting is done by a core, well tested and maintained module, freeing format adaptations to be focused on the simpler data format transformations."

Today there are two cores: the IR walk (`tree_to_events` → serializer) and the AST walk (`render_dispatch` → splice plan). Format adapters consume both.

## Why it ended up on the AST

When render-dispatch landed (#524 / #541), the IR didn't yet have everything render hooks needed: full `LabelForm` preservation, complete parameter shape on annotations, document-scope annotation slot. The AST did. The pragmatic choice was to walk the AST.

Now that Sub A symmetrizes the IR (LabelForm is preserved at `crates/lex-babel/src/ir/events.rs` `StartAnnotation.form`; parameters are first-class on the IR `Annotation`; `document_annotations` is a real slot), the IR carries everything render hooks need.

## Fix sketch

`dispatch_render` operates on the IR (or, equivalently, on the event stream from `tree_to_events`):

```rust
pub fn dispatch_render(
    ir: &ir::nodes::Document,
    registry: &Registry,
    format_name: &str,
) -> RenderPlan { ... }
```

- Hook signature changes from `LabelCtx<AstAnnotation>` to `LabelCtx<IrAnnotation>` (or the equivalent IR-side type).
- `RenderOut` and `RenderPlan` types stay the same shape — only the input side moves.
- Existing splice-sentinel mechanism in `formats/html/serializer.rs` continues to work — handlers still produce strings; the serializer still splices them.

## Acceptance criteria

- `render_dispatch.rs` does not import `lex_core::lex::ast`. It operates on `ir::nodes` or `ir::events`.
- `serialize_to_html_with_registry` in `formats/html/serializer.rs` calls `dispatch_render` on the IR, not the AST.
- All existing render-handler tests continue to pass without changes to handler logic (only the input type changes).
- A new test demonstrates a render handler firing from a markdown serializer path (proving format-agnosticism in principle, even before Sub D lands the markdown wiring).
- The file shrinks meaningfully — much of the existing AST-walk plumbing becomes unnecessary once it's reading from the IR.

## Out of scope

- Generalizing the splice mechanism to non-HTML formats (the markdown wiring is Sub D).
- New handler features.

## Dependencies

Part of work-stream #613. Depends on Sub A (#614); blocks Sub D (#617). Can run in parallel with Sub B (#615) after Sub A lands; the ergonomic order is A → B → C → D so that C migrates render-dispatch onto the final unified registry shape rather than the old one.
