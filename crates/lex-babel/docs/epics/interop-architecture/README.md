# Interop Architecture Work-Stream

This directory is the design-doc home for the lex-babel interop architecture cleanup. The work was scoped after the v1 spec review (May 2026) and is filed as GitHub issues #613–#617.

Each `*.md` file in this directory mirrors the corresponding issue body, so reviewers can comment line-by-line on the design before/while the implementation lands.

## Issues

| # | File | Title |
|---|---|---|
| #612 | _(forthcoming as part of #612)_ | v1 interop scope — Markdown/HTML/PDF-out as the bar; Pandoc/LaTeX/HTML-import as planned; PDF-import is a category error |
| #613 | [`01-umbrella.md`](./01-umbrella.md) | Umbrella: symmetrize the IR, unify label dispatch, retire the markdown HACK |
| #614 | [`02-sub-a-symmetric-ir.md`](./02-sub-a-symmetric-ir.md) | Sub A: complete IR ↔ Lex symmetry |
| #615 | [`03-sub-b-unified-registry.md`](./03-sub-b-unified-registry.md) | Sub B: unify the two label-dispatch surfaces |
| #616 | [`04-sub-c-render-dispatch-ir.md`](./04-sub-c-render-dispatch-ir.md) | Sub C: migrate render_dispatch from AST to IR |
| #617 | [`05-sub-d-markdown-hack.md`](./05-sub-d-markdown-hack.md) | Sub D: retire the markdown HACK via typed event |

## Reading order

1. `01-umbrella.md` — the design rationale and why the four sub-issues are one project.
2. `02-sub-a-…` through `05-sub-d-…` — in dependency order. Each sub builds on the previous ones; reading them out of order will obscure why a given decision was made.

## Why these are checked-in files, not just issues

GitHub issues don't get Gemini / Copilot review when filed. A PR with the issue text as committed files lets the AI reviewers comment on the substance line-by-line, alongside any human reviewers. The files stay in the repo as historical record after the work-stream completes.
