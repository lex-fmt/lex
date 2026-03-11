/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

/**
 * Tree-sitter grammar for the Lex document format.
 *
 * The external scanner detects line-level tokens (list lines, annotation
 * markers) because tree-sitter's longest-match lexer rule would otherwise
 * always prefer text_content (/[^\n]+/) over shorter prefixes.
 *
 * Token strategy:
 * - Scanner emits full-line tokens: list_item_line (entire line with marker)
 * - Scanner emits annotation_marker (:: prefix) and annotation_end_marker
 * - Grammar lexer emits: subject_content (line ending with :), text_content,
 *   inline tokens (code_span, math_span, reference, escape_sequence)
 * - INDENT/DEDENT/NEWLINE are always from scanner
 */
module.exports = grammar({
  name: "lex",

  externals: ($) => [
    $._indent,
    $._dedent,
    $._newline,
    $.annotation_marker, // ":: " at line start
    $.annotation_end_marker, // "::" alone on a line (closing marker)
    $.list_item_line, // entire line starting with list marker (- , 1. , etc.)
    $.subject_content, // entire line ending with : (scanner verifies EOL)
  ],

  extras: (_$) => [],

  conflicts: ($) => [
    // list_item_line can start a list_item or line_content (paragraph text)
    [$.list_item, $.line_content],
    // line_content _newline: text_line vs session/verbatim (blank lines case)
    [$.session, $.verbatim_block, $.text_line],
    // after dedent: session done vs verbatim continues with closing annotation
    [$.session, $.verbatim_block],
    // verbatim_block shares structure with definition (no blank lines case)
    [$.verbatim_block, $.definition],
    // verbatim without content: subject + closing annotation vs paragraph
    [$.verbatim_block, $.text_line],
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
    session: ($) =>
      prec.dynamic(
        1,
        seq(
          field("title", $.line_content),
          $._newline,
          repeat1($.blank_line),
          $._indent,
          repeat1($._block),
          $._dedent,
        ),
      ),

    // ===== Verbatim Blocks =====
    verbatim_block: ($) =>
      prec.dynamic(
        4,
        seq(
          field("subject", $.line_content),
          $._newline,
          repeat($.blank_line),
          optional(seq($._indent, repeat1($._block), $._dedent)),
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
          field("subject", $.line_content),
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
        $.list_item_line,
        $._newline,
        optional(seq($._indent, repeat1($._block), $._dedent)),
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

    annotation_header: (_$) => /[^\n:]+/,

    // ===== Paragraphs =====
    paragraph: ($) => prec.right(-1, repeat1($.text_line)),

    text_line: ($) => seq($.line_content, $._newline),

    line_content: ($) =>
      choice($.list_item_line, $.subject_content, $.text_content),

    // subject_content is an external token (scanner detects lines ending with :)

    // ===== Inline-Aware Text Content =====
    // Replaces the old monolithic /[^\n]+/ regex with inline element parsing.
    // subject_content still wins for colon-ending lines (longer match + prec).
    // list_item_line still wins for list lines (external scanner priority).
    text_content: ($) => repeat1($._inline),

    _inline: ($) =>
      choice(
        $.code_span,
        $.math_span,
        $.reference,
        $.escape_sequence,
        $._word,
        $._delimiter_char,
      ),

    // Literal inline spans — matched as single regex tokens.
    // Lexer longest-match ensures these win over _delimiter_char for
    // the opening character when a closing delimiter exists on the line.
    code_span: (_$) => /`[^`\n]+`/,
    math_span: (_$) => /#[^#\n]+#/,
    reference: (_$) => /\[[^\]\n]+\]/,

    // Backslash before non-alphanumeric = escape (removes backslash).
    // Backslash before alphanumeric = preserved (falls through to
    // _delimiter_char for \ + _word for the letter).
    escape_sequence: (_$) => /\\[^a-zA-Z0-9\n]/,

    // Plain text — everything that isn't an inline delimiter or newline
    _word: (_$) => /[^\n*_`#\[\]\\]+/,

    // Fallback for unmatched delimiters (orphan *, _, `, #, [, ], \)
    _delimiter_char: (_$) => /[*_`#\[\]\\]/,

    blank_line: ($) => $._newline,
  },
});
