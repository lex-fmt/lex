Line Token Grammar for lex

	This document covers the line tokens. For related token types see:
	- Core tokens (lower level): [./grammar-core.lex]
	- Inline tokens (span-based): [./grammar-inline.lex]

    This document defines the logical line tokens emitted by the lexer transformation stage. It mirrors the LineType enum in the line[1] code

1. Scope & Inputs

	- Source tokens are grouped into logical lines by the line grouping transform.
    - Classification happens in `classify_line_tokens`, followed by a dialog pass in `apply_dialog_detection`.
    - The rules below are ordered; the first match wins. Every non-structural line maps to exactly one of the mutually exclusive types.

2. Structural Line Tokens

    2.1. <indent>

        Generated when an `Indent` token is encountered.
        Carries the original whitespace tokens that produced the indent span.

    2.2. <dedent>

        Generated when a `Dedent` token is encountered.
        Synthetic token that marks a decrease in indentation depth.

3. Classified Textual Lines

    3.1. <blank-line>

        <blank-line> = (<whitespace> | <indentation>)+ <line-break>
        Only whitespace (spaces, tabs, indentation tokens) plus the terminating
        newline. Resets dialog detection state. Blank lines remain in the
        container where they appear; they are not hoisted across indentation
        boundaries.

    3.2. <annotation-end-line>

        <annotation-end-line> = <indent>? <lex-marker> <whitespace>* <line-break>
        Contains only the :: marker (plus optional whitespace). Used to close
        annotation and verbatim blocks.

    3.3. <annotation-start-line>

        <annotation-start-line> =
            <indent>? <lex-marker> <whitespace>
            <label> (<whitespace> <parameters>)?
            <whitespace>? <lex-marker>
            (<whitespace> <text-span>+)? <line-break>
        Follows the same <data> grammar as annotations. Tail content after the
        closing marker stays inline.

    3.4. <data-line>

        <data-line> = <indent>? <lex-marker> <whitespace>
            <label> (<whitespace> <parameters>)? <whitespace>* <line-break>
        Similar to <annotation-start-line> but without a trailing :: marker.
        Used for metadata headers where the payload stops after the label block.

    3.5. <subject-or-list-item-line>

        <subject-or-list-item-line> =
            <indent>? <list-marker> <whitespace> <text-span>+ <colon> <line-break>
        Starts with a list marker and ends with a colon. Parser decides whether it
        behaves like a subject or a list entry based on surrounding context.

    3.6. <list-line>

        <list-line> = <indent>? <list-marker> <whitespace> <text-span>+ <line-break>
        Covers bullet/ordered markers (dash, numbers with period/paren, single
        letters, Roman numerals). Does not end with a colon.

    3.7. <subject-line>

        <subject-line> = <indent>? <text-span>+ <colon> <line-break>
        Any line whose last non-whitespace token is a colon and that was not
        claimed by the previous rules.

    3.8. <paragraph-line>

        <paragraph-line> = <indent>? <text-span>+ <line-break>
        Fallback for non-blank lines that do not match the specialised patterns.

    3.9. <dialog-line>

        Dialog detection runs after the initial classification:
        - Trigger: a <list-line> whose last two non-whitespace tokens are both
          end punctuation (currently periods).
        - Effect: the triggering line and subsequent non-blank lines inherit the
          <dialog-line> type until a blank line resets the dialog state.
        - Purpose: accurately model script-style dialog blocks written as list
          items.

4. Classification Order Reference

    The classifier evaluates the predicates in this sequence:
        1. <blank-line>
        2. <annotation-end-line>
        3. <annotation-start-line>
        4. <data-line>
        5. <subject-or-list-item-line>
        6. <list-line>
        7. <subject-line>
        8. <paragraph-line>

    Structural tokens (<indent>, <dedent>) are emitted directly by the grouping
    pass and bypass the ordered checks above. <dialog-line> is a mutation step on
    top of the initial result.

5. Line Families

    These aliases help parser combinators express higher-level expectations:

        <blank-line-group> = (<blank-line>)+
        <annotation-line> = <annotation-start-line> | <annotation-end-line>
        <content-line> = any classified line excluding annotation-start/end
        <any-line> = any non-blank line
        <all-line> = any line, including structural tokens


Notes: 

1. lex-parser/src/lex/token/line.rs
