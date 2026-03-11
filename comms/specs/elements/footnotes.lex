Footnotes

Introduction

	Footnotes are used to provide additional information, citations, or comments without interrupting the main flow of the text.
	In Lex, footnotes are implemented as a combination of inline references and a corresponding list of definitions at the end of the document.

Syntax

	Reference:
		The inline reference is denoted by a number enclosed in square brackets.
		Pattern: `[<integer>]`

	Definition:
		Footnote definitions are collected in a specific "Notes" session at the end of the document.
		The definitions themselves are formatted as a **List**.

		Notes

			1. First footnote content.
			2. Second footnote content.

	Key Requirements:
	1.  **Notes Session**: The definitions MUST be contained within a session, typically named "Notes", located at the end of the document.
	2.  **List Format**: The definitions themselves MUST be formatted as list items.
	3.  **Numbering**: The list item markers (1., 2., etc.) correspond to the inline reference numbers.

Content

	Footnote list items can contain any block element that is valid within a list item, including:
	-   Paragraphs
	-   Nested lists
	-   Verbatim blocks
	-   Definitions

	Example with mixed content:

		Notes

			1. Simple note.
			2. Note with a paragraph.
			    
			    This is the second paragraph of the second note.

			3. Note with code.
			    The code:
			        print("Hello")

Legacy Support (Deprecated)

	Previously, footnotes were sometimes formatted as nested sessions.
	
	Notes

		1. Note Title
		
			Note content.

	This format is deprecated in favor of the cleaner list syntax. Tools may automatically convert legacy formats to the new list format.

Examples

	Standard Footnotes:

		Here is a reference [1] and another [2].

		Notes

			1. This is the first note.
			2. This is the second note.

	Complex Footnotes:

		See the code [1].

		Notes

			1. The implementation details:
				
				:: Rust ::
				fn main() {
				    println!("Footnote code");
				}

