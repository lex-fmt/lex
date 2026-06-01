### Added — reference anchoring in HTML / Markdown serializers (references-general.lex §2.3)

The babel HTML and Markdown serializers now honour Lex's implicit reference anchors instead of always linking a bracketed reference to itself.

- **Inline word anchor (§2.3.1).** A link-like inline reference (`Url` / `File` / `Session` / `General`) wraps its anchored word — the preceding word by default, or the following word when the reference is first on the line — and the bracketed reference no longer renders as literal `[...]` text. `the project website [https://lex.ing] today` → HTML `the project <a href="https://lex.ing">website</a> today`, Markdown `the project [website](https://lex.ing) today`.
- **Whole-element anchor (§2.3.2).** A reference line targeting an element's head line wraps that head line in the link: session title (`<h2><a …>Title</a></h2>`), list item (`<li><a …>Water</a></li>`), definition term and verbatim subject (trailing colon excluded), and a plain paragraph line. The reference line itself emits no separate output.
- **Self-link (§2.3.2).** A reference line with no element directly above renders as a standalone link of its own text, spliced into the document at its source position.
- **Marker-style references unchanged (§2.3.4).** Footnotes `[1]`, citations `[@key]`, and annotation references `[::label]` keep their existing marker rendering and are never given a word or whole-element anchor.

Anchors are read from lex-core's authoritative resolution (`ReferenceInline.word_anchor` and `Document::reference_lines()`); the previous in-babel anchor heuristic (`common/links.rs`) is removed. IR `Verbatim` gains a `subject_href` field carrying the verbatim-subject link through to the serializers.
