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
 * - Grammar lexer emits: subject_content (line ending with :), text_content
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
    // title + blank line(s) + INDENT + content + DEDENT
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
    // subject (ends with :) + optional blank lines + INDENT + content + DEDENT
    // + closing annotation (:: label params ::)
    // Content is structurally parsed as blocks but represents raw/verbatim text.
    // Higher dynamic precedence than definition/session — closing annotation
    // disambiguates via GLR.
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
    // subject (ends with :) + INDENT immediately
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
    // 2+ list items required
    list: ($) =>
      prec.dynamic(3, prec.right(seq($.list_item, repeat1($.list_item)))),

    list_item: ($) =>
      seq(
        $.list_item_line,
        $._newline,
        optional(seq($._indent, repeat1($._block), $._dedent)),
      ),

    // ===== Annotations =====
    // Block: :: header :: [text] \n INDENT blocks DEDENT [:: \n]
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

    // Single-line / marker: :: header :: [text] \n
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

    // Scanner emits synthetic NEWLINE at EOF, so every line ends with one
    text_line: ($) => seq($.line_content, $._newline),

    // Any single line of content (including list-marker lines that didn't
    // form a list of 2+ items). Named rule so it can participate in conflicts.
    line_content: ($) =>
      choice($.list_item_line, $.subject_content, $.text_content),

    // Lines ending with colon — higher lexer precedence breaks ties with
    // text_content when both match the same length
    subject_content: (_$) => token(prec(1, /[^\n]+:/)),

    // Any non-empty line content (fallback)
    text_content: (_$) => /[^\n]+/,

    blank_line: ($) => $._newline,
  },
});
