---
publishing-date:  2025/10/12 20:23:23, São Paulo, Brazil
author:  Arthur Debert  
---

# Ideas, Naked

We know of nothing more powerful than ideas, and no denser medium for them than written language. (actually we do, math, but math is too dense).

Specific ideas differ, but there is an ideal structure for ideas, as they are made of the same things: definitions, descriptions, references, comparisons, connections, structure and so on.

And communicating them, regardless of domain, shows recurring themes: segmenting into smaller parts, comparison, mathematics, images, sequencing them, referencing these parts, showing other ideas and so forth.

Given the central role of ideas in human society you'd think we'd have, if not one universally accepted, a handful of well known, well established, and more importantly useful document formats for ideas. And yet, we don't, not exactly.

## 1\. Formatting Ideas

HTML has proven to be a very rich structure for ideas of all kinds, but it is very tied to browser, presentation and other publishing legacies in one way, and too poor conceptually in others (flat structure, sections are lists). Wikimedia's format is a good candidate, but also too domain and legacy driven.  

The best candidates we have from this tend to be SGML descendants in a variety of XML formats, normally quite domain specific. While very capable and widely used, these serve as the backend part, while content is authored somewhere else (LaTeX, Doc, HTML), that is they are the final stage of an automated pipeline.

**GML/SGML**:

IBM's 1960s Generalized Markup Language work by Charles Goldfarb, Edward Mosher, and Raymond Lorie. XML established the principle of separating content from presentation—marking up what something is (a title, equation, citation) rather than how it should look. Later on SGML, in the 80s standardized and expanded the idea.

HTML and XML are the children of these, and have proven incredibly successful as general vessels of human knowledge. They have, however, from SGML and parts of XML proven to be too complex and never quite achieved the universal and unequivocal format they hoped to be.

There is an underlying assumption here, that any such formats are not good fits for generating / iterating through ideas, but as a final database-like stop. HTML, probably the most successful under this light has proven way too complex for people to hand edit, and in fact, it rarely is, with the work being done either by bespoke GUIs or markdown (which is great at this)

## 2\. A New Idea for Ideas Old and New

What if we could foster a format for storing ideas that would be great at an idea's very birth: a single line "X sounds interesting, should look into that" or "What if one made X out of Y" but scale too, being able to grow as the idea takes form into a structured, concept rich document.

What if this could be done with a minimal learning curve for non technical people, and in fact requiring only literacy and basic computer skills? And what if this could run on any modern computer, in fact by any computer from the past 40 years, being light and simple? And what if this required no specialized software, allowing people to work from a mobile phone, to Word documents, from shell line editors to sophisticated IDEs?

## 3\. Lex: ideas. naked

Lex is a plain text document format designed for ideas, in general. It can scale down from a single line note to a full engineering spec. It is flexible enough to carry a novel, a love letter, a scientific paper or an internet RFC. It's inclusive, not only by being openly licensed, free, but also by requiring nothing more than processing unicode text files, which any computer from the 1980's onwards can.

### 3.1. Principles

Lex is designed around being:

- Accessible: from a technical point of view, with minimum hardware and software requirements. If a computer can edit a text file, that's all it needs to do.  
- Expressive: a small, but powerful set of primitives that can be combined to express many different types of ideas from unstructured natural text to highly addressable nested sections (i.e. 1.3.4.b).  
- Universal: not designed for a specific presentation (as HTML or WikiText) or a single domain, Lex is about ideas, not presentation, and concepts, not domains.  
- Extensible: the combination of plain text with every element being annotable through metadata, it opens extension points for tooling and workflows. The use of label namespaces can foster a level of specialization and consistency in an automated way.  
- Simple: leveraging innate human skill, such as spatial grouping of similar objects, from indentation to a rich tradition of well established text formatting patterns (such as - This is a dialog\!), Lex has low cognitive load for most people requiring no training to read, and very little training to write.  
- Flexible: while it shines in structured documents, Lex is expressive enough for regular natural language, and the parser and tooling are designed to degrade gracefully for unstructured prose when parsing fails, that is, a regular document is the least you get, no parser errors, no warnings.

### 3.2. The Form in Format

The north star of Lex's syntax is keeping the cognitive load as low as possible when interacting with Lex formatted ideas.

This is achieved by what we call "invisible syntax", that is, one structures text, but in a way where the tokens dedicated to document format are kept to a minimal. This is achieved by:

\- Indentation: leverages human cognition's ability to derive similarity and relationship visually by layout. - The Great Plain Text Tradition: for centuries people have agreed on standard ways to represent text, by using titles, dashes, blank lines and so forth. Lex draws heavily on this tradition. - Minimal syntax: if you consider markdown **bold** and *italics* part of the current lexicon, Lex only adds the Definition:: and :: label constructs, both using the :: marker.

Lex is homomorphic, that is if you copy from a well exported HTML file to a text editor, you get the source form, ready to work (you do loose presentation formating, of course, but not the document's structure).

### 3.3 The Primitives

#### 3.3.1. Nested structure

Lex documents are not flat, as HTML and markdown files are but instead nested. With sections having formal children, siblings and parents. In fact this very section, section 3.3.1 has 3.3 The primitives as its parent, itself a child of section 3. Lex: ideas, naked.

Structure is encoded through indentations, where a section's children (its content) is +1 indented off the section (including its title).

#### 3.3.2 . Paragraphs

The mother of all structures, the most forgiving and flexible one. A sequence of sentences, separated by blank spaces. Paragraphs are too good, too perfectly design in order to need further introduction.

#### 3.3.3. Lists

They are every where, connecting ideas, things, todos and whatever you ideate.

- Form: Lists look like what you though they would.  
- Function: they can nest too.
  1\. And have several decoration styles, from plain to numeral to b. Alphabetical and lest we forget iii. Roman styling.
  Lex lists are powerful and can contain any form of content save for sections.

#### 3.3.4. Verbatim Content

``` javascript

alert("Look Ma, no Lex");
```

Many times this is, as above a piece of text that is formatted in a formal language, like a computer programming language, but at its core it signals that Lex won't process that content. Tools can choose to (as in syntax highlighting code or inlining images), as they please.

``` image
With its epic proportions the tower of Babel is, although a gloomy one, a unequivocal recognition that human communication takes all sorts of forms.
```

#### 3.3.5. Annotations

Annotations represent meta-data, that is, information about the content, not the content itself. Like the author and pub date at this document's top level ones.

<!-- lex:note-editor -->

 Maybe this could be better rephrased? Not sure how annotations are linked to the closes element comes in from this phrase.

<!-- /lex:note-editor -->

<!-- lex:note.author -->

 Done, I'm still keeping it simple, though. They can be attached to any content, and are connected through the annotation position.

<!-- /lex:note.author -->

Annotations are very flexible, and allow tooling, like review and comments in publishing workflows

#### 3.3.6. References

Ideas, without connections are not ideas. That's why Lex includes a rich set of reference targets, for urls <http://example.com> to files [./hi-mom.txt](./hi-mom.txt), sections in the same document [\#3.2](#3.2), placeholders \[TK-betterimage\], citations [@john-2023, p.43](#ref-john-2023,%20p.43) and last but never least footnotes\[1\]

#### 3.3.7. Inlines

These are rendered instructions such as this should be **strong** or *emphasized* and that a word is a `technical term` or math \#$$E=mc^2$$\#.

## 4\. Parting Notes

This document is Lex-formatted itself. In fact, it showcases all of Lex elements and structures. Hopefully it serves as an example of how simple and unobtrusive the syntax is, where ideas are centered, with minimal distractions, and where most syntax is leveraged by prior knowledge of the rich plain text tradition most people are exposed to.

## Notes

1\. Citations format hook into established citation management systems as BibTeX and Zotero standards.
