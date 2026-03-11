Paragraphs vs Lists Disambiguation Test {{paragraph}}

This document tests the disambiguation between paragraphs and lists, including edge cases. {{paragraph}}

Single item with dash (illegal list, becomes paragraph): {{paragraph}}
- This is not a list {{paragraph}}

Single item with number (illegal list, becomes paragraph): {{paragraph}}
1. This is also not a list {{paragraph}}

Lists require at least two items: {{paragraph}}

- First item {{list-item}}
- Second item {{list-item}}

Paragraph followed by list WITH blank line (required): {{paragraph}}

- This is a list {{list-item}}
- Blank line required before list {{list-item}}

List followed by paragraph without blank line: {{paragraph}}

- Last list item {{list-item}}
- Another list item {{list-item}}

This paragraph follows after blank line {{paragraph}}

Blank lines between list items (illegal, becomes separate paragraphs): {{paragraph}}
- This is not {{paragraph}}

- A list {{paragraph}}

Proper list with blank lines around it: {{paragraph}}

- First proper list item {{list-item}}
- Second proper list item {{list-item}}

Paragraph after proper list. {{paragraph}}

Valid mixed decoration list: {{paragraph}}

- First item {{list-item}}
1. Second item {{list-item}}
a. Third item {{list-item}}
