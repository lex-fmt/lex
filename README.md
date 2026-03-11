# Lex: A Format for Ideas

Lex is a document format for ideas. It's plain text based, with powerful primitives that scale from free-form notes to highly technical content. It's designed to be effortlessly readable and writable, even without any tooling, with metadata as a first class citizen so that higher level automations work on top of it.

It's flexible and permissive so that you can go from free text to more structured elements as your ideas evolve. It's designed as a generic ideas container, one that can scale from a line of free text all the way up to scientific papers and anything in between, being specially useful for technical writing.

## 1\. Ideas, Unbounded

Lex is designed to capture the universal structure of ideas, not as an authoring shortcut to a specific output format. Hence, it provides a precise set of tools for structure, relationships, lists, definitions, annotations, citations, non-verbal content (math), non-textual content, or even textual content in other formats.

Ideas come in many forms, and ideas are made of other ideas. That's why most elements are nestable and can be mixed (except for Paragraphs, which can only contain lines of text, and Sessions, which can only nest inside other Sessions).

Content is addressable, and metadata is a first class citizen, which allows hooks for extensions, be it human interaction or tooling for editing and sharing.

## 2\. Clay, Not Marble

It starts with being simple.

It strives for legibility for humans, while offering a rich set of primitives, being readable and writable without any special tools. It has the lofty goal of being powerful and yet easier to use than alternatives, and it does so by leveraging innate human capabilities such as spatial organization, and by relying on the rich set of conventions and patterns for text that emerged in the last couple of centuries.

The end result is a format which feels effortless, adds the very minimal amount of syntax, and yet is more powerful than Markdown and other alternatives.

## 3\. The Structure

### 3.1. Hierarchical Organization

Lex documents are organized hierarchically, with most elements, starting with Sessions and Lists, being arbitrarily nestable. This makes for a natural way to organize ideas and makes any content addressable in the hierarchy.

### 3.2. Indentation

Indentation encodes structure, the parent-child relationship for all elements, and maps naturally to human spatial organization capabilities. One level of indentation is 4 spaces.

## 4\. Elements

### 4.1. Block Elements

#### 4.1.1. Sessions

Titled containers that can be nested. Optionally presented with ordered markers (1., a., I.) or as plain text. A blank line after the title separates it from its content.

#### 4.1.2. Lists

Like Sessions, also nestable and with flexible presentation. Support unordered (-) and ordered (1., a., I.) markers. Require a minimum of 2 items.

#### 4.1.3. Definitions

Subject-content pairs where the subject ends with a colon and content follows immediately indented, with no blank line in between.

#### 4.1.4. Verbatim

For non-Lex text, such as code snippets or references to binary files like images. Content is preserved exactly as written, with a closing annotation that can carry metadata like language or format.

#### 4.1.5. Annotations

Metadata that can be attached to any element. Not part of the content itself, but notes about the content. Support labels, parameters, and block content.

#### 4.1.6. Paragraphs

The default text element. One or more consecutive lines of text.

### 4.2. Reference System

A core part of ideas is relations, and Lex's reference system is designed to represent a wide variety of them.

#### 4.2.1. URLs

Link to external resources such as web pages or documents. `[https://example.com]`

#### 4.2.2. File Paths

Link to local files such as images, code snippets, or other documents. `[./path/to/file]`

#### 4.2.3. Session References

Sessions can be referenced by their path in the hierarchy. `[#2.1]`

#### 4.2.4. Citations

Reference other works such as scientific papers, books, or articles. `[@doe2024]` Citations support metadata and integrate with citation management systems like Zotero and Mendeley.

#### 4.2.5. Footnotes

Both labeled `[^note]` and numbered `[1]` forms.

### 4.3. Inline Elements

#### 4.3.1. Strong

Bold text with `*asterisks*`.

#### 4.3.2. Emphasis

Italic text with `_underscores_`.

#### 4.3.3. Code

Monospace literals with backticks, for technical terms and inline code.

#### 4.3.4. Math

Mathematical and symbolic notation with `#hashes#`.

## 5\. Batteries Included

The Lex ecosystem includes:

- Specs for the language 
- Core parser and AST (lex-core) 
- Format conversion (lex-babel): markdown, html, pdf, png, pandoc JSON (enabling LaTeX, DOCX, EPUB, RST, and 30+ formats through Pandoc), and RFC XML 
- Inspectors for internal phases, from tokens to final AST, with several visualizations (treeviz, tag, nodemap, JSON) 
- Semantic analysis library (lex-analysis) 
- An LSP server (lex-lsp) for editors, with semantic highlighting, document symbols, formatting, completion, diagnostics, hover, go to definition, references, folding, and document links 
- Editor integrations:  nvim <https://github.com/lex-fmt/nvim>,  the VSCode Extension <https://github.com/lex-fmt/vscode>, and lexed <https://github.com/lex-fmt/lexed>, a standalone desktop editor 

## 6\. Installation

All Rust tools can be installed via cargo.

**Install**

``` shell
cargo install lex-cli
cargo install lex-lsp
```

Each editor plugin has its own installation instructions in its respective repository.

## 7\. Contributing and Contact

- Contributing: contributions are welcome, including code, documentation, and bug reports, via GitHub issues and pull requests. 
- License: MIT 
- Contact: debert@gmail.com. Issues and pull requests are welcome. 
