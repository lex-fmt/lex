Grammar for lex

    This document describes the formal grammar for the lex language, outlining its syntax and structural rules.

    The grammar is defined using a combination of Backus-Naur Form (BNF) and descriptive text to provide clarity on the language constructs.
	This document covers the core tokens, that is the lower level ones. For the higher level tokens see:
	- Line tokens: [./grammar-line.lex]
	- Inline tokens: [./grammar-inline.lex]

    1. Notation

        Occurrence Indicators:
        - `<element>?` - Optional (0 or 1 occurrence)
        - `<element>*` - Zero or more occurrences
        - `<element>+` - One or more occurrences
        - `<element>{n}` - Exactly n occurrences
        - `<element>{n,m}` - Between n and m occurrences
        - `<element>{n,*}` - n or more occurrences

        Sequence Operators:
        - `A B` - A followed by B
        - `A | B` - A or B (alternatives)
        - `(A B)` - Grouping

        Examples:
            <text-line> = <text-span>+ <line-break>
            <paragraph> = <text-line>+
            <list-marker> = <dash> | <number> <period> | <letter> <period>
            <session-title> = (<number> <period>)? <text-span>+ <line-break>
        :: grammar ::
    1.2. AST Tree Notation

        AST structures are shown using ASCII tree notation:

        Example AST structure:
            ├── session
            │   ├── session-title
            │   └── container
            │       ├── paragraph
            │       └── list
            │           ├── list-item
            │           └── list-item
        :: tree ::
        Conventions:
        - `├──` indicates a child node
        - `│` indicates continuation of parent structure
        - `└──` indicates the last child at a level
        - Indentation shows nesting depth

    1.3. Token Pattern Notation

        Individual token patterns use regular expression-like syntax:

        - `\s` - whitespace character
        - `\n` - line break
        - `[a-z]` - character class
        - `+` - one or more
        - `*` - zero or more
        - `?` - optional
        - `^` - start of line
        - `$` - end of line

        Examples:
            SequenceMarker: ^- \s
            AnnotationMarker: :
            VerbatimStart: .+:\s*$
        :: regex ::
    1.4. Indentation Notation

        Indentation levels are shown with explicit markers:

        Indentation example:
            Base level (column 0)
                +1 indented (column 4)
                    +2 indented (column 8)
        :: indentation ::
        - Each indentation level is exactly 4 spaces
        - `+n` indicates n levels of indentation from base
        - Tabs are converted to 4 spaces during preprocessing

2. Tokens

    Tokens are the atomic units of the lex language, from single characters to complete lines.

    2.1. Character Tokens

        2.1.1. lex-marker

            <lex-marker> = ':' ':'

            The only explicit syntax element in lex, defined by two consecutive colons (::). This marker is used to denote special constructs within the document, such as annotations.

        2.1.2. Whitespace

            <space> = ' '
            <tab> = '\t'
            <whitespace> = <space> | <tab>
            <line-break> = '\n'

        2.1.3. Sequence Markers

            <dash> = '-'
            <period> = '.'
            <open-paren> = '('
            <close-paren> = ')'
            <colon> = ':'

    2.2. Composite Tokens

        2.2.1. Indentation

            <indent-step> = <space>{4} | <tab>
            <indent-token> = <indent-step>
            <dedent-token> = (generated in later transformation phase)
            
            Note: The lexer generates simple indent tokens (one per 4 spaces or tab).
            Semantic indent/dedent tokens are generated in a later transformation step.

        2.2.2. Sequence Decorations

            <plain-marker> = <dash> <space>

            <number> = [0-9]+
            <letter> = [a-zA-Z]
            <roman-numeral> = 'I' | 'II' | 'III' | 'IV' | 'V' | ...

            <separator> = <period> | <close-paren>
            
            <ordered-marker> = (<number> | <letter> | <roman-numeral>) <separator> <space>
            <list-item-marker> = <plain-marker> | <ordered-marker>
            <session-title-marker> = <ordered-marker>
        2.2.3. Subject Markers
            <subject> = <colon>
        2.2.4. Text Spans
            <text-char> = (any character except <line-break>)
            <text-span> = <text-char>+
            <any-character> = (any character including <line-break>)

        2.2.5. Character Classes
            <letter> = [a-zA-Z]
            <digit> = [0-9]

        2.2.6. Quoted and Unquoted Values
            <quoted-char> = '\"' | '\\' | <text-char>  (\" and \\ are escape sequences)
            <quoted-string> = '"' <quoted-char>* '"'
            <unquoted-value> = (<letter> | <digit> | <dash> | <period>)+

        Token Locations

            Every lexer stage returns tokens paired with a byte-range (`start..end`). The range always uses half-open semantics (inclusive start, exclusive end) and points back into the original UTF-8 source. Even synthetic tokens introduced by transformations (Indent, Dedent, BlankLine) are assigned spans that either cover the whitespace they summarize or the boundary where the semantic event occurs.

            Parsers, AST builders, formatters, and tests must provide or preserve these spans. Helper APIs in the codebase therefore require explicit offsets when constructing tokens.

    2.3. Line Tokens

        Line token classification moved to `specs/v1/grammar-line.lex`.
        The dedicated document stays in lockstep with `lex-parser/src/lex/token/line.rs`
        and the classifiers under `lex-parser/src/lex/lexing/`, making it the
        authoritative reference for how logical lines are identified prior to the
        element grammar defined below.


3. Element Grammar

    These are the core elements of lex: annotations, lists, definitions, sessions, verbatim blocks, and paragraphs:

    <data> = <lex-marker> <whitespace> <label> (<whitespace> <parameters>)?
    <annotation> = <data> <annotation-marker> <annotation-tail>?
    <annotation-marker> = "::"
    <label> = <letter> (<letter> | <digit> | "_" | "-" | ".")*
    <parameters> = <parameter> ("," <parameter>)*
    <parameter> = <key> "=" <value>
    <key> = <letter> (<letter> | <digit> | "_" | "-")*
    <value> = <quoted-string> | <unquoted-value>
    <annotation-tail> = <single-line-content> | <block-content>
    <single-line-content> = <whitespace> <text-line>
    <block-content> = <line-break> <indent> (<paragraph> | <list>)+ <dedent> <annotation-marker>

    Note: Annotations have multiple forms:
    - Marker form: :: label :: (no content, no tail)
    - Single-line form: :: label :: inline text (text is the tail)
    - Block form: :: label :: \n <indent>content<dedent> :: (note TWO closing :: markers)
    - Combined: :: label params :: inline text
    Labels are mandatory; parameters are optional.
    Content cannot include sessions or nested annotations.

    <list> = <blank-line> <list-item-line>{2,*}
    <list-in-container> = <list-item-line>{2,*}

    Note: Lists require a preceding blank line for disambiguation at root level. This means:
    - At root level, a list must start after a blank line (or at document start)
    - Inside containers (sessions, definitions), lists do NOT require a preceding blank line
    - Blank lines between list items are NOT allowed (would terminate the list)
    - Single list-item-lines become paragraphs (not lists)

    <definition> = <subject-line> <indent> <definition-content>
    <definition-content> = (<paragraph> | <list> | <definition> | <verbatim-block> | <annotation>)+

    Note: Definitions differ from sessions in one key way:
    - NO blank line between subject and content (immediate indent)
    - Content cannot include sessions (everything else is allowed)
    - Subject line must end with a colon (:)
    - Content CAN include nested definitions (recursive), verbatim blocks, and annotations

    <session> = <session-title-line> <blank-line> <indent> <session-content>
    <session-content> = (<paragraph> | <list> | <session> | <definition> | <verbatim-block> | <annotation>)+

    Notes on separators and ownership:
    - A blank line between the title and the indented content is REQUIRED (disambiguates from definitions).
    - A session may start at document/container start, after a blank-line group, or immediately after a just-closed child (a boundary). Blank lines stay in the container where they appear; dedent boundaries also act as separators for starting the next session sibling.
    - Content can include nested sessions, definitions, verbatim blocks, annotations, lists, and paragraphs.

    <verbatim-block> = <subject-line> <blank-line>? <verbatim-content>? <closing-annotation>
    <subject-line> = <text-span>+ <colon> <line-break>
    <verbatim-content> = <indent> <raw-text-line>+ <dedent>
    <raw-text-line> = <indent>? <any-character>+ <line-break>
    <closing-annotation> = <annotation-marker> <annotation-header> <annotation-marker> <single-line-content>?

    Note: Verbatim blocks have two forms:
    - Block form: subject + blank line (optional) + indented content + closing annotation
    - Marker form: subject + blank line (optional) + closing annotation with optional text (no indented content)
    The "Indentation Wall" rule applies: content must be indented deeper than subject,
    and closing annotation must be at same level as subject.
    The closing annotation can have optional text content after the second :: marker (single-line form).

    <paragraph> = <any-line>+

    <document> = <metadata>? <content>
    <metadata> = (document metadata, non-content information)
    <content> = (<verbatim-block> | <annotation> | <paragraph> | <list> | <definition> | <session>)*

    Parse order: <verbatim-block> | <annotation> | <list-in-container>? | <list> | <definition> | <session> | <paragraph>

4. Implementation Notes: Differences from Formal Specification

    This section documents where the actual implementation in the codebase differs from or clarifies the formal grammar specification.

    4.1. Annotation Elements

        Specification compliance: FULL

        All four annotation forms are correctly implemented:
        - Marker form: :: label :: (empty, no content)
        - Single-line form: :: label :: inline text
        - Block form: :: label :: <newline> <indent>content<dedent> ::
        - Combined form with parameters still requires labels

        Clarification: Earlier revisions allowed parameter-only annotations; the grammar now factors the shared :: label params? portion into <data> so other elements can embed the same payload while keeping labels mandatory.

        Constraint verification: Content cannot include sessions or nested annotations (enforced).

    4.2. List Elements

        Specification compliance: FULL

        List rules:
        - At root level: preceded by a blank line (or at document start)
        - Inside containers (sessions, definitions): no preceding blank line required
        - Must contain at least 2 list items
        - Blank lines between items terminate the list

        Marker support: All marker types are supported:
        - Plain: - (dash with space)
        - Ordered: 1. or 1) (number with period or paren)
        - Letter: a. or a) (single letter with period or paren)
        - Roman: I. or I) (Roman numerals with period or paren)
        - Double-paren: (1) or (a) (number/letter with double parentheses)

        Single items: A single list-item-line (without blank line prefix) becomes a paragraph,
        not a list. This correctly implements "Single list-item-lines become paragraphs".

    4.3. Definition Elements

        Specification compliance: FULL

        Content types: definitions can contain paragraphs, lists, nested definitions
        (recursive), verbatim blocks, and annotations. The only restriction is that
        definitions cannot contain sessions (enforced at both parser and AST level).

        Nested definitions support hierarchical outline structures:
        - Definition > List > Definition > Paragraph is valid
        - Definition > Session is NOT valid

    4.4. Session Elements

        Specification compliance: FULL

        Key distinction from definitions:
        - Sessions REQUIRE a blank line after the title (definitions don't)
        - Sessions CAN contain nested sessions (definitions cannot)
        - Sessions can contain paragraphs, lists, definitions, verbatim blocks, annotations, and other sessions

        Title flexibility: Any text can be a session title (it's just <text-line> or <subject-line>).
        The presence of a blank line after determines if it's a session vs a definition.

    4.5. Verbatim Block Elements

        Specification compliance: FULL

        Closing annotation MUST be present (not optional).
        Content is NOT parsed (preserves raw whitespace/formatting exactly).
        Indentation Wall rule: correctly enforced — content must be indented deeper than subject.

    4.6. Paragraph Elements

        Specification compliance: PARTIAL - implementation is more sophisticated

        What the spec says:
        - <paragraph> = <any-line>+
        - Simple: consecutive non-blank lines form a paragraph

        What implementation adds:
        - Each line is wrapped in a TextLine object (not just raw text)
        - Lines are separated by newline tokens (preserved in structure)
        - This allows formatters to reconstruct exact source spacing

        This is not a functional difference but a structural one that enables
        more accurate source-round-tripping in tools like formatters.

    4.7. Parsing Precedence Order

        The parser attempts matches in this order:
        1. verbatim-block (imperative match — requires closing annotation, tried first)
        2. annotation-block-with-end (block annotation with explicit closing ::)
        3. annotation-block (block annotation without closing marker)
        4. annotation-single (single-line annotation)
        5. list-no-blank (inside containers only — 2+ items, no preceding blank required)
        6. list (at root — requires preceding blank line + 2+ items)
        7. definition (requires subject + immediate indent, no blank line)
        8. session (requires subject + blank line(s) + indent)
        9. paragraph (fallback — catches everything else)
        10. blank-line-group (one or more consecutive blank lines)

        This order is CRITICAL for correct parsing because:
        - Verbatim blocks are the only elements with closing annotations
        - Annotations must be tried before lists (both can start at line beginning)
        - Lists inside containers don't need a blank line; root lists do
        - Definitions vs sessions are distinguished by blank line presence
        - Paragraphs catch any remaining lines
