//! Element-specific AST node definitions for the lex format
//!
//!     This module defines all AST element types for the lex format. It serves as the entry point
//!     for understanding how Lex structures content into a hierarchical tree.
//!
//! Element Types
//!
//!     There are four types of elements: blocks, containers, inlines and components.
//!
//!     Components:
//!         Carry a bit of information inside an element, only used in metadata: label and parameters.
//!         See [label](label) and [parameter](parameter) modules.
//!
//!     Inlines:
//!         Specialization of text spans inside text lines. These are handled differently than blocks,
//!         as they are much simpler and do not affect structure nor the surrounding context.
//!         See [inlines](inlines) module.
//!
//!     Blocks:
//!         These are the core elements of Lex, and the ones that users work with. Block elements
//!         are line based, that is they take at least a full line.
//!
//!     Containers:
//!         Containers are a special kind of element that can contain children, and are part of
//!         nestable block elements. See [container](container) module.
//!
//!     Lex's elements are:
//!         - Sessions: have a title and their child content. See [session](session).
//!         - Paragraphs: simple text blocks. See [paragraph](paragraph).
//!         - Lists: have multiple list items, each with marker and optional child content. See [list](list).
//!         - Definitions: have a subject (term) and their content. See [definition](definition).
//!         - Annotations: metadata, have a data tag and optional content. See [annotation](annotation).
//!         - Verbatim Blocks: has a subject, optional content and data tag. See [verbatim](verbatim).
//!
//! Structure, Children, Indentation and the AST
//!
//!     The design for children nodes and the AST has a point that is too easy to miss, and missing
//!     causes a whole lot of problems.
//!
//!     The first key aspect is: indentation is the manifestation of a container node, that is,
//!     where elements hold their children. This is a subtle point, but one worth making.
//!
//!     For example, why in sessions is the title on the same indentation as its sibling nodes, when
//!     its content is indented? Answer: because the title is a child of the session node, and a
//!     session's content is a child of session.content, a container.
//!
//!     Likewise, list elements do not indent, that's why they are shown in the same indentation as
//!     their items and siblings. On nested lists, a list's item content container holds the nested
//!     list, which is why it's indented.
//!
//!     This is true for sessions (titles are outside their children), annotations (data is not its
//!     content), definitions (subject is not its content) and verbatim blocks (subject is not its content).
//!
//!     One can see a pattern here: most elements in Lex have a form:
//!
//!         <preceding-blank-line>?
//!         <head>
//!         <blank-line>?
//!         <indent>
//!             <content>
//!         <dedent>
//!         <tail>?
//!
//!     Seen in this way, it's now clear how one can parse a full level without peeking into the
//!     children, because the container / content is enough to know what to do.
//!
//!     This is to say that save for paragraph, flat lists, and short annotation, all elements use
//!     a combination of head, presence of blank lines, and dedent and the tail to determine what
//!     it's parsing.
//!
//!     Once you factor in the lack of formal syntax, that heads can be regular, list or subject
//!     lines and tails can be data lines or regular lines, and it's clear how this is a delicate
//!     balancing act. All it takes to parse is:
//!         1. Does the head line have list markers, colon, both or neither?
//!         2. Is there a blank line between the head and the content?
//!         3. Is there indented content?
//!         4. Does the tail end with a lex marker?
//!
//!     In short: what form is the head and tail lines, and between is there a blank line and/or content?
//!
//!     For the complete parsing tables showing head/tail/blank-line/indent patterns for each element
//!     type, see the parser grammar documentation and individual element module docs.
//!
//! Special Parsing Cases and Rules
//!
//!     Beyond the basic parsing structure, several element types have special case rules that affect
//!     how they are parsed:
//!
//!     - **Dialog Rule** (Paragraphs): Lines starting with "-" can be formally specified as dialog,
//!       which are treated as paragraphs rather than list items. This provides a way to distinguish
//!       narrative dialog from actual lists.
//!
//!     - **Two Item Minimum** (Lists): A list must have 2 or more items to be recognized as a list.
//!       A single dash-prefixed line will be treated as a paragraph instead. This prevents accidental
//!       list creation.
//!
//!     - **Short Form** (Annotations): Annotations have a shorthand one-liner form for simple metadata:
//!       `:: label params? ::` without requiring content indentation or closing marker.
//!
//!     - **Full Width Form** (Verbatim Blocks): Verbatim content can break normal indentation rules
//!       by starting at column 2 (zero-based column 1), allowing wide content like tables to hug the
//!       left margin. See [verbatim](verbatim) module for details on the indentation wall concept.
//!
//!     - **Multiple Groups** (Verbatim Blocks): A single verbatim element can contain multiple
//!       subject/content pairs, all sharing one closing annotation. This reduces boilerplate for
//!       related code blocks.
//!
//!     - **Termination by Dedent**: All container elements except verbatim blocks are terminated by
//!       a dedent token. You don't explicitly mark where they end; you just know that something else
//!       started. Verbatim blocks are the exception, requiring an explicit closing data marker.
//!
//!     There are a couple of interesting things to note here. The first is that all container
//!     elements, save for Verbatim blocks, are terminated by a dedent. That is, you don't know where
//!     they ended, you just know that something else started.
//!
//!     Sessions are unique in that the head must be enclosed by blank lines. The reason this is
//!     significant is that it makes for a lot of complication in specific scenarios. Consider the
//!     parsing of a session that is the very first element of its parent session. As it's the very
//!     first element, the preceding blank line is part of its parent session. It can see the following
//!     blank line before the paragraph just fine, as it belongs to it. But the first blank line is
//!     out of its reach.
//!
//!     The way this is handled is that we inject a synthetic token that represents the preceding blank
//!     line. This token is not produced by the logos lexer, but is created by the lexing pipeline to
//!     capture context information from parent to children elements so that parsing can be done in a
//!     regular single pass. As expected, this token is not consumed nor becomes a blank line node,
//!     but it's only used to decide on the parsing of the child elements.
//!

pub mod annotation;
pub mod blank_line_group;
pub mod container;
pub mod content_item;
pub mod data;
pub mod definition;
pub mod document;
pub mod inlines;
pub mod label;
pub mod list;
pub mod paragraph;
pub mod parameter;
pub mod sequence_marker;
pub mod session;
pub mod typed_content;
pub mod verbatim;
pub mod verbatim_line;

// Re-export all element types
pub use annotation::Annotation;
pub use blank_line_group::BlankLineGroup;
pub use content_item::ContentItem;
pub use data::Data;
pub use definition::Definition;
pub use document::Document;
pub use label::Label;
pub use list::{List, ListItem};
pub use paragraph::{Paragraph, TextLine};
pub use parameter::Parameter;
pub use sequence_marker::{DecorationStyle, Form, Separator, SequenceMarker};
pub use session::Session;
pub use typed_content::{ContentElement, ListContent, SessionContent, VerbatimContent};
pub use verbatim::Verbatim;
pub use verbatim_line::VerbatimLine;
