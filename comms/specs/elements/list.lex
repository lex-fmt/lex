Lists

Introduction

	Lists organize related items in sequence. They are collections of at least two list items, distinguished from single-item paragraphs.
	List items are ordered according to their position in the list, regardless of marker style. For example: 

	2. First item
	1. Second item

	Confusing as it it, marking the first item with "2." does not make it second in the list. The first item is still the first item, and the second item is still the second item, regardless of the marker used.
	Hence, the list markers are about visual formatting. Likewise, Lex does not error on non-sequential numbering or mixed marker styles within a list. The first item's marker style sets the semantic type of the list, but subsequent items can use different markers without affecting the list's structure.
	The style should be consistent for items in the same list  / level, but can change across different levels of nesting. There fore the style is actually a property of the list , not the items.
	Tools (formmaters, renderers) can fix ordering and consistency issues, but Lex does not enforces them.

Syntax

	Pattern:
		<blank-line>
		- First item
		- Second item
		- Third item

	Key rule: Lists REQUIRE a preceding blank line
	(for disambiguation from paragraphs containing dash-prefixed text)

	Minimum items: 2
	(single dash-prefixed lines are paragraphs, not lists)

List Item Markers

	Plain (unordered):
		- Item text here

	Numbered:
		1. First item
		2. Second item

	Alphabetical:
		a. First item
		b. Second item

	Parenthetical:
		1) First item
		2) Second item
		a) Alphabetical with parens

	Roman numerals:
		I. First item
		II. Second item

Mixing Markers

	List items can mix different marker styles within the same list.
	The first item's style sets the semantic type, but rendering is flexible.

	Example (all treated as single list):
		1. First item
		2. Second item
		a. Third item
		- Fourth item

Compact and Extended Marker forsm

	Since lists can be nested, markers can reference the current item at the current level, or the absolute one (that is, all of it's list ancestors).
	For example: 

	1. First item (absolute: 1.)
	2. Second item (absolute: 2.)
		2.1. Nested first item 
		2.2. Nested second item
			2.2.1. Nested nested item

	The extended form can change different decoration styles per level, as the commmon practise: 

	1. First item (absolute: 1.)
		1.a. Second item (absolute: 1.a)
			1.a.i. Nested first item 
			1.a.ii. Nested second item
		1.b Second item in the same level (absolute: 1.b)

	If a first item is set to extended form, it's list and inner lists will also use the extended form (the root level is too shallow to distinguish the intention of extended vs compact, forms, hence this must happen on the second child level or deeper.

Blank Line Rule

	Lists require a preceding blank line for disambiguation:

	Paragraph (no list):
		Some text
		- This dash is just text, not a list item

	List (has blank line):
		Some text

		- This is a list item
		- Second item

	No blank lines BETWEEN list items:
		- Item one
		- Item two
		
		- This starts a NEW list (blank line terminates previous)

Content

	List items contain text on the same line as the marker.
	Indented content can contain:
		- Paragraphs (multiple paragraphs allowed)
		- Nested lists (list-in-list nesting)
		- Mix of paragraphs and nested lists
	List items CANNOT contain:
		- Sessions (use definitions instead for titled containers)
		- Annotations (inline or block)

Block Termination

	Lists end on:
		- Blank line (creates gap to next element)
		- Dedent (back to parent level)
		- End of document
		- Start of new element at same/lower indent level

Examples

	Simple unordered list:
		- Apples
		- Bananas
		- Oranges

	Numbered list:
		1. First step
		2. Second step
		3. Third step

	Mixed markers:
		1. Introduction
		2. Main content
		a. Subsection A
		b. Subsection B
		3. Conclusion

	Lists in definitions:
		HTTP Methods:
		    - GET: Retrieve resources
		    - POST: Create resources
		    - PUT: Update resources

	Multiple lists in sequence:
		List one:

		- Item A
		- Item B

		List two:

		- Item X
		- Item Y

	List items with nested paragraphs:
		1. Introduction
		    This is a paragraph nested inside the first list item.

		- Key point
		    Supporting details for this key point.

		    Additional context paragraph.

	List items with mixed content:
		- First item
		    Opening paragraph.

		    - Nested list item one
		    - Nested list item two

		    Closing paragraph.

Use Cases

	- Task lists and checklists
	- Enumerated steps or instructions
	- Feature lists
	- Options or choices
	- Bulleted information
	- Ordered sequences
