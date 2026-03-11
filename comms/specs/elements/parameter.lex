Parameters

Introduction

	Parameters are a component of annotations (not standalone elements). They provide structured metadata for tooling and are typically removed during publishing. Every parameter list is anchored to a label; standalone parameter headers are invalid.

Syntax

	Parameter format:
		key=value          (unquoted value)
		key="value"        (quoted value - allows spaces)

	Multiple parameters:
		key1=value1, key2=value2

	Separators: comma only (whitespace around parameters is ignored)

Key Syntax

	Pattern: letter (letter | digit | "_" | "-")*
	Valid: severity, ref-id, api_version, type2
	Invalid: 2key, key.name, key:value

Value Syntax

	Unquoted: letters, digits, dashes, periods (no spaces)
		Examples: high, 3.11, item-1

	Quoted: any text (allows spaces, special chars)
		Examples: "Hello World", "value with, comma"

	Escaping in Quoted Values

		Within quoted values, two escape sequences are recognized:
			\" — literal quote character
			\\ — literal backslash

		Other backslashes are literal (no special meaning).

		Examples:
			message="say \"hello\""     (value: say "hello")
			path="C:\\Users\\name"      (value: C:\Users\name)
			note="backslash: \\"        (value: backslash: \)
			raw="hello\nworld"          (value: hello\nworld — \n is NOT an escape)

		Unquoted values have no escaping (restricted character set makes it unnecessary).

Parsing

	Parameters are parsed within the bounded region between :: markers.
	The parser stops at the closing :: to prevent consuming text content.
	Parameters are order-preserving (stored as list, not map).

Examples

	:: note severity=high ::
	:: warning type=critical, id=123 ::
	:: author name="Jane Doe" ::
	:: meta version=3.11 :: (labels remain mandatory; meta carries the identity)
