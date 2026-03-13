A Gentle Introduction to Lex

    Lex is a plain text format for ideas, that can grow with them from a quick note all the way up to a complex technical document, to say a scientific paper. It's designed to be very easy to read and write, by using many text formatting practices we use daily, while still giving you powerful features for complicated and structured ideas.

1. Structure: Sessions

    We start with parts, that is sessions. You can structure your ideas over sessions, keeping a more manageable structure, that is easier to navigate and refer to.

    Sessions are defined by a session title (like this one, "Structure" above) surrounded by blank lines, and after the latter, some content, which is indented, that is, a tab over there.

    1.1 Subsessions, Nesting and Trees, oh My

        Some sessions are best captured with their own sessions, that is, subsessions. In Lex, you control the structure: sessions can be as deep as you want them to (be kind to your readers, though), have many or few subsessions, as your idea calls for it.

    1.2 Session Titles

        Session titles can be any piece of text, and they can be direct, or sequenced, that is, have some counter for their numbers. These sequence markers come in various styles.

        1.2.a. Numbers

            These are classic numbers like 3. or 4.

        1.2.b. Letters

            These can use letters like a. or b).

        1.2.c. Roman Numerals

            These can use roman numerals like iii. or vi.

        1.2.d. Style Forms

            These can be short (as in 3.) or extended, that is, containing the full sequence of markers including parent ones. This session, in that style, would be 1.2.d or d in compact form.

        1.2.e. Ordering

            Sessions are ordered by their appearance in the document, not by the marker numbering. These are helpers for finding things, and they can be non-contiguous or even repeated, and Lex can order them for you automatically when you save your document.

    1.3 Content

        Sessions can have any content, including other sessions, as long as they have some. Since a session must have some content, at least the innermost one should have a non-session element, like a paragraph.

2. Paragraphs

    So far, we've only seen sessions and paragraphs, the latter being exactly what you'd expect it to be, that is one or more lines of text.
    Blank lines split paragraphs, so this paragraph is two lines long.

    While this one is a single line paragraph.

3. Intermezzo I: Simple Yet Powerful

    It's worth pausing a bit. We've only talked about sessions and paragraphs and it's worth noting a few points.

    Flexibility: ideas are wild little things, caging them into a very square box won't help them grow. And this is why Lex is very flexible, as long as it can give some structure, which helps nurture them too. That is why sessions are arbitrarily (that is: as much as you like) nestable, why they can have any content, why they can have sequence markers of various types and even no markers at all.

    And this gives you addressability: that is, a way to refer (formally or not) to a part of a document, like [#1.2.c] taking you straight to Roman Numerals. Being able to address bits of an idea precisely is quite useful, and surprisingly lacking in alternative formats.

    It's also worth noting the format syntax: that is the rules of how to organize text to specify that X is a Title and Y is a paragraph. Lex strives for readability and ease of use, and minimizes syntax by using common sense things that most of us can understand without formal training. Like blanks separating sessions, tabs for inner content and blank lines splitting paragraphs. This is another principle you will find over and over in Lex.

    Besides flexible structure and forms, Lex also strives to make elements composable, that is, to allow you to mix and match what goes inside what and how. You can almost always put any element inside others, most of the exceptions being about the two elements we've just seen:

    - Sessions: must happen within sessions, you don't want to have a session inside a list or a paragraph, as sessions are the spinal cord of the document.
    - Paragraphs can only contain lines of text.

4. Lists

    A collection of things is a common need in ideas, after all that's a good way to think about the world: your children is the list of your offspring, states in a country, items in a shopping list.

    Lists are made up of at least two items, one after the other, and the first being separated by a blank line from what came before.

    - Miles Davis
    - Dave Brubeck
    - Charles Mingus
    - Thelonious Monk

    Here is a list of some pretty good jazz musicians. Lists, like sessions, can have markers of different styles. The same explicit sequence markers: numbers, letters, romans and plain, that is the good old "-" dash. It's up to you. Plain markers are useful when you don't need to refer to items individually and when ordering is not critical, like in a shopping list.

    And also, like sessions, lists can be nested.

    1. Jazz Musicians
        - Miles Davis
        - Dave Brubeck
        - Charles Mingus
        - Thelonious Monk
    2. Bossa Nova Musicians
        - Antonio Carlos Jobim
        - Joao Gilberto
        - Baden Powell

    Here is a nested list, and one that mixes different styles of markers. And remember, you can have paragraphs inside list items:

    1. Jazz Musicians
        Known for the improvisation and the complex harmonies, jazz has produced some of the most influential musicians in the history of music. Here are some of them:

        - Miles Davis
        - Dave Brubeck
        - Charles Mingus
        - Thelonious Monk
    2. Bossa Nova Musicians
        Known for its smooth and melodic style, Bossa Nova has produced some of the most influential musicians in the history of music. Here are some of them:

        - Antonio Carlos Jobim
        - Joao Gilberto
        - Baden Powell

5. Definitions

    Ideas are about defining things too, and important enough to warrant an element of their own. A definition is made up of the term being defined and its actual definition content.

    Term goes here:
        Definition. Inside the definition element, indented with a +1 level, like a session's content or a child list.

        The definition content can have multiple paragraphs, lists, and even other definitions. They cannot however, have blank lines between the term and the value: that blank line would turn it into a session instead.

6. Intermezzo II: Special Lex Syntax

    We've mentioned before how Lex aims at simplicity and readability, and a big part of that is not introducing syntax elements, that is, characters that are really not part of the content, that feel different from how we'd normally write, that exist solely for the purpose of structuring the document.

    That is correct, but a simplification: blank lines, indented blocks, numbers, dashes and colons are actual characters that are needed for Lex to correctly identify their role. It's just that these are used in a consistent way with common use, hence they feel natural. A more accurate way to put it is that Lex disguises syntax behind common writing practices, making the cognitive load of using it significantly lower, since the prior knowledge makes them somewhat invisible for us.

    That is all true except for one marker, the Lex marker, which is composed of two double colons "::". This only exception is purposefully visible, as it's used to say "this is not regular content", this is special meaning content, a content about the content.

    Hence it's used in verbatim blocks and annotations, the two element types that are not the content itself, but additional information about it.

7. Verbatim

    Awesome as it is, Lex is not the only useful format. There are plenty of content types that require other formats. Verbatim blocks are a way to include those, they say to Lex "This isn't Lex formatted, hands off".

    Verbatim content comes in two flavors: binary and text. Verbatim blocks have a subject (what is being included), a label, with optional parameters [1] and optional textual content for the block. The general form is shown below.

    Alert in the browser:
        alert("Hi mom!");
    :: javascript ::

    The subject:
        A line of text that ends with a colon.
    The textual content:
        (Optional) one or more lines of content, like other elements, indented with a +1 level.
    The label:
        Its type name, enclosed in Lex markers ("::", double colons).
    Parameters:
        Structured extra information, optional [1]. They are written as `key=value` pairs, separated by commas, and placed inside the Lex markers after the label. For example, in the image block below, we have a parameter "src" with the value "angorwat.jpg", which tells us where to find the image file.

    7.1. Textual Content

        For textual content, things like programming code, math formulas and the like, the content can be embedded in the Lex document itself, as its content. Remember that a verbatim block's inner content, like a session's or definition's, is +1 indented.

        Python is very approachable:

            def greet(name):
                print(f"Hello, {name}!")

        :: python ::

        There are some important things to remember:

        - The subject and label are indented at the same level, while the content is indented with a +1 level.
        - The column where the content starts (base level + 1) is called the wall:
            - Anything to the left of it is not part of the content (can only be blank spaces / tabs).
            - Anything to the right is part of the content, even spaces (like the second line's content is "    print(f"Hello, {name}!")" with 4 spaces at the beginning).
        - Blank lines are part of the content, so you can have multiple paragraphs in a verbatim block.

        Though not really a part of Lex itself, most editors will correctly syntax highlight the content as long as the type in the label is of a known format.

    7.2. Binary Content

        This includes things like images, videos, audio and any other such files. Lex, being a text format, can't display them.
        Hence, the binary form holds the subject (what is being included) and the type of data.

        Sunset at Angkor Wat:
        :: image src="angkorwat.jpg" ::

        Note that binary blocks can have textual content as well, and its usefulness can be situation dependent.

        Sunset at Angkor Wat:
            This photo was taken on a Rollei Flex 2.8F, with Kodak Portra 400 film. The colors are stunning, and the details are incredible. I love how the light plays on the ancient stones, creating a magical atmosphere.
        :: image src="angkorwat.jpg" ::

    7.3. Verbatim Groups

        Multiple subject/content pairs can share a single closing annotation. This is handy for step-by-step shell transcripts or grouped code samples that use the same language.

        Install dependencies:
            npm install express
        Start the server:
            node server.js
        :: shell ::

        Each subject anchors to the indentation wall established by the first subject. Content for every pair must be indented past the wall.

    7.4. Labels

        A label [2] is required, but it can be anything that you fancy. There are however, some guidelines that can be useful:
        - For textual formats, well known names like javascript, python, html et al give you syntax highlighting in editors and exports (Markdown, HTML, PDF) for free.
        - Image, video, audio and media are built-in types, so using those labels can give you better support in editors and exports as well.

8. Tables

    Tables are incredibly useful for organizing data in a structured way. For now, Lex uses a verbatim block with the `table` label.

    Some Jazz Records:
        | Name         | Year | Artist         |
        |--------------|------|----------------|
        | Kind of Blue | 1959 | Miles Davis    |
        | Time Out     | 1959 | Dave Brubeck   |
        | Mingus Ah Um | 1959 | Charles Mingus |
    :: table ::

9. Annotations

    Annotations are metadata, that is, information about the content, not the content itself. They are used to add extra information, like comments, explanations, or any other kind of data that is not part of the main content but is still relevant.

    Lex treats annotations as a first class object, that is, they have a key role in Lex. The reason is that this enables systems (editors, other programs, agents) to build on top of Lex. For example, you can build an editing/publishing system on Lex, using annotations as comments, status, etc.

    Together with verbatim blocks, they are the two elements that are not the content itself, and to make that explicit, they have the only Lex-specific syntax, the label (`:: label ::` syntax). This can include an optional parameter section [1], like in verbatim blocks.

    Like other elements, an annotation uses the label as opening, then content is indented. Earlier, we mentioned that elements could contain any other elements, except for sessions and paragraphs. The other exception is annotations, which cannot contain neither sessions nor inner annotations, since metadata about metadata would push us into high metaphysics territory, which sounds pretty scary. But they can still contain other elements: definitions, paragraphs, lists, verbatim blocks and even binary verbatim blocks, which can be useful for things like images that are not part of the main content but still relevant as metadata.

    :: comment ::
        This is an annotation, that is, a comment about the content. It can contain paragraphs, lists, verbatim blocks and even binary verbatim blocks, but it cannot contain sessions or inner annotations.

        It can contain any other element, like a list:

        - Be civil when engaging in discussions, community trumps correctness.
        - Be open to changing your mind, the world is complex and we are all fallible.
    ::

    9.1 Annotation Attachment

        In order to be useful, annotations must have clear targets, to what the annotation is attached to. That allows people, tools and agents to know and have the right context, be it for discussion or a bespoke interface.

        Annotations attach to the element that precedes them. For example, the note below is attached to the list that follows it.

        :: note ::
            This is a note about the list that follows.
        ::

        - Blank lines between annotation and target are optional.
        - Multiple annotations are supported for the same target.
        - If, at that level, no target precedes it, then the annotation is attached to the parent element (the container in which it appears).

    9.2 Short-Form

        For quick notes, Lex supports another form, for annotations whose content is a single line of text, by following the label with the content directly.

        :: author :: Ada Lovelace

        Here instead of having "Ada Lovelace" in an indented, separate line, we have it right after the label. This is a more compact form, that can be useful for quick notes, but it can only be used for single line content, and it doesn't support any other element inside it, since it's all in one line.

10. Intermezzo III: Text Modifiers

    With these elements, we have covered the structural elements, also called block elements (as each defines its own, possibly multi-line, region). We've seen how sessions structure the document, how paragraphs are the basic text element, how lists and definitions can organize content in different ways, and how verbatim blocks and annotations can include content that is not strictly Lex formatted or add metadata to the content.

    The next elements appear intermingled with the text itself, as they modify a word or group of words, hence they are called inlines.

11. Inlines

    There are three types of inlines: formatting, languages and references.

    11.1 Formatting Inlines

        These are inlines that mark their word(s):

        - Strong: use a * to enclose the relevant words, like *important: do not forget!*.
        - Emphasis: use a _ to enclose the relevant words, like _regrettably, I have to say_.
        - Technical Term / Code: use a ` to enclose the relevant words, like `print("Hello, World!")` or `polymorphism`.

        These can be nested: you can have *bold with _emphasis_ inside* or _italic with *bold* inside_. The only restriction is that same-type nesting is not allowed (no bold inside bold).

    11.2 Language Inlines

        Verbatim blocks allow us to include any type of content, math, music, programming languages. But they do require a full block, they don't allow us to, mid sentence, reference a few words, say an equation or a melody.

        11.2.1 Math

            For math, we can use the # marker, with the math content enclosed between # symbols. For example, #x^2 + y^2 = z^2# renders the equation inline with the text. The content inside is treated literally, so you can use any math notation without worrying about Lex interpreting it.

        11.2.2 Music

            [TK-music-inline]

12. References

    The final inline type, references are important enough to warrant a session all to themselves. References link ideas, and, as such, they are, together with structuring (sessions), describing (paragraphs), defining (definitions), listing (lists), annotating (annotations) and including external content (verbatim blocks), one of the core functions of Lex.

    The general form for references, like all inlines, is the marker, in this case [ to start and ] to end, with the reference content in between. References come in several types:

    12.1 URL References

        For linking to web resources. The content starts with `http://`, `https://`, or `mailto:`.

        Check out [https://lex.ing] for more information.
        Send feedback to [mailto:hello@lex.ing].

    12.2 Session References

        For linking to other parts of the same document. The content starts with #, followed by the session marker.

        As we discussed in [#3], simplicity is a core principle.
        See [#1.2.c] for roman numeral markers.

    12.3 File References

        For linking to other files. The content starts with . or /.

        See the full grammar at [./grammar-core.lex].
        Configuration lives at [/etc/lex/config.toml].

    12.4 Citation References

        For academic-style citations. The content starts with @, and supports multiple keys and page locators.

        As argued by [@doe2024], the approach is sound.
        Multiple sources agree [@smith2023; @jones2022].
        See the details in [@author2023, pp. 42-45].

    12.5 Footnote References

        For adding supplementary notes. There are two forms: numbered, using a plain integer, and labeled, starting with ^.

        This claim needs support [1].
        See the caveat [^important-caveat].

        Numbered footnotes link to a corresponding entry in a Notes session at the end of the document (see [#Notes]).

    12.6 TK (To Come) References

        A placeholder for content that is not yet written. TK stands for "to come" and is a common convention in publishing.

        The implementation details are [TK].
        The benchmarks section is [TK-benchmarks].

    12.7 Not Sure References

        When you're not sure what to reference, or want to flag something for later.

        This needs a source [!!!].

    12.8 General References

        Any other text inside brackets becomes a general reference, useful for cross-referencing by name.

        See the [Definitions] section for more.

13. Conclusion

    This is Lex. A format for ideas, where simplicity meets expressiveness. With sessions for structure, paragraphs for prose, lists for collections, definitions for terms, verbatim blocks for foreign content, annotations for metadata, inlines for emphasis and references for linking, you have a complete toolkit for capturing and organizing your thinking, from a quick note to a complex document.

Notes

    1. Parameters are structured metadata written as `key=value` pairs, separated by commas, and placed inside the Lex markers (`::`) after the label. Keys follow the pattern `letter (letter | digit | "_" | "-")*`, so things like `severity`, `ref-id`, or `api_version` are valid. Values can be unquoted (letters, digits, dashes, periods only, like `high` or `3.11`) or quoted (any text, like `"Hello World"` or `"path with spaces"`). Inside quoted values, use `\"` for a literal quote and `\\` for a literal backslash. For example: `:: warning severity=high ::`, `:: image src="angkorwat.jpg" ::`, or `:: author name="Jane Doe" ::`. Parameters are used in verbatim blocks [#7] and annotations [#9] to provide tooling with structured information beyond the label.
    2. Labels follow the pattern `letter (letter | digit | "_" | "-" | ".")*`. Valid labels include `note`, `warning`, `javascript`, `code-example`, `api_endpoint`, and even dotted namespaces like `build.debug` or `lint.ignore`. Labels must start with a letter and cannot contain colons or slashes. Dotted namespaces are useful for tool-specific annotations, letting different systems coexist without collision.
