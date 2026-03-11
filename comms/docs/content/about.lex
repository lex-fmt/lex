About

1. About lext

    lex is a plain text document format for structured documents. It combines the immediacy of plain text with the rigor of hierarchical, machine-readable markup so you can stay in the flow from first note to published artifact.

2. The Format

    lex documents rely on indentation and a small set of markers to reveal structure. The syntax stays invisible so authors focus on ideas, not delimiters.


    2.1. Core Elements

        Session:
            Hierarchical sections with numbered titles.

        Example:
            1. Introduction
            
                Content indented under the title belongs to this section.
            
                1.1. Subsection
            
                    Nested content at deeper levels.
        :: text language=lex

        Definition:
            Term:
                The definition follows immediately after the colon, indented one level.
        :: text language=lex

        Lists:
            - Unordered item
            - Another item
            
            1. Ordered item
            2. Another ordered item
                a. Nested alphabetical
                b. Another nested
        :: text language=lex

        Verbatim Blocks:
            Example Code:
                function hello() {
                    return "world";
                }
            :: javascript ::
        Annotation:
            :: note :: This is a single-line annotation.
            
            :: todo status=open ::
                This is a block annotation
                with multiple lines.
            ::
        :: text language=lex

    2.2. Inline Formatting

        - `*bold*` for strong emphasis
        - `_italic_` for emphasis
        - `` `code` `` for inline code
        - `#math#` for mathematical notation
        - `[reference]` for links, citations, footnotes

    2.3. Reference Types

        - `[https://example.com]` - URLs
        - `[@author2024]` - Citations
        - `[42]` or `[^note]` - Footnotes
        - `[#2.1]` - Session references
        - `[./file.txt]` - File references
        - `[TK-placeholder]` - Placeholders

3. Implementation

    The reference implementation lives in Rust crates under the lex-fmt organization. Each crate focuses on a specific role in the toolchain:

    Definition:
        lex-core:
            Parser with a five-phase pipeline.
    Definition:
        lex-babel:
            Format conversion for Markdown, HTML, PDF, and more.
    Definition:
        lex-analysis:
            Document analysis powering editor features.
    Definition:
        lex-lsp:
            Language Server Protocol implementation.
    Definition:
        lex-cli:
            Command-line interface.
    Definition:
        lex-config:
            Configuration loader shared by the CLI and tools.

    All crates are published to [https://crates.io/search?q=lex-].

4. Design Principles

    - *Invisible syntax*: Structure emerges from indentation and textual conventions.
    - *Graceful degradation*: Unmatched constructs become paragraphs instead of errors.
    - *Complete lifecycle*: Scales from quick notes to finished documents.
    - *Tool-friendly*: Deterministic grammar for reliable parsing.
    - *Future-proof*: Plain Unicode text, no proprietary containers.

5. License

    lex is open source. See each repository for license details.
