Definitions

Introduction

	Definitions are a core element for explaining terms and concepts. They pair a subject (the term being defined) with indented explanatory content.

Syntax

	Subject:
		Content here

		Can be multiple paragraphs or any other content but sessions.

	Key rule: NO blank line between subject and content
	(blank line would make it a session instead)

	Subject line:
		- Ends with colon (:)
		- Colon is a marker, not part of the subject text
		- Subject extracted without the colon

	Content:
		- Must be indented immediately after subject
		- Can contain any element but sessions.

Disambiguation from Sessions

	Definitions vs Sessions - the blank line rule:

	The presence/absence of blank line after subject determines which element type.

	Definition syntax (no blank line):
		API Endpoint:
		    A URL that provides access...
	:: code ::
	Session syntax (has blank line):
		API Endpoint:

		    A URL that provides access...
	:: code ::
Content Structure

	Can contain:
		- Multiple paragraphs (separated by blank lines)
		- Lists (2+ items)
		- Nested definitions

	Cannot contain:
		- Sessions (keeps definitions as focused explanatory units)
		- Annotations with block content (annotations can only be markers/single-line)

Block Termination

	Definitions end on:
		- Dedent (back to subject level or less)
		- End of document
		- No closing marker needed

Examples

	Simple definition:
		Cache:
		    Temporary storage for frequently accessed data.

	Multi-paragraph:
		Microservice:
		    An architectural style that structures applications as loosely coupled services.

		    Each service is independently deployable and scalable.

	With list:
		HTTP Methods:
		    - GET: Retrieve resources
		    - POST: Create resources
		    - PUT: Update resources
		    - DELETE: Remove resources

	Nested definitions:
		Authentication:
		    The process of verifying identity.

		    OAuth:
		        An open standard for access delegation.
