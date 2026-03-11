:: title :: Formatting Inline Elements Specification
:: author :: Arthur Debert
:: pub-date :: 2025-01-12

Complete specification for formatting inline elements - strong, emphasis, and code elements that provide visual and semantic text markup in lex documents.

:: note :: This specification describes the specific characteristics of formatting inline elements. For the general inline token pattern, universal grammar, parsing architecture, and AST foundation shared by all inline elements, see [inlines-general.lex].

1. Purpose

    Formatting inline elements provide visual emphasis and semantic markup for text content. They follow the general inline token pattern defined in [inlines-general.lex] with specific tokens and content rules. These elements maintain readability in plain text while delivering clear semantic meaning for both human readers and automated processing.

2. Element Types

    All formatting elements follow the `<token>content<token>` pattern from [inlines-general.lex].

    2.1. Strong (Bold)

        Visual and semantic emphasis for important content:
        - Syntax: `*content*`
        - Token: Single asterisk (`*`)
        - Purpose: Strong importance, key concepts, warnings
        - Semantic meaning: High-priority information
        - Visual rendering: Bold text
        - Nesting: Can contain other inline types (except strong)

    2.2. Emphasis (Italic)

        Subtle emphasis and stylistic distinction:
        - Syntax: `_content_`
        - Token: Single underscore (`_`)
        - Purpose: Emphasis, foreign words, titles, definitions
        - Semantic meaning: Stressed or distinguished content
        - Visual rendering: Italic text
        - Nesting: Can contain other inline types (except emphasis)

    2.3. Code

        Technical content and literal text:
        - Syntax: `` `content` ``
        - Token: Single backtick (`` ` ``)
        - Purpose: Code, commands, filenames, technical terms
        - Semantic meaning: Literal or technical content
        - Visual rendering: Monospace font
        - Nesting: No nesting allowed (literal content only)

3. Formatting-Specific Rules

    3.1. Content Processing Differences

        Unlike math and references, formatting elements have varying nesting behavior:
        - **Strong/Emphasis**: Allow nested inline elements (recursive parsing)
        - **Code**: Literal content only (no further parsing)
        - All types follow universal rules from [inlines-general.lex]

    3.2. Token Conflicts

        When multiple formatting tokens appear adjacent:
            *bold*_italic_ # Valid - separate elements
            *bold_mixed*   # Invalid - underscore treated as literal
        :: conflicts

    3.3. Same-Type Nesting Prohibition

        Formatting elements cannot nest within themselves:
            *outer *inner* text* # Breaks at first closing asterisk
            _outer _inner_ text_ # Breaks at first closing underscore
        :: same-type-nesting

4. Grammar Specifics

    4.1. Formatting Grammar

        The authoritative grammar for formatting elements is defined in the main syntax reference.

        Bold text:
            <bold-span> = <asterisk> <text-content> <asterisk>

        Italic text:
            <italic-span> = <underscore> <text-content> <underscore>

        Code text:
            <code-span> = <backtick> <text-content> <backtick>
        
        In each case, the `<text-content>` can contain other inline elements, though it is generally not recommended for `<code>` spans.
        :: grammar

5. Parsing Priority

    Formatting elements in the general parsing order:
    1. Code spans (highest priority - prevents conflicts)
    2. Math expressions
    3. References  
    4. **Strong elements** (asterisk tokens)
    5. **Emphasis elements** (underscore tokens)
    6. Plain text

    Code spans are parsed first to prevent their content from being interpreted as other formatting.


6. Edge Cases Specific to Formatting

    6.1. Adjacent Formatting Elements

        Handling adjacent formatting:
            *bold*_italic_        # Valid - separate elements
            *bold* and _italic_   # Valid - clearly separated
            *bold*and*more*       # Valid - multiple strong elements
        :: lex.core.spec.formatting.edge.adjacent-elements :

    6.2. Formatting Within Code

        Code elements do not process internal formatting:
            `*not bold*` # Asterisks preserved literally
            `_not italic_` # Underscores preserved literally
        :: lex.core.spec.formatting.edge.code-literal-content :
 
