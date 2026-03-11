Paragraphs

Introduction

	Paragraphs are the fundamental text element in lex. They are the catch-all element - if text doesn't match any other element pattern, it's a paragraph.

Syntax

	Pattern:
		One or more consecutive non-blank lines of text.

	No special markers or syntax required.
	Paragraphs are defined by what they are NOT (other elements).

	Examples:
		This is a paragraph.

		This is another paragraph after a blank line.

		This paragraph spans
		multiple lines without
		a blank line between them.

The Catch-All Rule

	Paragraphs are defined by what they are NOT. If text doesn't match other element patterns, it's a paragraph.
	This makes lex forgiving - ambiguous content defaults to paragraph.

Paragraph Boundaries

	Paragraphs are separated by:
		- Blank lines (one or more)
		- Different element types
		- Dedent (back to parent level)
		- End of document

	Continuous text (no blank lines) forms single paragraph:
		This is line one
		This is line two
		This is still the same paragraph

	Blank line creates new paragraph:
		First paragraph here.

		Second paragraph here.

Content

	Paragraphs contain plain text with inline formatting.
	No nesting of other elements within paragraphs.
	No special parsing of paragraph content (text is text).

Special Cases

	Single list-item-like line (not a list):
		- Just one item here
		(becomes paragraph because lists need 2+ items)

	Dash-prefixed without blank line (not a list):
		Some text
		- This dash is just text
		(becomes paragraph because no preceding blank line)

	Invalid annotations (degrade to paragraphs):
		:: missing closing marker
		(becomes paragraph because annotation syntax incomplete)

	Subject line without indent (not definition):
		Term:
		(becomes paragraph because no indented content follows)

Multi-line Paragraphs

	Lines are joined into single paragraph if no blank line separates them:

		This is a long paragraph that
		spans multiple lines in the source
		but forms a single paragraph element.

	Rendering tools decide wrapping/formatting based on output format.

Examples

	Simple paragraph:
		This is a simple paragraph of text.

	Multi-line paragraph:
		This paragraph contains several lines
		that are all part of the same paragraph
		because there are no blank lines between them.

	Paragraphs separated by blank lines:
		First paragraph here.

		Second paragraph here.

		Third paragraph here.

	In a session:
		Introduction:

		    This paragraph is inside a session.
		    It's indented relative to the session title.

	In a definition:
		Cache:
		    This paragraph explains what a cache is.
		    Multiple lines, same paragraph.

Use Cases

	- Body text and narrative content
	- Explanations and descriptions
	- Default element for ambiguous text
	- Fallback when other element parsing fails
	- Any text that doesn't fit other patterns
