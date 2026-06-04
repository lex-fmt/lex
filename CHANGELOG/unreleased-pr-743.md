### Changed — smart paste: single-line fresh-line re-anchor + merge first-line strip-to-baseline ([#743](https://github.com/lex-fmt/lex/issues/743))

Two refinements to the `lex/preparePaste` re-anchor transform, from the comms#73 spec review:

- **Single-line paste on a fresh line now re-anchors.** A single clipboard line dropped onto a blank/whitespace-only caret position is a new block, so it is re-anchored to the caret's structural level instead of inserted verbatim (a deep-indented line lifted from a nesting no longer lands over-indented). Single-line pastes that *merge* into existing content still pass through unchanged.
- **Merge-case first line strips only to the clipboard baseline.** Previously the first line of a merge paste had its leading whitespace stripped entirely; if that line was indented deeper than the block baseline, the extra relative indentation was lost. It is now stripped only down to the baseline, preserving `max(0, original_indent - baseline)`.
