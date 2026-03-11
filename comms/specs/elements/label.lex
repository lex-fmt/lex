Labels

Introduction

	Labels are identifiers for annotations. They categorize the annotation type (e.g., "note", "warning") and support namespacing for extensibility.

Syntax

	Pattern: letter (letter | digit | "_" | "-" | ".")*

	Valid examples:
		note
		warning
		code-example
		api_endpoint
		lex.internal
		plugin.myapp.custom

	Invalid examples:
		2note           (cannot start with digit)
		note:warning    (colon not allowed)
		note/type       (slash not allowed)

Namespacing

	Labels support dot notation for namespaces:
		Standard: note, warning, example
		Namespaced: lex.internal, plugin.myapp.custom

	Namespacing allows:
		- Tool-specific annotations (build.debug, lint.ignore)
		- Plugin extensions (myplugin.custom)
		- Avoiding conflicts between tools

Examples

	:: note :: Simple annotation with label only
	:: warning severity=high :: Label with parameters
	:: lex.internal :: Namespaced label
	:: build.output format=html :: Tool-specific namespaced label
