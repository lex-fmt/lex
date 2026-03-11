# Kitchensink

This document includes **all major features** of the lex language to serve as a comprehensive "kitchensink" regression test for the parser, as noted in [@spec2025, pp. 45-46](#ref-spec2025,%20pp.%2045-46). {{paragraph}}

This is a two-lined paragraph. First, a simple *definition* at the root level. {{paragraph}}

**Root Definition**:

This definition contains a paragraph and a `list` to test mixed content at the top level. {{definition}}

- Item 1 in definition referencing \[TK-rootlist\]. {{list-item}}
- Item 2 in definition with note \[42\]. {{list-item}}

This is a marker annotation at the root level, attached to the definition above.

## 1\. Primary Session {{session}}

This session acts as the main container for testing nested structures. It starts with a simple paragraph. {{paragraph}}

<!-- lex:warning severity=high -->

 This is a single-line annotation inside the session.

<!-- /lex:warning -->

- Followed by a simple list. {{list-item}}
- This list has two items. {{list-item}}

### 1.1. Nested Session (Level 2) {{session}}

This is a second-level session containing a definition and a list with nested content. {{paragraph}}

**Nested Definition**:

This definition is inside a nested session and contains a list. {{definition}}

- List inside a nested definition. {{list-item}}
- Second item. {{list-item}}

- A list item at level 2. {{list-item}}
  This list item contains a nested paragraph. {{paragraph}}
  - And a nested list (Level 3). {{list-item}}
  - With its own items. {{list-item}}
- Another list item at level 2. {{list-item}}

A paragraph back at the first level of nesting. {{paragraph}}

``` javascript
// This is a verbatim block with code.
function example() {
return "lex";
}
```

## 2\. Second Root Session {{session}}

This session tests annotations with block content and marker-style verbatim blocks. {{paragraph}}

<!-- lex:todo status="open" assignee="team" -->

This is a block annotation. {{paragraph}}

It contains a paragraph and a list. {{paragraph}}

- Task 1 to complete. {{list-item}}
- Task 2 to complete. {{list-item}}

<!-- /lex:todo -->

``` image
```

Final paragraph at the end of the document. {{paragraph}}
