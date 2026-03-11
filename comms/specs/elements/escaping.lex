Feature: Escaping

	Lex uses escape sequences to allow special characters to appear as literal text. Escaping
	operates in two distinct contexts: inline content and quoted parameter values. Structural
	elements (indentation, annotations, verbatim blocks) have no character-level escaping.

1. Inline Content Escaping

	Inline content uses backslash (\) as the escape character. The behavior depends on the
	character following the backslash:

	1.1. Rules

		- Backslash before non-alphanumeric character: the backslash is removed and the character
		  is treated as literal text. This prevents inline markup from being triggered.

			\*literal asterisks\*    renders as: *literal asterisks*
			\[not a reference\]      renders as: [not a reference]
			\_not emphasis\_         renders as: _not emphasis_
			\`not code\`             renders as: `not code`
			\#not math\#             renders as: #not math#

		- Backslash before alphanumeric character: the backslash is preserved as literal text.
		  This ensures common patterns like file paths work without double-escaping.

			C:\Users\name            renders as: C:\Users\name
			item\1                   renders as: item\1

		- Double backslash (\\): produces a single literal backslash, since backslash is
		  non-alphanumeric.

			C:\\Users\\name          renders as: C:\Users\name

		- Trailing backslash at end of input: preserved as literal text.

			text\                    renders as: text\

	1.2. Escapable Characters

		The following characters have special meaning in inline content and can be escaped:

		- * (strong/bold marker)
		- _ (emphasis/italic marker)
		- ` (code marker)
		- # (math marker)
		- [ (reference start)
		- ] (reference end)
		- \ (escape character itself)

	1.3. Literal Contexts

		Inside literal inline elements (code, math, reference), backslash escaping does NOT apply.
		All content is preserved verbatim:

			`\*text\*`               produces Code("\*text\*")
			#\alpha + \beta#         produces Math("\alpha + \beta")

		This is essential for code spans (where backslash is a common character) and math
		(where backslash commands like \alpha are standard notation).

2. Quoted Parameter Value Escaping

	Quoted parameter values in annotations use a different, simpler escape system.

	2.1. Rules

		Inside a quoted parameter value (delimited by double quotes), only two escape
		sequences are recognized:

		- \" produces a literal double quote
		- \\ produces a literal backslash

		All other backslash sequences are preserved literally:

			:: note msg="say \"hello\"" ::     value is: say "hello"
			:: note path="C:\\Users" ::        value is: C:\Users
			:: note text="line\nbreak" ::      value is: line\nbreak (backslash preserved)

	2.2. Interaction with Structural Markers

		The :: marker inside a quoted value is NOT treated as a structural delimiter:

			:: note msg="contains :: inside" ::

		The first :: is the annotation opener, the second :: is inside the quoted value
		(not structural), and the third :: closes the annotation.

		Escaped quotes do not toggle quote context:

			:: note msg="value with \" inside" ::

		The \" is an escaped quote, not a closing quote.

3. Structural Elements

	The following elements have NO character-level escaping:

	3.1. Labels

		Annotation labels are plain identifiers with no escape processing. They support
		alphanumeric characters, hyphens, and underscores.

	3.2. Indentation

		Indentation is structural (4-space tabs) and not subject to escaping.

	3.3. Verbatim Blocks

		Content inside verbatim blocks is preserved exactly as written. No escape sequences
		are processed. The only significant characters are the closing :: label :: marker,
		which must match the opening label.

4. Design Rationale

	4.1. Alphanumeric Preservation

		The rule that backslash before alphanumeric is preserved (rather than being an error
		or consuming the backslash) serves a practical purpose: file paths like C:\Users\name
		work naturally without requiring users to double-escape every backslash.

	4.2. Separate Escape Domains

		Inline escaping and quoted value escaping are deliberately separate systems:

		- Inline escaping must handle 7 special characters (the inline markers plus backslash)
		- Quoted values only need to handle 2 characters (quote and backslash)
		- Mixing the two would create confusing interactions

	4.3. No Escaping in Literal Contexts

		Code and math spans do not process escapes because their content is domain-specific:
		code frequently uses backslashes (regex, paths), and math notation relies on backslash
		commands (\alpha, \sum, \frac). Processing escapes would require users to double-escape
		every backslash, defeating the purpose of literal spans.
