### Fixed — paragraph lines split only by indentation no longer merge on format ([#699](https://github.com/lex-fmt/lex/issues/699))

A paragraph whose continuation lines were merely more-indented (alignment / hanging indent) was split into separate sibling paragraphs by the parser, then re-merged into one paragraph after formatting normalized the indent — a silent semantic change across a round-trip. Such hanging-indent continuations now fold back into the paragraph at parse time (real blank-line breaks are preserved).
