/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

/**
 * Tree-sitter grammar for the Lex document format.
 *
 * The external scanner detects line-level tokens (list markers, annotation
 * markers) because tree-sitter's longest-match lexer rule would otherwise
 * always prefer text_content (/[^\n]+/) over shorter prefixes.
 *
 * Token strategy:
 * - Scanner emits list_marker (just the marker: "- ", "1. ", etc.)
 * - Scanner emits full-line token: subject_content (line ending with :)
 * - Scanner emits annotation_marker (:: prefix) and annotation_end_marker
 * - Scanner emits emphasis delimiters: _strong_open, _strong_close,
 *   _emphasis_open, _emphasis_close (with flanking validation)
 * - Scanner emits _session_break: blank line(s) + indent increase (lookahead)
 * - Grammar lexer emits: text_content (inline-aware), inline tokens
 *   (code_span, math_span, reference, escape_sequence)
 * - INDENT/DEDENT/NEWLINE are always from scanner
 *
 * Session disambiguation:
 *   Sessions and paragraphs share the same prefix (line_content + newline).
 *   Without _session_break, tree-sitter's GLR creates forks at every text
 *   line, and the wrong fork (flat paragraphs) can win. The _session_break
 *   token is emitted by the scanner when a blank line is followed by an
 *   indent increase, eliminating the ambiguity: only confirmed session
 *   boundaries receive _session_break, so paragraphs never compete.
 */
module.exports = grammar({
  name: "lex",

  externals: ($) => [
    $._indent,
    $._dedent,
    $._newline,
    $.annotation_marker, // ":: " at line start
    $.annotation_end_marker, // "::" alone on a line (closing marker)
    $.list_marker, // list marker only: "- ", "1. ", "a) ", etc.
    $.subject_content, // entire line ending with : (scanner verifies EOL)
    $._strong_open, // opening * validated by scanner flanking rules
    $._strong_close, // closing * validated by scanner flanking rules
    $._emphasis_open, // opening _ validated by scanner flanking rules
    $._emphasis_close, // closing _ validated by scanner flanking rules
    $._session_break, // blank line(s) + indent increase (scanner lookahead)
  ],

  extras: (_$) => [],

  conflicts: ($) => [
    // list_marker can start a list_item or line_content (paragraph/session text)
    [$.list_item, $.line_content],
    // blank_line after dedent: part of list_item's trailing blanks or next block
    [$.list_item],
    // subject_content: definition vs verbatim vs line_content (paragraph text)
    [$.verbatim_block, $.definition, $.line_content],
    // after dedent: session done vs verbatim continues with closing annotation
    [$.session, $.verbatim_block],
    // verbatim_block shares structure with definition (no blank lines case)
    [$.verbatim_block, $.definition],
  ],

  rules: {
    document: ($) => repeat($._block),

    _block: ($) =>
      choice(
        $.verbatim_block,
        $.annotation_block,
        $.annotation_single,
        $.definition,
        $.session,
        $.list,
        $.paragraph,
        $.blank_line,
      ),

    // ===== Sessions =====
    // _session_break replaces the old "blank+ indent" sequence. The scanner
    // emits it after confirming blank line(s) followed by increased indent
    // via lookahead. This eliminates the GLR fork between session and
    // paragraph, fixing nested session nesting.
    session: ($) =>
      prec.dynamic(
        1,
        seq(
          field("title", $.line_content),
          $._newline,
          $._session_break,
          repeat1($._block),
          $._dedent,
        ),
      ),

    // ===== Verbatim Blocks =====
    verbatim_block: ($) =>
      prec.dynamic(
        4,
        seq(
          field("subject", $.subject_content),
          $._newline,
          choice(
            // Blank line(s) + indent: scanner emits _session_break
            seq($._session_break, repeat1($._block), $._dedent),
            // No blank line, direct indent (or no content at all)
            seq(
              repeat($.blank_line),
              optional(seq($._indent, repeat1($._block), $._dedent)),
            ),
          ),
          $.annotation_marker,
          $.annotation_header,
          $.annotation_marker,
          $._newline,
        ),
      ),

    // ===== Definitions =====
    definition: ($) =>
      prec.dynamic(
        2,
        seq(
          field("subject", $.subject_content),
          $._newline,
          $._indent,
          repeat1($._block),
          $._dedent,
        ),
      ),

    // ===== Lists =====
    list: ($) =>
      prec.dynamic(3, prec.right(seq($.list_item, repeat1($.list_item)))),

    list_item: ($) =>
      seq(
        $.list_marker,
        optional($.text_content),
        $._newline,
        optional(
          seq(
            $._indent,
            repeat1($._block),
            $._dedent,
            // Trailing blank lines after nested content — these appear
            // between the DEDENT (end of nested blocks) and the next
            // list item at the same level, keeping the list open.
            repeat($.blank_line),
          ),
        ),
      ),

    // ===== Annotations =====
    annotation_block: ($) =>
      seq(
        $.annotation_marker,
        $.annotation_header,
        $.annotation_marker,
        optional(alias($.text_content, $.annotation_inline_text)),
        $._newline,
        $._indent,
        repeat1($._block),
        $._dedent,
        optional(seq($.annotation_end_marker, $._newline)),
      ),

    annotation_single: ($) =>
      seq(
        $.annotation_marker,
        $.annotation_header,
        $.annotation_marker,
        optional(alias($.text_content, $.annotation_inline_text)),
        $._newline,
      ),

    // Annotation header: everything between the :: markers.
    // Allows single colons inside (e.g., :: author: Name ::) but stops
    // before :: (double colon) which the scanner handles as annotation_marker.
    annotation_header: (_$) => /([^:\n]|:[^:\n])+/,

    // ===== Paragraphs =====
    paragraph: ($) => prec.right(-1, repeat1($.text_line)),

    text_line: ($) => seq($.line_content, $._newline),

    line_content: ($) =>
      choice(
        seq($.list_marker, optional($.text_content)),
        $.subject_content,
        $.text_content,
      ),

    // ===== Inline-Aware Text Content =====
    text_content: ($) => repeat1($._inline),

    _inline: ($) =>
      choice(
        $.strong,
        $.emphasis,
        $.code_span,
        $.math_span,
        $.reference,
        $.escape_sequence,
        $._word,
        $._delimiter_char,
      ),

    // ===== Strong and Emphasis =====
    strong: ($) =>
      seq(
        $._strong_open,
        $._word_alnum,
        repeat($._inline_no_star),
        $._strong_close,
      ),

    emphasis: ($) =>
      seq(
        $._emphasis_open,
        $._word_alnum,
        repeat($._inline_no_underscore),
        $._emphasis_close,
      ),

    _inline_no_star: ($) =>
      choice(
        $.emphasis,
        $.code_span,
        $.math_span,
        $.reference,
        $.escape_sequence,
        $._word,
        $._delimiter_char,
      ),

    _inline_no_underscore: ($) =>
      choice(
        $.strong,
        $.code_span,
        $.math_span,
        $.reference,
        $.escape_sequence,
        $._word,
        $._delimiter_char,
      ),

    code_span: (_$) => /`[^`\n]+`/,
    math_span: (_$) => /#[^#\n]+#/,
    reference: (_$) => /\[[^\]\n]+\]/,
    escape_sequence: (_$) => /\\[^a-zA-Z0-9\n]/,

    _word: ($) => choice($._word_alnum, $._word_space, $._word_other),
    _word_alnum: (_$) =>
      token(seq(/[a-zA-Z0-9]+/, repeat(seq(/[*_]/, /[a-zA-Z0-9]+/)))),
    _word_space: (_$) => /[ \t]+/,
    _word_other: (_$) => /[^\na-zA-Z0-9 \t*_`#\[\]\\]+/,

    _delimiter_char: (_$) => /[*_`#\[\]\\]/,

    blank_line: ($) => $._newline,
  },
});
