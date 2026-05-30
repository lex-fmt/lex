### Changed — removed the open-form data marker; unrecognized `:: label` lines are kept as text ([#700](https://github.com/lex-fmt/lex/issues/700))

There was never a real "open form" of a data marker. A `:: label` with no closing `::` was classified as a distinct token that no grammar rule consumed, so the parser silently dropped such lines (and a definition whose sole body was one collapsed into a paragraph). Following Lex's rule that anything unrecognized becomes a paragraph — be forgiving, never lose content — these lines now classify as paragraph text.

- **New diagnostic.** An `unclosed-annotation` Warning flags a paragraph line shaped like `:: label …` with no closing `::`, so authors know it looks like metadata but is treated as content. Configurable via `[diagnostics.rules]`.
- `LineType::DataLine` and its classifier are removed. Closed-form `:: label ::` (annotations, verbatim closings) is unchanged.
