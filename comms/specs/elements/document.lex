Document Title

    A document title is a single line of text at the very beginning of the document, followed by a blank line. It serves as the human-readable title for the entire file.

    <document-title> = <title-line> <blank-line>
    <title-line> = <text-span> <line-break>

    Rules:
    1. Must be the first element in the document.
    2. Must be a single line.
    3. Must be followed by at least one blank line.
    4. Must not be indented.

    :: lex ::

    Example: Explicit Title
        My Document Title

        Content starts here.
    :: lex ::

    Example: Not a Title (No blank line)
        Not a title
        Because no blank line follows.
    :: lex ::

    Example: Not a Title (Indented)
        Not a title
        
        Because it is indented (this would be a code block or continuation).
    :: lex ::
