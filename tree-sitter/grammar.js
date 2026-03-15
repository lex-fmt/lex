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
 * - Scanner emits _list_start: list_marker when next line also has a list marker
 *   (confirms 2+ item list boundary — paragraphs can't absorb this token)
 * - Scanner emits full-line token: subject_content (line ending with :)
 * - Scanner emits _definition_subject: subject_content when next line has
 *   increased indent (confirms definition/verbatim boundary)
 * - Scanner emits annotation_marker (:: prefix) and annotation_end_marker
 * - Scanner emits emphasis delimiters: _strong_open, _strong_close,
 *   _emphasis_open, _emphasis_close (with flanking validation)
 * - Scanner emits _session_break: blank line(s) + indent increase (lookahead)
 * - Scanner emits _pipe_row_start: | at line start with ≥2 pipes on the line
 * - Scanner emits pipe_delimiter: subsequent | inside an active pipe row
 * - Scanner emits _table_separator: full separator line (|---|---|)
 * - Grammar lexer emits: text_content (inline-aware), inline tokens
 *   (code_span, math_span, reference, escape_sequence)
 * - INDENT/DEDENT/NEWLINE are always from scanner
 *
 * Block boundary disambiguation:
 *   Paragraphs greedily absorb list-marker lines (as dialog_line) and subject
 *   lines (via line_content). Without scanner help, a paragraph never yields
 *   to a list or definition mid-stream. The scanner solves this with lookahead:
 *   _list_start is emitted instead of list_marker when the next line also has
 *   a list marker (confirming a 2+ item list). _definition_subject is emitted
 *   instead of subject_content when the next line has increased indent. Since
 *   paragraphs only match list_marker and subject_content, they deterministically
 *   end before these boundary tokens, allowing lists and definitions to start
 *   without preceding blank lines.
 *
 * Table row disambiguation:
 *   Lines starting with | that have at least one more | on the line are
 *   detected by the scanner as pipe rows. The scanner emits _pipe_row_start
 *   for the first |, then pipe_delimiter for each subsequent |. Since
 *   paragraphs don't match _pipe_row_start, they end before table rows.
 *   The in_pipe_row scanner state ensures | characters within a pipe row
 *   are intercepted as pipe_delimiter before the grammar lexer can consume
 *   them as part of _word_other.
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
    $.list_marker, // list marker only: "- ", "1. ", "a) ", etc.
    $.subject_content, // entire line ending with : (scanner verifies EOL)
    $._strong_open, // opening * validated by scanner flanking rules
    $._strong_close, // closing * validated by scanner flanking rules
    $._emphasis_open, // opening _ validated by scanner flanking rules
    $._emphasis_close, // closing _ validated by scanner flanking rules
    $._session_break, // blank line(s) + indent increase (scanner lookahead)
    $.verbatim_content, // fullwidth verbatim: opaque multi-line content block
    $._list_start, // list_marker when next line also has list marker (lookahead)
    $._definition_subject, // subject_content when next line has indent (lookahead)
    $._pipe_row_start, // | at line start when line has pipe structure (≥2 pipes)
    $.pipe_delimiter, // subsequent | inside an active pipe row
    $._table_separator, // full separator line: |---|---|
  ],

  extras: (_$) => [],

  conflicts: ($) => [
    // _list_start: list_start_item vs session/document title at _block level
    [$._list_start_item, $._session_title],
    // list_marker: dialog_line vs session/document title (at _block level)
    [$.dialog_line, $._session_title],
    // document_title's repeat1(blank_line) end ambiguity
    [$.document_title],
    // blank_line after dedent: part of list_item's trailing blanks or next block
    [$.list_item],
    [$._list_start_item],
    // subject_content: verbatim vs line_content vs session title
    [$.verbatim_block, $.line_content, $._session_title],
    // _definition_subject: verbatim vs definition vs session title
    [$.verbatim_block, $.definition, $._session_title],
    // verbatim_block shares structure with definition
    [$.verbatim_block, $.definition],
    // text_content: session title vs line_content in paragraph
    [$._session_title, $.line_content],
    // blank lines between verbatim groups: body's repeat vs group_item's repeat
    [$.verbatim_group_item],
  ],

  rules: {
    // Document optionally starts with a title, then content blocks.
    // document_title is ONLY in this position — it cannot appear mid-document.
    // GLR resolves the fork: if blank+indent follows → session wins via
    // _session_break; if just blank → document_title wins via dynamic prec.
    document: ($) =>
      choice(
        seq($.document_title, repeat($._block)),
        repeat1($._block),
      ),

    // ===== Document Title =====
    // A single line followed by blank line(s), only at document start.
    // Dynamic precedence makes this win over the repeat1(_block) alternative.
    document_title: ($) =>
      prec.dynamic(
        2,
        seq(
          field("title", alias($._session_title, $.line_content)),
          $._newline,
          repeat1($.blank_line),
        ),
      ),

    _block: ($) =>
      choice(
        $.verbatim_block,
        $.annotation_block,
        $.annotation_single,
        $.definition,
        $.session,
        $.list,
        $.table_row,
        $.table_separator_row,
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
          field("title", alias($._session_title, $.line_content)),
          $._newline,
          $._session_break,
          repeat1($._block),
          $._dedent,
        ),
      ),

    // Session titles can include list markers (e.g., "1. Introduction")
    // and subject lines (e.g., "Chapter Title:"). Both scanner-differentiated
    // variants are accepted and aliased to preserve tree structure.
    _session_title: ($) =>
      choice(
        seq(
          choice(alias($._list_start, $.list_marker), $.list_marker),
          optional($.text_content),
        ),
        choice(
          alias($._definition_subject, $.subject_content),
          $.subject_content,
        ),
        $.text_content,
      ),

    // ===== Verbatim Blocks =====
    // Verbatim blocks can contain multiple subject/content pairs (groups)
    // sharing a single closing annotation. The first subject/content lives
    // directly in verbatim_block; additional pairs are verbatim_group_item nodes.
    verbatim_block: ($) =>
      prec.dynamic(
        4,
        seq(
          field(
            "subject",
            choice(
              alias($._definition_subject, $.subject_content),
              $.subject_content,
            ),
          ),
          $._newline,
          choice(
            // Blank line(s) + indent: scanner emits _session_break
            seq($._session_break, repeat1($._block), $._dedent),
            // No blank line, direct indent (or no content at all)
            seq(
              repeat($.blank_line),
              optional(seq($._indent, repeat1($._block), $._dedent)),
            ),
            // Fullwidth: content at column 1 (sub-indent-width), scanner
            // emits an opaque multi-line verbatim_content token
            seq(repeat($.blank_line), $.verbatim_content),
          ),
          repeat($.verbatim_group_item),
          $.annotation_marker,
          $.annotation_header,
          $.annotation_marker,
          $._newline,
        ),
      ),

    // Additional subject/content pair in a verbatim group.
    verbatim_group_item: ($) =>
      prec.dynamic(
        4,
        prec.right(
          seq(
            repeat($.blank_line),
            field(
              "subject",
              choice(
                alias($._definition_subject, $.subject_content),
                $.subject_content,
              ),
            ),
            $._newline,
            choice(
              seq($._session_break, repeat1($._block), $._dedent),
              seq(
                repeat($.blank_line),
                optional(seq($._indent, repeat1($._block), $._dedent)),
              ),
              seq(repeat($.blank_line), $.verbatim_content),
            ),
          ),
        ),
      ),

    // ===== Definitions =====
    // Uses _definition_subject from scanner (subject + indent lookahead)
    // so paragraphs can't absorb definition subjects.
    definition: ($) =>
      prec.dynamic(
        2,
        seq(
          field("subject", alias($._definition_subject, $.subject_content)),
          $._newline,
          $._indent,
          repeat1($._block),
          $._dedent,
        ),
      ),

    // ===== Lists =====
    // Lists start with _list_start_item (scanner confirms 2+ items via lookahead)
    // then continue with list_items (using regular list_marker). This split is
    // critical: only _list_start can begin a list at the _block level, preventing
    // the list_item fork from making INDENT valid in paragraph contexts.
    list: ($) =>
      prec.dynamic(
        3,
        prec.right(
          seq(alias($._list_start_item, $.list_item), repeat($.list_item)),
        ),
      ),

    // First item of a confirmed list — uses _list_start from scanner.
    // Aliased to list_item in the list rule for uniform tree structure.
    _list_start_item: ($) =>
      seq(
        alias($._list_start, $.list_marker),
        optional($.text_content),
        $._newline,
        optional(
          seq(
            $._indent,
            repeat1($._block),
            $._dedent,
            repeat($.blank_line),
          ),
        ),
      ),

    // Subsequent list items — accepts both list_marker and _list_start.
    // _list_start may be emitted by the scanner even for middle items because
    // GLR unions valid_symbols across forks (the "end list → new _block → list"
    // fork makes LIST_START valid alongside the "continue list" fork).
    list_item: ($) =>
      seq(
        choice(alias($._list_start, $.list_marker), $.list_marker),
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

    // ===== Table Rows =====
    // Pipe-delimited table rows. The scanner emits _pipe_row_start for the
    // first | and pipe_delimiter for subsequent |. Cell content between
    // delimiters uses the standard _inline rule — the scanner intercepts
    // | as pipe_delimiter before the grammar lexer matches it as _word_other.
    table_row: ($) =>
      prec(
        5,
        seq(
          alias($._pipe_row_start, $.pipe_delimiter),
          repeat1(seq(optional($.table_cell), $.pipe_delimiter)),
          $._newline,
        ),
      ),

    // Table separator row (cosmetic: |---|---|). The scanner emits the
    // entire line content as _table_separator.
    table_separator_row: ($) =>
      prec(6, seq($._table_separator, $._newline)),

    // Cell content between pipe delimiters. Wraps text_content (named node)
    // so that inline elements are visible in the CST. The scanner intercepts
    // | as pipe_delimiter, so cell content naturally stops at pipe boundaries.
    table_cell: ($) => $.text_content,

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
    // Paragraphs consume text_line and dialog_line. They only match
    // list_marker (not _list_start) and subject_content (not _definition_subject),
    // so the paragraph deterministically ends before confirmed list/definition
    // boundaries.
    paragraph: ($) =>
      prec.right(-1, repeat1(choice($.text_line, $.dialog_line))),

    text_line: ($) => seq($.line_content, $._newline),

    // Dialog line: list-marker line in paragraph context (single item = dialog).
    // Uses list_marker only — _list_start forces paragraph to end.
    dialog_line: ($) => seq($.list_marker, optional($.text_content), $._newline),

    // Line content: subject or text (no list markers — those go through
    // dialog_line in paragraphs or _session_title in sessions).
    // Uses subject_content only — _definition_subject forces paragraph to end.
    line_content: ($) => choice($.subject_content, $.text_content),

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

    // Reference types — lexically distinguished by prefix/content.
    // Mirrors the classification in lex-core/src/lex/inlines/references.rs.
    // Order matters: token.immediate() alternatives are tried by specificity.
    reference: ($) =>
      choice(
        $.citation_reference,
        $.footnote_reference,
        $.url_reference,
        $.file_reference,
        $.session_reference,
        $.tocome_reference,
        $.number_reference,
        $.general_reference,
      ),

    // [@key] or [@key, p.42] — citation
    citation_reference: (_$) => token(seq("[", "@", /[^\]\n]+/, "]")),

    // [^label] — labeled footnote
    footnote_reference: (_$) => token(seq("[", "^", /[^\]\n]+/, "]")),

    // [https://...], [http://...], [mailto:...] — URL
    url_reference: (_$) =>
      token(
        choice(
          seq("[", "https://", /[^\]\n]+/, "]"),
          seq("[", "http://", /[^\]\n]+/, "]"),
          seq("[", "mailto:", /[^\]\n]+/, "]"),
        ),
      ),

    // [./...], [../...], [/...] — file path
    file_reference: (_$) =>
      token(
        choice(
          seq("[", "./", /[^\]\n]*/, "]"),
          seq("[", "../", /[^\]\n]*/, "]"),
          seq("[", "/", /[^\]\n]+/, "]"),
        ),
      ),

    // [#digits.dashes] — session reference
    session_reference: (_$) => token(seq("[", "#", /[0-9][0-9.\-]*/, "]")),

    // [TK] or [TK-identifier] — to-come placeholder (case insensitive)
    tocome_reference: (_$) =>
      token(
        choice(
          seq("[", choice("TK", "tk", "Tk", "tK"), "]"),
          seq(
            "[",
            choice("TK-", "tk-", "Tk-", "tK-"),
            /[a-z0-9]+/,
            "]",
          ),
        ),
      ),

    // [42] — numbered footnote (digits only)
    number_reference: (_$) => token(seq("[", /[0-9]+/, "]")),

    // [anything else] — general reference (fallback)
    general_reference: (_$) => /\[[^\]\n]+\]/,

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
