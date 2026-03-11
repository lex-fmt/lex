---
layout: default
title: Specifications
---

# lex Specifications

This is the formal reference for the lex document format.

## Quick Start

lex documents are plain text files where structure is defined by indentation and minimal markers.

```
Document Title

1. First Section

    Paragraphs are just text. Multiple lines
    form a single paragraph until a blank line.

    Nested Definition:
        Definitions use a colon after the term,
        with content indented immediately below.

    - List items start with dashes
    - Or numbers: 1. 2. 3.
    - Or letters: a. b. c.

    Code Example:
        function hello() {
            return "world";
        }
    :: javascript ::
2. Second Section

    :: note :: Annotations attach metadata to content.

    References link to things: [https://example.com],
    citations [@author2024], and footnotes [1].
```

## Core Concepts

### Indentation

Indentation defines hierarchy. Each level is 4 spaces (or 1 tab, converted to 4 spaces).

- Content indented under a section title belongs to that section
- Deeper indentation creates nested structure
- Returning to a shallower level closes the current block

### The `::` Marker

The double colon (`::`) is the only explicit syntax in lex. It's used for:

- **Annotations**: `:: label :: content`
- **Verbatim closers**: `:: language`
- **Block annotations**: `:: label ::\n  content\n::`

## Elements

### Sessions

Hierarchical sections with optional numbered titles.

```
1. Main Section

    Content here.

    1.1. Subsection

        Nested content.

    1.2. Another Subsection

        More content.

2. Second Main Section

    And so on.
```

Sessions require a blank line between the title and content.

### Definitions

Term-explanation pairs.

```
Term:
    The definition follows immediately after the colon.
    No blank line between term and content.

Another Term:
    Definitions can contain paragraphs, lists,
    and even nested definitions.
```

Definitions do NOT have a blank line between the subject and content (unlike sessions).

### Lists

Sequences of items with various markers.

```
- Unordered with dashes
- Another item

1. Numbered
2. Items

a. Alphabetical
b. Items

i. Roman
ii. Numerals
```

Lists require a preceding blank line and at least 2 items. A single item becomes a paragraph.

### Paragraphs

Consecutive non-blank lines form a paragraph.

```
This is a paragraph. It can span
multiple lines until a blank line
separates it from the next block.

This is another paragraph.
```

### Verbatim Blocks

Content that should not be parsed as lex.

```
Code Block:
    console.log("Hello, world!");
    const x = 42;
:: javascript ::
Image Reference:
:: image src="diagram.png" alt="Architecture diagram"
```

The closing `::` can include a label and parameters.

### Annotations

Metadata attached to content.

```
:: author :: Jane Doe
:: date :: 2024-01-15

:: note ::
    This is a block annotation
    with multiple lines of content.
::

Content here has the annotation attached.
```

Forms:
- **Marker**: `:: label ::`
- **Single-line**: `:: label :: inline content`
- **Block**: `:: label ::\n  content\n::`

## Inline Formatting

| Syntax | Meaning |
|--------|---------|
| `*text*` | Strong/bold |
| `_text_` | Emphasis/italic |
| `` `text` `` | Code |
| `#text#` | Math |
| `[ref]` | Reference |

### Reference Types

| Pattern | Type | Example |
|---------|------|---------|
| URL | Link | `[https://example.com]` |
| `@key` | Citation | `[@author2024, pp. 42-45]` |
| Number | Footnote | `[1]` |
| `^label` | Named footnote | `[^important]` |
| `#num` | Section ref | `[#2.1]` |
| Path | File ref | `[./data.csv]` |
| `TK-*` | Placeholder | `[TK-figure]` |

## Parsing Order

Elements are matched in this order:

1. Verbatim blocks (require closing annotation)
2. Annotations (start with `::`)
3. Lists (blank line + 2+ items)
4. Definitions (subject + immediate indent)
5. Sessions (subject + blank line + indent)
6. Paragraphs (fallback)

## Grammar Files

The complete formal grammar is maintained in the [comms repository](https://github.com/lex-fmt/comms):

- `specs/grammar-core.lex` - Core tokens and element grammar
- `specs/grammar-line.lex` - Line-level token classification
- `specs/grammar-inline.lex` - Inline formatting and references
