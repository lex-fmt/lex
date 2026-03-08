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
            <quoted-string> = '"' <text-char>* '"'
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

    Note: Lists require a preceding blank line for disambiguation. This means:
    - A list must start after a blank line (or at document start)
    - Blank lines between list items are NOT allowed (would terminate the list)
    - Single list-item-lines become paragraphs (not lists)

    <definition> = <subject-line> <indent> <definition-content>
    <definition-content> = (<paragraph> | <list>)+

    Note: Definitions differ from sessions in two key ways:
    - NO blank line between subject and content (immediate indent)
    - Content cannot include sessions (only paragraphs and lists)
    - Subject line must end with a colon (:)

    <session> = <session-title-line> <blank-line> <indent> <session-content>
    <session-content> = (<paragraph> | <list> | <session>)+

    Notes on separators and ownership:
    - A blank line between the title and the indented content is REQUIRED (disambiguates from definitions).
    - A session may start at document/container start, after a blank-line group, or immediately after a just-closed child (a boundary). Blank lines stay in the container where they appear; dedent boundaries also act as separators for starting the next session sibling.
    - Content can include nested sessions, definitions, lists, and paragraphs.

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

    Parse order: <verbatim-block> | <annotation> | <list> | <definition> | <session> | <paragraph>

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

        Specification compliance: FULL with clarification

        Implementation detail: While the grammar shows `<blank-line> <list-item-line>{2,*}`,
        the blank line requirement is enforced but could be clearer. A list MUST:
        - Be preceded by a blank line (or start at document beginning)
        - Contain at least 2 list items
        - NOT contain blank lines between items (would terminate the list)

        Marker support: All marker types are supported:
        - Plain: - (dash with space)
        - Ordered: 1. or 1) (number with period or paren)
        - Letter: a. or a) (single letter with period or paren)
        - Roman: I. or I) (Roman numerals with period or paren)

        Single items: A single list-item-line (without blank line prefix) becomes a paragraph,
        not a list. This correctly implements "Single list-item-lines become paragraphs".

    4.3. Definition Elements

        Specification has incomplete description.

        What the spec says:
        - <definition-content> = (<paragraph> | <list>)+
        - NO blank line between subject and content

        What the implementation actually does:
        - Content can include NESTED DEFINITIONS (not mentioned in spec)
        - This is a recursive capability: definitions can contain other definitions
        - Example valid structure: Definition > List > Definition > Paragraph

        This is a significant extension not documented in the formal grammar.
        The intent appears to support hierarchical outline structures.

    4.4. Session Elements

        Specification compliance: FULL

        Key distinction from definitions (correctly specified):
        - Sessions REQUIRE a blank line after the title (definitions don't)
        - Sessions CAN contain nested sessions (definitions cannot)
        - Sessions can contain paragraphs, lists, definitions, and other sessions

        Title flexibility: Any text can be a session title (it's just <text-line> or <subject-line>).
        The presence of a blank line after determines if it's a session vs a definition.

    4.5. Verbatim Block Elements

        Specification compliance: MOSTLY - with one clarification

        What the spec says:
        - Two forms: block form (with indented content) and marker form (no content)
        - Closing data is listed as <closing-data> (reusable data node syntax)

        What implementation clarifies:
        - Closing data MUST be present (not optional)
        - Descriptive text belongs inside the block content, not after the closing data line
        - Content is NOT parsed (preserves raw whitespace/formatting exactly)

        Indentation Wall rule: Correctly enforced - content must be indented deeper than subject.

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
        1. verbatim-block (requires closing annotation - must try first for disambiguation)
        2. annotation (single-line annotations with ::)
        3. list (requires preceding blank line)
        4. definition (requires subject + immediate indent)
        5. session (requires subject + blank line + indent)
        6. paragraph (fallback - catches everything else)

        This order is CRITICAL for correct parsing because:
        - Verbatim blocks are unique (only elements with closing annotation)
        - Lists are distinguished by blank line + multiple items
        - Definitions vs sessions are distinguished by blank line presence
        - Paragraphs catch any remaining lines
