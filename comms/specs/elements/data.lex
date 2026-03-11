Data Nodes

Introduction

	Data nodes capture the reusable "annotation header" portion of lex syntax.
	They begin with the :: marker, followed by a mandatory label and optional
	parameters. The trailing closing :: marker and any content belong to the
	element that embeds the data node (annotations today, others in the future).

	Data nodes are not standalone content blocks; they behave like labels or
	parameters — structured metadata that other elements embed.

Syntax

	<data> = <lex-marker> <whitespace> <label> (<whitespace> <parameters>)?

	Example:
		:: note severity=high

	Notes:
	- Labels follow specs/v1/elements/label.lex (mandatory)
	- Parameters follow specs/v1/elements/parameter.lex (optional)
	- Whitespace between components is ignored
	- No closing :: marker is part of the data node

Usage

	Annotations currently embed a data node before their closing :: marker.
	Future elements can reuse data nodes whenever they need the same label +
	parameter payload.

	When combined with annotations the grammar becomes:
		<annotation> = <data> <lex-marker> <annotation-tail>?

	This keeps the existing annotation-start line syntax, but explicitly names the
	data payload for reuse.
