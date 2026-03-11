Feature: Inlines

	Most elements in lex are block elements, that is ones that take a full line. The only exceptions to these are the fixed labels and parameters, that is two very clearly marked elements, surrounded by spaces and easy to parse. That is to say, that Lex is mainly a line based language.

	In this spec, we introduce Inlines, a new kind of element, Inline elements. These are , not only span (that is not line ) based, but they can start and end at arbitrary positions in a group of lines. 

1. Inlines

	At it's core a Inline is an element that : 

	- Has a clear start and end marker.
	- Surrounds a piece of text.
	- Is always contained, that is , in no way it can break it's parent element boundaries

	The are the well known inlines from markdown (except no double tokens, just one): 

	- *bold*  which is strong.
	- _italic_ which is emphasized.
	-  `code`  which is monospaced.

2. Syntax

    And just like markdown, there can't be a space between the token and the text. So the general form is: 

		<token><alpha-numeric-string><string></end token>

	And they can be nested but not crossed.  The start and end tokens can be different (more on this later)

2. Context

	Inlines can be used in any text context in Lex. They can be used inside paragraphs, lists, definitions, sessions, verbatim blocks, and annotations.

	Inlines are very distinct from the block elements in which they do not depend on the outside context. For this reason, they are parelelizable: if a document has 40 text containers each one can be parsed in parallel with no coordination of any kind.

	For the same reason inlines are much easier to test, no indentation no document level structures or state.
    That is why we waited until all the complex parsing was in place to tackle inlines, as they are simple and stand alone. 

4. Implementation Guidelines

	We will over simplify this to get the point across, the final implementation will be more complex. Say that you have the following string: "Welcome **kiddo** to the party". when parsed, you will end up with something like this: "welcome <strong>kiddo</strong> to the party". While this works , now the ast is heterogenous, as it contains both strong and text nodes.

	For this reason, to end up with a uniform ast, all text content in Lex is wrapped in a TextContent [src/lex/ast/text_content.rs] node. If no inline is found for a string, it's the Identity tranformation, that is, the text content is returned as is. So the first point is that inlines must be implemented in a way that they return a TextContent node.

	The second point is that inlines are incredibly repetitive. The common form (as shown above) in fact can be parametrized by defining the start, end token, the name of the inline. That's it.

	So the implementation wont't be much more complex than a simple loop over the text content, and a conditional to check if the current token is an inline start token. If it is, we parse the inline, otherwise we return the text content as is

	
 