Semantic Token Highlighting

This document demonstrates all semantic token types emitted by lex-lsp and serves as a visual test for editor syntax highlighting. See https://lexlang.org for more information.

1. Highlighting Philosophy

    LexEds use a three-intensity highlighting model that respects user colorschemes:

    Intensity Levels:
        - NORMAL: Theme's foreground color. Used for content text readers focus on.
          Typography (bold/italic) is applied but color comes from the theme.
        - MUTED: Dimmer color for structural elements. Navigation aids that should
          be visible but not prominent. Maps to @punctuation in Neovim,
          punctuation.* scopes in VSCode.
        - FAINT: Faded like comments. Meta-information and syntax markers that
          should recede into the background. Maps to @comment in Neovim,
          comment.* scopes in VSCode.

    In Neovim, users can override `@lex.muted` and `@lex.faint` to customize intensity.
    The `debug_theme` option uses exact colors from lex-light.json for testing.

2. Token Types by Intensity

    2.1. Normal Intensity

        Content text with typography:

        - Session titles are *bold* like this heading
        - Inline *strong text* renders bold
        - Inline _emphasized text_ renders italic
        - Inline `code spans` may have distinct styling
        - Inline #math expressions# render italic

        Definition Subject:
            Definition subjects are italic. The content below is normal text.
            This paragraph is definition content at normal intensity.

        Code blocks have their content at normal intensity:
            function example() {
                return "lex";
            }
        :: javascript ::
    2.2. Muted Intensity

        Structural elements for navigation:

        - List markers (the dash/number) are muted + italic
        - This list item text is also muted + italic
        1. Numbered markers work the same way
        2. They help readers scan document structure

        References use muted color with underline: [^footnote], [@citation], [Cache]

        Cache:
            A definition entry that can be referenced for navigation testing.

        Session markers (1., 1.1., etc.) are muted + italic to separate them
        from the session title text which is bold at normal intensity.

    2.3. Faint Intensity

        Meta-information that should fade into background:

        :: note :: Annotation labels and parameters are faint.
            Annotation content is also faint. This entire block
            should be visually subdued compared to main content.
        ::

        Verbatim block metadata is faint:
            # This content is normal intensity
            echo "hello"
        :: bash language=shell ::

        Inline syntax markers are faintest - the *, _, `, #, [] characters
        that delimit inline formatting should nearly disappear, leaving
        just the formatted text visible.

3. All Token Types Reference

    SessionTitle:
        Full session header line (muted base)
    SessionMarker:
        The 1., 1.1., A. prefix (muted + italic)
    SessionTitleText:
        The title text after marker (normal + bold)
    DefinitionSubject:
        Term being defined (normal + italic)
    DefinitionContent:
        Definition body text (normal)
    ListMarker:
        Bullet or number prefix (muted + italic)
    ListItemText:
        Text after list marker (muted + italic)
    AnnotationLabel:
        The :: label :: part (faint)
    AnnotationParameter:
        Parameters like severity=info (faint)
    AnnotationContent:
        Content inside annotations (faint)
    InlineStrong:
        Text between *markers* (normal + bold)
    InlineEmphasis:
        Text between _markers_ (normal + italic)
    InlineCode:
        Text between `markers` (normal, may link to @markup.raw)
    InlineMath:
        Text between #markers# (normal + italic)
    Reference:
        Cross-references [like this] (muted + underline)
    ReferenceCitation:
        Citations [@like this] (muted + underline)
    ReferenceFootnote:
        Footnotes [^like this] or [1] (muted + underline)
    VerbatimSubject:
        Label before :: in code blocks (faint)
    VerbatimLanguage:
        Language identifier after :: (faint)
    VerbatimAttribute:
        Attributes like language=bash (faint)
    VerbatimContent:
        Code block content (normal, may link to @markup.raw.block)
    InlineMarker_*:
        The delimiter characters themselves (faint + italic)

:: doc.note ::
    This document is used for visual regression testing of syntax highlighting
    across VSCode and Neovim. The lex-light.json theme provides reference colors.
::
