Why lex?

1. The Universal Format for Ideas

    We know of nothing more powerful than an idea, and no denser medium for it than written language. Yet, our ideas are born into a world of compromise, forced into formats that were never designed to nurture them.

    They begin their lives as scattered notes in plain text files—simple, free, but unstructured and disconnected. As they grow, we move them into word processors, gaining features but trapping them in brittle, presentation-focused formats that crumble under programmatic analysis. For their final, most rigorous form, we wrestle them into the rigid syntax of academic or archival systems, gaining structure at the cost of all creative fluidity.

    Each step is a painful migration. Each format is a cage built for only one stage of an idea's life.

2. A Native Habitat for Ideas

    What if a format could be a habitat, not a cage? A place where an idea could grow organically, from a single seed of thought into a forest of interconnected concepts, without ever needing to be transplanted.

    This ideal format would be:
    - **Simple at the Start**: As easy and immediate as a plain text note
    - **Structured as it Grows**: Able to gain rich, hierarchical structure as the idea develops
    - **Readable by Anyone**: Intuitively understandable in its source form by any human, without special training
    - **Parsable by Anything**: Unambiguously structured for any machine, ensuring reliability
    - **Durable for All Time**: Built on the bedrock of Unicode text, independent of any company, software, or era

3. lex: Ideas, Uncaged

    lex is this habitat. It is a plain text document format designed for the complete lifecycle of an idea. It scales from a fleeting thought to a finished thesis, from a line of code to a novel. It is built on a single, powerful principle: structure should be visible, intuitive, and effortless.


    3.1. The lex Philosophy: Invisible Structure

        **Invisible Structure**: A design philosophy where document structure is defined not by intrusive and complex syntax (like XML tags or LaTeX commands), but by leveraging innate human intuition, primarily through spatial layout (indentation) and a minimal, unambiguous set of markers. The format's syntax becomes "invisible," allowing the author to focus entirely on the ideas themselves.

        lex achieves this by trusting the traditions of written language. We already understand that indented text is subordinate. We know that numbered lines form a sequence. lex formalizes this intuition, making it both human-readable and machine-parsable.


    3.2. From Seed to Forest: The Lifecycle of an Idea

        An idea does not spring into existence fully formed. It grows, branches, and connects. lex is designed to grow with it.


        3.2.1. The Spark

            What if we could represent knowledge structurally without complex syntax?


        3.2.2. The Outline

            - The Problem: Current formats are compromises
            - The Vision: A single format for the whole lifecycle
            - The Solution: lex
                1. Principle: Invisible Structure via indentation
                2. Benefit: Scales from simple to complex


        3.2.3. The Draft

            The core benefit of lex is its ability to scale. An idea can start as a simple sentence, grow into a structured outline, and then be fleshed out with detailed paragraphs, code examples, and even mathematical formulas without ever leaving the same file or learning a new syntax.


        3.2.4. The Paper

            The final form retains this simplicity. Complex elements like code blocks are handled cleanly, integrating directly into the document's flow.

            Python Example:
                def parse(document):
                    """Parses a lex document."""
                    # The five-phase parsing pipeline ensures robustness.
                    tokens = tokenize(document)
                    blocks = group_blocks(tokens)
                    ast = resolve_types(blocks)
                    return process_inlines(ast)
            :: python ::
            This clear, readable structure makes lex a superior authoring format for everything from technical specifications to academic papers.

4. A Manifesto for Your Ideas

    Choosing a format is choosing a future for your ideas. lex is built on a promise:

    **For the Author**: It puts your ideas first. You will spend your time thinking about your content, not fighting with your tools. The structure will flow naturally from your thoughts.

    **For the Machine**: It provides unambiguous, predictable structure. Tools built for lex will be reliable and powerful, because the format is designed from the ground up to be parsed, not just displayed.

    **For the Future**: It is plain text. Your work will be readable in five, fifty, or five hundred years, on any device that can process text. It is a durable archive for human thought, free from proprietary locks.
