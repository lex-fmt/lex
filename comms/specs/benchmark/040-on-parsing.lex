Useful Mental Models when Parsing Lex

    Lex, while remarkably simple and consistent, does differ quite a bit from most parseable languages, and this can make parsing seem much more complicated than it actually is, since Lex's apparent tricky bits are greatly simplified by some of the larger constraints of the language.

1. On Syntax

    Following the ethos of "feeling like regular text, not a formal language" lex masks syntax by matching established conventions on text formatting, not by having special syntax markers.

    1.1 Explicit Syntax Markers

        In fact, lex has only one such marker, the lex marker ("::"), which is used on an annotation marker (:: <label> ::). That is never used for elements, only *meta* elements: ones that are not part of the actual content.

        These are:

        1. Verbatim closers (verbatim is a meta structure designed to say to the parser: this is not lex formatted, ignore it)
        2. Annotations: metadata

        That is to say: lex's goal is to have text indistinguishable from common text formatting sans formal languages, and hence the elements that are explicitly not content need to be clearly marked as such.

    1.2 Indentation and Structure

        Another uncommon feature of lex is that, not only it allows arbitrary nesting of sessions, but it actually allows nested and mixed element children of most elements.

        The rule of thumb is this:

        - Every indentation is a visual cue that a <container> node is inserted.
        - Container nodes are mandatory for mixed-type items.
        - The only non-mixed types are paragraphs (lines), list's list items, and verbatim's verbatim lines.

        That is why paragraphs do not indent, nor do flat lists. But consider:

        1. Grocery
            1.1. Milk
        2. Laundry

        The first list item contains another list. Since heterogeneous content requires a container, the first list item's list element is enclosed in a container, which is indented.

        The AST looks like this:

            <list>
                <list-item>Grocery
                    <container>
                        <list>
                            <list-item>Milk</list-item>
                        </list>
                    </container>
                </list-item>
                <list-item>Laundry</list-item>
            </list>
        :: ast ::

        Likewise, a session is made of the heading, which is not its children, hence not indented. Its content, however, is wrapped in a container, which is indented.

2. On Containers and Allowed Elements

    Lex is designed to be a flexible, rich and powerful format for ideas, which can take many forms. Ideas contain other ideas, and so on, and hence lex is designed to allow (almost) all elements to contain others, with a handful of exceptions:

    1. Paragraphs only contain lines of text
    2. Sessions can only be inserted inside other sessions. Think about it, what would a session inside a list item or an annotation even mean? Sessions are structural, and allowing them to pop up anywhere would break the core structure.
    3. Annotations, which are metadata, cannot contain other annotations. A metadata about metadata is a mind bender, and the semantics, usability and meaning of that is just too unclear, too distant from regular mental models. (Note: the current implementation does allow nested annotations, but this is to be removed in the future.)

    That's it, three very sensible rules; paragraphs contain lines, sessions belong in other sessions, and metadata cannot be squared.

3. Intermezzo: The Invisible Structure

    These simple points, what needs containers, how elements can nest, and that meta elements are the only explicitly marked ones, goes a long way in guiding parsing.

    All that is left is understanding how to tell elements apart.

4. The Common Structure

    All nestable elements are variations of the same structure:

        <preceding-blank-line>?
        <head>
        <blank-line>?
        <indent>
            <content>
        <dedent>
        <tail>?
    :: structure ::

    Here is how these come together:

        | Element    | Prec. Blank | Head                | Blank    | Content  | Tail             |
        |------------|-------------|---------------------|----------|----------|------------------|
        | Session    | Yes         | ParagraphLine       | Yes      | Yes      | dedent           |
        | Definition | Optional    | SubjectLine         | No       | Yes      | dedent           |
        | Verbatim   | Optional    | SubjectLine         | Optional | Optional | dedent + DataLine|
        | Annotation | Optional    | AnnotationStartLine | Yes      | Yes      | AnnotationEnd    |
        | List       | Optional*   | ListLine            | No       | Optional | dedent           |
        | Paragraph  | Optional    | Any Line            | -        | -        | BlankLine/Dedent |
    :: doc.table ::

    *List preceding blank: at root level, lists require a preceding blank line. Inside containers, they do not. See the list spec and grammar patterns list vs list_no_blank for details.

    In short:

    - What distinguishes definitions from verbatim is that definitions require content without an opening content blank line.
    - Sessions can be told apart from lists as lists require 2+ items, not split by blank lines, where session headings must be enclosed in such, then followed by content.

5. Common Gotchas

    5.1 Lists, Decoration Styles and Sessions

        We tend to carry over some pretty particular ideas from Markdown and this contaminates the parsing of Lex. The first point is that the ordering of a list's items has NOTHING to do with the number label for it, consider:

        2. Mom
        1. Dad

        Mom comes first, regardless of the flipped numbers. List items have a decoration style, like plain (-), numbered (1), alphabetical (a), roman (i). This is a preference on how to normalize the decoration. Even more, regardless of numbers on formatting we will correct the sequencing, removing gaps, etc. That also means that, for a given level, a list should use the same style. If a lex string has a list with mixed styles, we will normalize it to the first one.

        That is: decoration markers are a formatting / normalization preference, which Lex will honor on formatting and interop.

        Session headers, not only can have the same markers as lists, but they can have none (a regular line). That is, again, what tells a session apart is a single line of text, enclosed in blank lines, followed by indented content. List items do require one valid decoration though (else they would be indistinguishable from regular lines of text).

        Trying to parse from the decoration style will lead you astray. The other bit: lists require more than one item at the top level:

        - This is not a list

        - While:
            - This is a list
            - With two items

        Note that "- While:" above is not a single-item list (lists require 2+ items). Since it ends with a colon and has indented content, it parses as a definition. The inner three items do form a list (3 items), nested inside that definition's container.

    5.2 Tricky Bits: The Trifecta

        The hardest elements to tell apart are precisely the interplay between sessions, paragraphs and lists, as no clear markers and rules are apparent (but there are).

        For that reason, we call this the trifecta. If you can get mixed and nested versions of them correctly, most of the parsing is done. And to get that right, there are no two ways, you need to first test for lists (2+ items without intervening blank lines), then sessions (heading enclosed in blank lines, followed by indented content), and paragraphs are the fallback, that is, everything that is not a clear list nor session.

    5.3 Verbatim and Definitions

        All these have some common elements, but there is a strict way to disambiguate them.

        1. Verbatim and Definitions both start with a subject line.
        2. Verbatim requires an annotation end marker, at the same indentation level as the subject line, while definitions require a dedent.

        Annotations start with an annotation marker, hence can be confused with a verbatim block end. In practice this is not ambiguous: verbatim blocks are tried first in the grammar, and their imperative matcher identifies the closing annotation at the same indentation as the subject. By the time annotations are tried, any data line that closes a verbatim has already been consumed.

    5.4 Verbatim Blocks

        Verbatim blocks are by far the most complicated element to parse. Let's go over some ideas that will make this easier.

        5.4.1 No Need for Inner Content

            The concept of verbatim is that the content is not lex formatted, and hence the parser should ignore it. This includes textual content (like a Python code snippet) but also binary content like an image (which we have the path for, but do not embed the bytes).

            This is the first heads-up: verbatim blocks can forgo content entirely. This is a valid verbatim block:

            An image:
            :: image src=foo.png ::

            "An image:" is the subject line, and `:: image src=foo.png ::` is the closing annotation. There is no inner content. The inner lines are optional!

        5.4.2 Isolating Inner Text Actual Characters

            As we've mentioned, we do not want to parse the inner text of verbatim blocks. However, because of the indentation rules, we need to tell apart what is the content *sans* the enclosed block's indentation characters.

            A Python Function:
                def foo():
                    print("Hello World")
            :: python ::

            The inner content has to be +1 indented, that is, its characters start at the "d" in def, that is called the indentation wall. All content starts there: it's illegal to start before that. It can perfectly start after that, as in the print line, in which case the actual content is "    print("Hello World")", that is 4 spaces then print. Lines blank or otherwise are part of the inner block.

        5.4.3 Final Trick: Stretched Mode

            The last tricky bit: since some verbatim content makes use of wide text (i.e. markdown tables), and indentation can, from nesting, consume many characters, there is a special form, in which the indentation wall is arbitrarily at 2 chars, regardless of the indentation.

            Some Table:
  | Header A | Header B |
  |----------|----------|
  | Cell A   | Cell B   |
            :: table ::

            The use of 2 is to precisely make clear that no valid indentation is possible but stretched, and this is detected on parsing the first non-blank content line (see the verbatim spec for details).

    5.5 Annotation Attachment

        While not strictly required for parsing, annotation attachment is what makes lex's first-class metadata genuinely useful for tooling. Without it, annotations are just free-floating markers; with it, every annotation has a clear target element.

        The attachment rules are:

        1. An annotation attaches to the closest content element, measured by blank lines separating them.
        2. On equal distance, the next element wins over the previous.
        3. Annotations at document start followed by a blank line attach to the Document itself.
        4. When an annotation is the last element in a container, the container becomes the "next" element for distance comparison.

        This is handled as a post-parsing assembly stage, not during grammar matching. See assembling/stages/attach_annotations.rs for the full logic and distance calculation.

    5.6 Document Title vs Session Title

        A subtle but important distinction: the document title and session titles look identical in the source text but are parsed differently.

        A session requires: heading + blank line + indented content. A document title is the first paragraph of the document followed by blank lines but *not* followed by indented content. That negative lookahead is what distinguishes them: if there is a container after the blank, it is a session; if not, it is the document title.

        The grammar tries the document_title pattern before session, and uses a synthetic DocumentStart token (injected by the lexing pipeline) to mark where document content begins. The title is then promoted into the root session's title field during AST assembly. See grammar.rs (document_title pattern) and building/ast_tree.rs (extract_document_title).
