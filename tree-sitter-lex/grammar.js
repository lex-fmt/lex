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
 * - Scanner emits emphasis delimiters: _strong_open, _strong_close,
 *   _emphasis_open, _emphasis_close (with flanking validation)
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
    $._strong_open, // opening * validated by scanner flanking rules
    $._strong_close, // closing * validated by scanner flanking rules
    $._emphasis_open, // opening _ validated by scanner flanking rules
    $._emphasis_close, // closing _ validated by scanner flanking rules
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
    // Scanner-validated delimiters enable proper nesting:
    //   *bold _italic_ inside* — emphasis nested inside strong
    //   _italic *bold* inside_ — strong nested inside emphasis
    // Same-type nesting is blocked by _no_star / _no_underscore variants.
    // First content token must be _word_alnum — this enforces the Lex rule
    // that "next char after opening delimiter must be alphanumeric".
    // The scanner validates the "prev" constraint; the grammar validates "next".
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

    // Inline content inside *strong* — excludes strong to prevent same-type nesting
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

    // Inline content inside _emphasis_ — excludes emphasis to prevent same-type nesting
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

    // Inline spans — matched as single regex tokens by the grammar lexer.
    code_span: (_$) => /`[^`\n]+`/,
    math_span: (_$) => /#[^#\n]+#/,
    reference: (_$) => /\[[^\]\n]+\]/,

    // Backslash before non-alphanumeric = escape (removes backslash).
    escape_sequence: (_$) => /\\[^a-zA-Z0-9\n]/,

    // Plain text — everything that isn't an inline delimiter or newline.
    // Split into three tokens so the scanner can infer the character class
    // of the last consumed character for emphasis flanking validation:
    //   _word_alnum always ends with an alphanumeric char (class WORD)
    //   _word_space always ends with whitespace (class WHITESPACE)
    //   _word_other always ends with punctuation (class PUNCTUATION)
    _word: ($) => choice($._word_alnum, $._word_space, $._word_other),
    _word_alnum: (_$) =>
      token(seq(/[a-zA-Z0-9]+/, repeat(seq(/[*_]/, /[a-zA-Z0-9]+/)))),
    _word_space: (_$) => /[ \t]+/,
    _word_other: (_$) => /[^\na-zA-Z0-9 \t*_`#\[\]\\]+/,

    // Fallback for unmatched delimiters (orphan *, _, `, #, [, ], \)
    _delimiter_char: (_$) => /[*_`#\[\]\\]/,

    blank_line: ($) => $._newline,
  },
});
