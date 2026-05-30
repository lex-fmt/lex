### Fixed — table column alignment is read from the markdown separator row ([#702](https://github.com/lex-fmt/lex/issues/702))

The separator row's colon hints (`:---` left, `---:` right, `:---:` center) were detected only to be discarded; alignment was sourced solely from the `:: table align=… ::` parameter. Markdown-style aligned tables now keep their alignment across a format round-trip. The explicit `align=` parameter still overrides the separator row.
