Home

1. lex At A Glance

    lex is a plain text habitat for long-lived ideas. It scales from the smallest scratch note to a fully typeset specification without switching tools, formats, or mindsets. The source you are reading is itself lex, rendered into HTML by the CLI released earlier today.[1]

    :: note ::
        Structure is defined by relationships between lines. Indentation, numbering, and annotations are the only signals the parser needs.
    ::

2. Core Philosophy

    Invisible Structure:
        lex trusts spatial layout. Nested sessions such as [#3.2] require no extra syntax beyond indentation and numbering.

    Durable Ideas:
        Documents are UTF-8 text. No binary containers, no proprietary schema—just files that will still open in decades.[^durable]

3. Lex In Practice

    3.1 Outline The Journey

        1. Capture the spark as a single sentence.
        2. Promote it into a structured outline.
        3. Flesh out each branch with paragraphs, code, math, and references.

    3.2 Sample Sessions

        3.2.1 The Cage of Compromise

            We know of nothing more powerful than an idea, and no denser medium for it than written language.

            Key problems with current formats:
                - Plain text is simple but unstructured.
                - Word processors have features but brittle storage.
                - Academic systems enforce structure but sacrifice fluidity.

        3.2.2 A Native Habitat for Ideas

            What if a format could be a habitat, not a cage?

            - Simple at the Start: As easy as a note.
            - Structured as it Grows: Gains hierarchy as ideas develop.
            - Readable by Anyone: No special tooling required.

    CLI Workflow:
        lex generate-lex-css > docs/assets/css/lex-content.css
        lex docs/_lex_src/pages/index.lex --to html --css-path docs/assets/css/lex-content.css -o index.html
    :: shell ::
4. Key Elements

    Definition:
        Invisible Structure:
            The rule that indentation and numbering reveal hierarchy while keeping the syntax invisible.

    Definition:
        References:
            Inline markers like [@specs], [#2], [https://lex.ing], and placeholders [TK-diagram] allow precise linking.

    Definition:
        Rich Blocks:
            Use verbatim blocks for code or ASCII diagrams, annotations for metadata, and lists or definitions for structured prose.

5. Getting Started

    - Visit [./why] for the full manifesto.
    - Read the [./specs/] to explore the grammar.
    - Install the Lex CLI tools at [./tools].
    - Try the editors ([./editors]) for VS Code, Lexed, and Neovim integrations.

6. Footnotes & References

    1. The CLI version 0.2.6 introduced `--css-path` (originally `--extras-css-path`), enabling this page to share styles with the rest of the site.
    ^durable.  Durable storage matters because lex documents often combine research, notes, and publication drafts across years.
    @specs.   The specification lives at https://lex.ing/specs/.
