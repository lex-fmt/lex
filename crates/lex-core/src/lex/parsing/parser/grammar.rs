//! Grammar Pattern Definitions
//!
//!     This module defines the declarative grammar patterns used by the parser. Patterns
//!     are defined as regex rules and are tried in declaration order for correct
//!     disambiguation according to the grammar specification.
//!
//! Markers
//!
//!     Markers are characters or small character sequences that have meaning in the grammar.
//!     There is only one syntax marker, that is a marker that is Lex introduced. All others
//!     are naturally occurring in ordinary text, and with the meaning they already convey.
//!
//!     The Lex marker (::):
//!         In keeping with Lex's ethos of putting content first there is only one formal
//!         syntax element: the lex-marker, a double colon (::). This is used only in
//!         metadata, in Data nodes. See [Data](crate::lex::ast::elements::data::Data).
//!
//!     Sequence Markers (Natural):
//!         Serial elements in Lex like lists and sessions can be decorated by sequence markers.
//!         These vary from plain formatting (dash) to explicit sequencing as in numbers,
//!         letters and roman numerals. These can be separated by periods or parenthesis and
//!         come in short and extended forms:
//!             <sequence-marker> = <plain-marker> | (<ordered-marker><separator>)+
//!         Examples are -, 1., a., a), 1.b.II. and so on.
//!
//!     Subject Markers (Natural):
//!         Some elements take the form of subject and content, as in definitions and verbatim
//!         blocks. The subject is marked by an ending colon (:).
//!
//! Lines
//!
//!     Being line based, all the grammar needs is to have line tokens in order to parse any
//!     level of elements. Only annotations and end of verbatim blocks use data nodes, that
//!     means that pretty much all of Lex needs to be parsed from naturally occurring text
//!     lines, indentation and blank lines.
//!
//!     Since this still is happening in the lexing stage, each line must be tokenized into
//!     one category. In the real world, a line might be more than one possible category.
//!     For example a line might have a sequence marker and a subject marker (for example
//!     "1. Recap:").
//!
//!     For this reason, line tokens can be OR tokens at times, and at other times the order
//!     of line categorization is crucial to getting the right result. While there are only
//!     a few consequential marks in lines (blank, data, subject, list) having them
//!     denormalized is required to have parsing simpler.
//!
//!     The definitive set is the LineType enum (blank, annotation start, data, subject,
//!     list, subject-or-list-item, paragraph, dialog, indent, dedent), and containers are
//!     a separate structural node, not a line token.
//!
//! Grammar Parse Order
//!
//!     Patterns are matched in declaration order for correct disambiguation:
//!         1. verbatim-block - requires closing annotation, tried first for disambiguation
//!         2. annotation_block - block annotation with indented content
//!         3. annotation_single - single-line annotation only
//!         4. list_no_blank - 2+ list items without preceding blank (anywhere)
//!         5. list - preceding blank line + 2+ list items (blank consumed as node)
//!         6. session - requires subject + blank + indent (with context conditions)
//!         7. definition - requires subject + immediate indent
//!         8. paragraph (imperative) - any content-line or sequence thereof, stopping
//!            before list starts (2+ list-like lines) and definition starts
//!            (subject + container). Matched imperatively, not by regex.
//!         9. blank_line_group - one or more consecutive blank lines
//!
//!     This ordering ensures that more specific patterns (like verbatim blocks) are matched
//!     before more general ones (like paragraphs).

use once_cell::sync::Lazy;
use regex::Regex;

/// Lazy-compiled regex for extracting list items from the list group capture.
///
/// This regex identifies individual list items and their optional nested containers
/// within the matched list pattern.
pub(super) static LIST_ITEM_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(<list-line>|<subject-or-list-item-line>)(<container>)?").unwrap());

/// Grammar patterns as regex rules with names and patterns.
///
/// Order matters: patterns are tried in declaration order for correct disambiguation.
/// Each pattern is a tuple of (pattern_name, regex_pattern_string).
///
/// # Pattern Structure
///
/// - Named capture groups (e.g., `(?P<start>...)`) allow extracting specific parts
/// - Token types in angle brackets (e.g., `<annotation-start-line>`) match grammar symbols
/// - `<container>` represents a nested indented block
/// - Quantifiers like `+` (one or more) and `{2,}` (two or more) enforce grammar rules
pub(super) const GRAMMAR_PATTERNS: &[(&str, &str)] = &[
    // Document title: DocumentStart + single title line + blank line(s)
    // Must be tried BEFORE document_start to capture titles
    // The negative lookahead for containers is checked imperatively after matching
    // (Rust regex crate does not support lookahead)
    // Title accepts same line types as session: paragraph, subject, list, or subject-or-list-item
    //
    // Subtitle variant: title line ending with colon (subject-line) + second line + blank lines.
    // Tried first so subtitle form wins over plain title.
    (
        "document_title_with_subtitle",
        r"^<document-start-line>(?P<title><subject-line>|<subject-or-list-item-line>)(?P<subtitle><paragraph-line>|<subject-line>|<list-line>|<subject-or-list-item-line>)(?P<blank>(<blank-line>)+)",
    ),
    (
        "document_title",
        r"^<document-start-line>(?P<title><paragraph-line>|<subject-line>|<list-line>|<subject-or-list-item-line>)(?P<blank>(<blank-line>)+)",
    ),
    // Document start marker: synthetic boundary between metadata and content
    // Only matched when there's no document title (fallback)
    ("document_start", r"^<document-start-line>"),
    // Annotation (multi-line): <annotation-start-line><container>
    (
        "annotation_block",
        r"^(?P<start><annotation-start-line>)(?P<content><container>)",
    ),
    // Annotation (single-line): <annotation-start-line><content>
    ("annotation_single", r"^(?P<start><annotation-start-line>)"),
    // List without preceding blank line (matches anywhere — paragraph lookaheads yield)
    (
        "list_no_blank",
        r"^(?P<items>((<list-line>|<subject-or-list-item-line>)(<container>)?){2,})(?P<trailing_blank><blank-line>)?",
    ),
    // List with preceding blank line (consumes blank lines as part of the match)
    (
        "list",
        r"^(?P<blank>(<blank-line>)+)(?P<items>((<list-line>|<subject-or-list-item-line>)(<container>)?){2,})(?P<trailing_blank><blank-line>)?",
    ),
    // Definition: subject (must end with colon) + immediate indented content
    (
        "definition",
        r"^(?P<subject><subject-line>|<subject-or-list-item-line>)(?P<content><container>)",
    ),
    // Session (requires subject + blank + indented content, allowed at start or after separator)
    (
        "session",
        r"^(?P<subject><paragraph-line>|<subject-line>|<list-line>|<subject-or-list-item-line>)(?P<blank>(<blank-line>)+)(?P<content><container>)",
    ),
    // Paragraph: matched imperatively in GrammarMatcher::match_paragraph()
    // Scans content lines, stopping before element boundaries (list starts, definition starts).
    // Kept here as a comment for grammar documentation; actual matching is in parser.rs.
    //
    // Blank lines: <blank-line-group>
    // Blank lines: <blank-line-group>
    ("blank_line_group", r"^(?P<lines>(<blank-line>)+)"),
];
