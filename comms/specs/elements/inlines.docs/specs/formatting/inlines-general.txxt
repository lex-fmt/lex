:: title :: Inline Elements General Specification
:: author :: Arthur Debert
:: pub-date :: 2025-01-12

Foundation specification for inline elements - the general token-based syntax pattern and parsing architecture shared by all inline content in lex documents.

:: note :: This specification defines the foundational token-based pattern shared by all inline elements. For specific implementations see: [formatting.lex] (strong, emphasis, code), [math.lex] (mathematical expressions), and [references/] (links, citations, footnotes).

1. Purpose

    Inline elements provide rich content formatting and semantic markup within text blocks. This specification defines the fundamental token-based pattern and parsing rules shared by all inline elements. Specific inline element types are detailed in their respective specifications.

2. General Token Form

    2.1. Basic Pattern

        All inline elements follow the fundamental pattern:
            <token>content<token>
        
        Where:
        - Token is typically the same character for start and end
        - No spaces between token and content boundaries
        - Content cannot be empty
        - Token immediately adjacent to content

    2.2. Standard Examples

        Common token patterns:
            *strong text*        # asterisk tokens
            _emphasis text_      # underscore tokens  
            `code text`          # backtick tokens
            #math expression#    # hash tokens
        :: token-examples

        Reference exception (different start/end tokens):
            [reference content]  # square bracket tokens
        :: reference-exception

3. Element Categories

    3.1. Formatting Elements

        Text appearance and semantic emphasis:
        - Strong, Emphasis, Code
        - See [formatting.lex] for complete specification

    3.2. Mathematical Elements

        Mathematical and scientific notation:
        - Math expressions using `#expression#`
        - See the main syntax reference for grammar details.

    3.3. Reference Elements

        Links and cross-references:
        - General references, Citations, Session references, Footnotes
        - See [references/] specifications for complete details

4. Universal Grammar

    4.1. Span Structures

        While all inline elements share a token-based pattern, their specific grammars vary. The authoritative definitions from the syntax reference are:

        Formatting Spans:
            <bold-span> = <asterisk> <text-content> <asterisk>
            <italic-span> = <underscore> <text-content> <underscore>
            <code-span> = <backtick> <text-content> <backtick>
            <math-span> = <hash> <text-content> <hash>

        Reference Spans:
            <reference-span> = <left-bracket> <reference-content> <right-bracket>
            <citation-span> = <left-bracket> <at-sign> <citation-keys> <citation-locator>? <right-bracket>
            <page-ref> = <left-bracket> <page-locator> <right-bracket>
            <session-ref> = <left-bracket> <hash> <session-number> <right-bracket>
            <footnote-ref> = <footnote-naked> | <footnote-labeled>
        :: grammar

    4.2. Content Rules

        Universal content constraints:
        - Content cannot be empty
        - Content cannot span line breaks (single-line only)
        - Content cannot contain block elements
        - Nested inline elements must be different types

    4.3. Nesting Rules

        Valid nesting patterns:
            Valid: *strong with `code` inside*
            Valid: _emphasis with #math# inside_
            Invalid: *strong with *nested strong* inside*
            Invalid: `code with `nested code` inside`
        :: nesting-rules

5. Parsing Architecture

    5.1. Recognition Phase

        Token detection process:
        1. Scan text for known token patterns
        2. Validate token balance (open/close pairs)
        3. Check for non-empty content between tokens
        4. Classify inline element type based on token

    5.2. Parsing Priority

        Element parsing order to resolve conflicts:
        1. Code spans (highest priority - no further parsing)
        2. Math expressions (no further parsing)  
        3. References (validate target format)
        4. Formatting elements (allow nesting)
        5. Plain text (default)

    5.3. Error Recovery

        Malformed inline handling:
        - Unbalanced tokens → Treat as literal text
        - Empty content → Parse error, skip element
        - Invalid nesting → Break at conflict point
        - Unknown token pattern → Preserve as text

6. AST Foundation

    6.1. General Structure

        Base inline AST pattern:
            ├── InlineElement
            │   ├── element_type: InlineType
            │   ├── content: InlineContent
            │   └── tokens: TokenSequence
        :: ast-base

    6.2. Content Variants

        Inline content types:
            ├── InlineContent
            │   ├── Text(String)           # Plain text
            │   ├── Formatted(FormattedContent) # Strong/Emphasis/Code
            │   ├── Math(String)           # Mathematical expressions
            │   └── Reference(ReferenceData)     # References/Citations/Footnotes
        :: content-variants

7. Implementation Notes

    7.1. Token Scanning

        Efficient token recognition:
        - Single pass scanning for all token types
        - Priority-based resolution for conflicts
        - Balanced delimiter validation
        - Context-sensitive parsing

    7.2. Memory Management

        Inline element storage:
        - Share string data for repeated content
        - Maintain source positions for error reporting
        - Efficient nested content representation

8. Escape Sequences

    8.1. General Escaping

        Token escaping rules:
        - Backslash escapes any token character: `\*not bold\*`

9. Integration with Block Elements

    9.1. Container Support

        Inline processing contexts:
        - Paragraphs: Primary container for inline content
        - List items: Rich inline formatting support
        - Definition terms: Inline formatting allowed
        - Annotation content: Full inline support

    9.2. Processing Flow

        Block to inline processing:
        1. Parse block structure first
        2. Process inline content within blocks
        3. Maintain block context for reference resolution
        4. Integrate with document-wide systems
