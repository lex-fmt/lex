/**
 * External scanner for tree-sitter (lex-fmt/lex).
 *
 * Handles:
 * - Indentation-based structure (INDENT/DEDENT tokens)
 * - NEWLINE emission at line boundaries and EOF
 * - Line-start detection for annotation markers (::) and list markers
 * - Session boundary detection via lookahead (_session_break)
 * - Emphasis delimiter validation (*strong* and _emphasis_) with flanking rules
 *
 * Session break lookahead:
 *   When a blank line is encountered, the scanner peeks ahead past additional
 *   blank lines to check if the next non-blank line has increased indent.
 *   If yes, it emits _session_break (which encompasses the blank lines and
 *   indent whitespace, also pushing the new indent level onto the stack).
 *   If no, it emits a regular NEWLINE for the blank line. This eliminates
 *   the GLR ambiguity between sessions and paragraphs that caused nested
 *   sessions to parse incorrectly.
 *
 * Flanking context tracking:
 *   The scanner tracks `last_char_class` to know what preceded the current
 *   position. When the scanner returns false (grammar lexer handles the token),
 *   it infers context from `lexer->lookahead`: the grammar's _word is split into
 *   _word_alnum (always class WORD), _word_space (always class WHITESPACE), and
 *   _word_other (always class PUNCTUATION). All other grammar tokens (code_span,
 *   math_span, reference, escape_sequence, _delimiter_char) end with punctuation.
 *   So classify_char(lookahead) correctly predicts the class of the LAST character
 *   of whatever grammar token will be consumed.
 *
 * Lex uses 4-space indentation units (or 1 tab = 1 level).
 *
 * Externals order (must match grammar.js):
 *   0: _indent
 *   1: _dedent
 *   2: _newline
 *   3: annotation_marker
 *   4: annotation_end_marker
 *   5: list_marker
 *   6: subject_content
 *   7: _strong_open
 *   8: _strong_close
 *   9: _emphasis_open
 *  10: _emphasis_close
 *  11: _session_break
 *
 * Flanking validation for emphasis delimiters:
 *   Opening: prev char must not be alphanumeric (WORD class), next must be WORD.
 *   Closing: prev must not be whitespace/none, next must not be WORD.
 *   The "prev" check uses last_char_class, which is only reliably updated when
 *   scan() returns true. To compensate, _word_alnum in grammar.js is defined as
 *   a greedy pattern that absorbs word-adjacent * and _ (e.g. word*not, snake_case)
 *   so the scanner never sees them as separate delimiter tokens.
 */

#include "tree_sitter/parser.h"

#include <string.h>
#ifdef SCANNER_DEBUG
#include <stdio.h>
#endif

#define MAX_INDENT_DEPTH 64
#define INDENT_WIDTH 4

enum TokenType {
    INDENT,
    DEDENT,
    NEWLINE,
    ANNOTATION_MARKER,
    ANNOTATION_END_MARKER,
    LIST_MARKER,
    SUBJECT_CONTENT,
    STRONG_OPEN,
    STRONG_CLOSE,
    EMPHASIS_OPEN,
    EMPHASIS_CLOSE,
    SESSION_BREAK,
};

// Character class for flanking rule context tracking.
// 0 = start-of-line/none, 1 = whitespace, 2 = punctuation, 3 = word (alnum)
#define CHAR_CLASS_NONE 0
#define CHAR_CLASS_WHITESPACE 1
#define CHAR_CLASS_PUNCTUATION 2
#define CHAR_CLASS_WORD 3

typedef struct {
    int indent_stack[MAX_INDENT_DEPTH];
    int indent_depth;
    int pending_dedents;
    bool at_line_start;
    bool emitted_eof_newline;
    uint8_t last_char_class;
    int line_indent;       // measured indent of current line (avoids re-measurement after DEDENT)
    bool indent_measured;  // true when line_indent is valid for the current line
} Scanner;

static uint8_t classify_char(int32_t c) {
    if (c == 0) return CHAR_CLASS_NONE;
    if (c == ' ' || c == '\t' || c == '\n' || c == '\r') return CHAR_CLASS_WHITESPACE;
    if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9'))
        return CHAR_CLASS_WORD;
    return CHAR_CLASS_PUNCTUATION;
}

void *tree_sitter_lex_external_scanner_create(void) {
    Scanner *scanner = calloc(1, sizeof(Scanner));
    scanner->indent_stack[0] = 0;
    scanner->indent_depth = 0;
    scanner->pending_dedents = 0;
    scanner->at_line_start = true;
    scanner->emitted_eof_newline = false;
    scanner->last_char_class = CHAR_CLASS_NONE;
    scanner->line_indent = 0;
    scanner->indent_measured = false;
    return scanner;
}

void tree_sitter_lex_external_scanner_destroy(void *payload) {
    free(payload);
}

unsigned tree_sitter_lex_external_scanner_serialize(void *payload,
                                                     char *buffer) {
    Scanner *scanner = (Scanner *)payload;
    unsigned offset = 0;

    buffer[offset++] = (char)scanner->indent_depth;
    buffer[offset++] = (char)scanner->pending_dedents;
    buffer[offset++] = (char)scanner->at_line_start;
    buffer[offset++] = (char)scanner->emitted_eof_newline;
    buffer[offset++] = (char)scanner->last_char_class;
    buffer[offset++] = (char)scanner->indent_measured;
    int16_t li = (int16_t)scanner->line_indent;
    memcpy(buffer + offset, &li, 2);
    offset += 2;

    for (int i = 0; i <= scanner->indent_depth; i++) {
        int16_t val = (int16_t)scanner->indent_stack[i];
        memcpy(buffer + offset, &val, 2);
        offset += 2;
    }

    return offset;
}

void tree_sitter_lex_external_scanner_deserialize(void *payload,
                                                    const char *buffer,
                                                    unsigned length) {
    Scanner *scanner = (Scanner *)payload;

    if (length == 0) {
        scanner->indent_depth = 0;
        scanner->indent_stack[0] = 0;
        scanner->pending_dedents = 0;
        scanner->at_line_start = true;
        scanner->emitted_eof_newline = false;
        scanner->last_char_class = CHAR_CLASS_NONE;
        scanner->line_indent = 0;
        scanner->indent_measured = false;
        return;
    }

    unsigned offset = 0;
    scanner->indent_depth = (int)(unsigned char)buffer[offset++];
    scanner->pending_dedents = (int)(unsigned char)buffer[offset++];
    scanner->at_line_start = (bool)buffer[offset++];
    scanner->emitted_eof_newline = (bool)buffer[offset++];
    scanner->last_char_class = (uint8_t)(unsigned char)buffer[offset++];
    scanner->indent_measured = (bool)buffer[offset++];
    int16_t li;
    memcpy(&li, buffer + offset, 2);
    scanner->line_indent = li;
    offset += 2;

    for (int i = 0; i <= scanner->indent_depth; i++) {
        int16_t val;
        memcpy(&val, buffer + offset, 2);
        scanner->indent_stack[i] = val;
        offset += 2;
    }
}

/// Check if a character is a digit
static bool is_digit(int32_t c) { return c >= '0' && c <= '9'; }

/// Check if a character is a lowercase letter
static bool is_lower(int32_t c) { return c >= 'a' && c <= 'z'; }

/// Check if a character is an uppercase letter
static bool is_upper(int32_t c) { return c >= 'A' && c <= 'Z'; }

/// Check if a character is a roman numeral letter (upper)
static bool is_roman_upper(int32_t c) {
    return c == 'I' || c == 'V' || c == 'X' || c == 'L' || c == 'C' ||
           c == 'D' || c == 'M';
}

/// Try to match a list marker at the current position.
/// Returns true if a valid list marker was found, false otherwise.
/// On success, the lexer is positioned right after the marker+space.
static bool try_list_marker(TSLexer *lexer) {
    // Plain dash: "- "
    if (lexer->lookahead == '-') {
        lexer->advance(lexer, false);
        if (lexer->lookahead == ' ') {
            lexer->advance(lexer, false);
            return true;
        }
        return false;
    }

    // Double-paren form: (1), (a), (IV)
    if (lexer->lookahead == '(') {
        lexer->advance(lexer, false);
        bool has_content = false;
        while (is_digit(lexer->lookahead) || is_lower(lexer->lookahead) ||
               is_roman_upper(lexer->lookahead)) {
            lexer->advance(lexer, false);
            has_content = true;
        }
        if (has_content && lexer->lookahead == ')') {
            lexer->advance(lexer, false);
            if (lexer->lookahead == ' ') {
                lexer->advance(lexer, false);
                return true;
            }
        }
        return false;
    }

    // Numerical: 1. 1) | Alphabetical: a. a) | Roman: IV. IV)
    bool starts_digit = is_digit(lexer->lookahead);
    bool starts_lower = is_lower(lexer->lookahead);
    bool starts_upper = is_upper(lexer->lookahead);

    if (!starts_digit && !starts_lower && !starts_upper) return false;

    lexer->advance(lexer, false);

    if (starts_digit) {
        while (is_digit(lexer->lookahead)) lexer->advance(lexer, false);
        // Extended form: 1.2.3
        while (lexer->lookahead == '.') {
            lexer->advance(lexer, false);
            if (!is_digit(lexer->lookahead) && !is_lower(lexer->lookahead) &&
                !is_roman_upper(lexer->lookahead)) {
                // Period followed by space = "1. " style
                if (lexer->lookahead == ' ') {
                    lexer->advance(lexer, false);
                    return true;
                }
                return false;
            }
            while (is_digit(lexer->lookahead) ||
                   is_lower(lexer->lookahead) ||
                   is_roman_upper(lexer->lookahead)) {
                lexer->advance(lexer, false);
            }
        }
    } else if (starts_lower) {
        // Single lowercase letter — already consumed
    } else if (starts_upper) {
        while (is_roman_upper(lexer->lookahead)) {
            lexer->advance(lexer, false);
        }
    }

    // Expect separator: . or )
    if (lexer->lookahead == '.' || lexer->lookahead == ')') {
        lexer->advance(lexer, false);
        if (lexer->lookahead == ' ') {
            lexer->advance(lexer, false);
            return true;
        }
    }

    return false;
}

/// Consume the rest of the line (everything up to but not including \n or EOF).
static void consume_rest_of_line(TSLexer *lexer) {
    while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
        lexer->advance(lexer, false);
    }
}

/// Scan ahead (without marking) to check if a matching delimiter exists on
/// the rest of the current line. Used to avoid emitting _strong_open or
/// _emphasis_open when no closer exists — which would produce ERROR nodes
/// for cases like "*List preceding blank" or "_not emphasized".
static bool has_matching_closer(TSLexer *lexer, int32_t delimiter) {
    while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
        if (lexer->lookahead == delimiter) {
            return true;
        }
        lexer->advance(lexer, false);
    }
    return false;
}

bool tree_sitter_lex_external_scanner_scan(void *payload, TSLexer *lexer,
                                            const bool *valid_symbols) {
    Scanner *scanner = (Scanner *)payload;

#ifdef SCANNER_DEBUG
    fprintf(stderr, "SCAN: at_line_start=%d depth=%d stack=[",
            scanner->at_line_start, scanner->indent_depth);
    for (int i = 0; i <= scanner->indent_depth; i++) {
        fprintf(stderr, "%d%s", scanner->indent_stack[i],
                i < scanner->indent_depth ? "," : "");
    }
    fprintf(stderr, "] pending=%d lookahead='%c'(%d) valid=[",
            scanner->pending_dedents, lexer->lookahead > 31 ? lexer->lookahead : '?',
            lexer->lookahead);
    const char *names[] = {"IND","DED","NL","AM","AEM","LM","SC","SO","SCl","EO","ECl","SB"};
    for (int i = 0; i <= 11; i++) {
        if (valid_symbols[i]) fprintf(stderr, "%s ", names[i]);
    }
    fprintf(stderr, "]\n");
#endif

    // Emit pending DEDENT tokens
    if (scanner->pending_dedents > 0 && valid_symbols[DEDENT]) {
        scanner->pending_dedents--;
        lexer->result_symbol = DEDENT;
        return true;
    }

    // At line start, calculate indentation and detect line-start tokens
    if (scanner->at_line_start) {
        int indent;
        if (scanner->indent_measured) {
            // Indent was already measured for this line (we're re-entering
            // after a DEDENT emission). Don't re-count — the whitespace
            // was already consumed by the prior scan.
            indent = scanner->line_indent;
        } else {
            indent = 0;
            while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                if (lexer->lookahead == '\t') {
                    indent += INDENT_WIDTH;
                } else {
                    indent++;
                }
                lexer->advance(lexer, true);
            }
            scanner->line_indent = indent;
            scanner->indent_measured = true;
        }

        // Blank line — check for session break or emit NEWLINE
        if (lexer->lookahead == '\n') {
            // Session break detection: blank line(s) followed by indent increase.
            // The scanner peeks ahead to determine if this blank line starts a
            // session boundary. If yes, emit SESSION_BREAK (encompassing the
            // blank lines + indent whitespace). If no, emit regular NEWLINE.
            if (valid_symbols[SESSION_BREAK]) {
                // Consume the first \n — this is our minimum token
                lexer->advance(lexer, false);
                lexer->mark_end(lexer);

                // Peek ahead past additional blank lines WITHOUT updating
                // mark_end. If SESSION_BREAK fires, we'll update mark_end
                // to include everything. If not, the token only covers
                // the first blank line (so each blank becomes its own node).
                while (lexer->lookahead == '\n') {
                    lexer->advance(lexer, false);
                }

                // If EOF after blank lines, not a session break
                if (lexer->eof(lexer)) {
                    lexer->result_symbol = NEWLINE;
                    scanner->at_line_start = true;
                    scanner->indent_measured = false;
                    scanner->last_char_class = CHAR_CLASS_NONE;
                    return true;
                }

                // Count indent of next non-blank line.
                // Advance with skip=false so characters are consumed
                // if SESSION_BREAK fires (we'll call mark_end then).
                int next_indent = 0;
                while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                    if (lexer->lookahead == '\t') {
                        next_indent += INDENT_WIDTH;
                    } else {
                        next_indent++;
                    }
                    lexer->advance(lexer, false);
                }

                // Check if next line is actually another blank line
                // (with leading whitespace)
                if (lexer->lookahead == '\n') {
                    // Not a session break — emit NEWLINE for just the
                    // first blank line (mark_end is already after it).
                    lexer->result_symbol = NEWLINE;
                    scanner->at_line_start = true;
                    scanner->indent_measured = false;
                    scanner->last_char_class = CHAR_CLASS_NONE;
                    return true;
                }

                int current_indent =
                    scanner->indent_stack[scanner->indent_depth];
                if (next_indent > current_indent) {
                    // Session break confirmed! Update mark_end to include
                    // all blank lines + indent whitespace.
                    lexer->mark_end(lexer);
                    scanner->indent_depth++;
                    scanner->indent_stack[scanner->indent_depth] = next_indent;
                    scanner->at_line_start = false;
                    scanner->last_char_class = CHAR_CLASS_NONE;
                    lexer->result_symbol = SESSION_BREAK;
                    return true;
                }

                // Not a session break. Emit NEWLINE for just the first
                // blank line. mark_end is after the first \n only —
                // subsequent blank lines and indent whitespace are past
                // mark_end and will be re-scanned.
                lexer->result_symbol = NEWLINE;
                scanner->at_line_start = true;
                scanner->indent_measured = false;
                scanner->last_char_class = CHAR_CLASS_NONE;
                return true;
            }

            // No session break context — regular blank line NEWLINE
            if (valid_symbols[NEWLINE]) {
                lexer->advance(lexer, false);
                lexer->result_symbol = NEWLINE;
                scanner->at_line_start = true;
                scanner->indent_measured = false;
                scanner->last_char_class = CHAR_CLASS_NONE;
                return true;
            }
            return false;
        }

        // End of file — emit remaining DEDENTs, then one synthetic NEWLINE
        if (lexer->eof(lexer)) {
            if (scanner->indent_depth > 0 && valid_symbols[DEDENT]) {
                scanner->indent_depth--;
                scanner->pending_dedents = scanner->indent_depth;
                scanner->indent_depth = 0;
                lexer->result_symbol = DEDENT;
                return true;
            }
            // Emit one synthetic NEWLINE at EOF to close any pending line
            if (!scanner->emitted_eof_newline && valid_symbols[NEWLINE]) {
                scanner->emitted_eof_newline = true;
                lexer->result_symbol = NEWLINE;
                return true;
            }
            return false;
        }

        int current_indent = scanner->indent_stack[scanner->indent_depth];

        // Handle indentation changes
        if (indent > current_indent) {
            if (valid_symbols[INDENT]) {
                scanner->indent_depth++;
                scanner->indent_stack[scanner->indent_depth] = indent;
                lexer->result_symbol = INDENT;
                scanner->at_line_start = false;
                scanner->last_char_class = CHAR_CLASS_NONE;
                return true;
            }
            // If INDENT is not valid, fall through to content detection
        } else if (indent < current_indent) {
            if (valid_symbols[DEDENT]) {
                int dedents = 0;
                while (scanner->indent_depth > 0 &&
                       scanner->indent_stack[scanner->indent_depth] > indent) {
                    scanner->indent_depth--;
                    dedents++;
                }
                if (dedents > 1) {
                    scanner->pending_dedents = dedents - 1;
                }
                lexer->result_symbol = DEDENT;
                // Keep at_line_start=true so next scan can detect
                // line-start tokens (annotation_end_marker, etc.)
                return true;
            }
        }

        scanner->at_line_start = false;
        scanner->last_char_class = CHAR_CLASS_NONE;

        // After handling indentation, try to detect line-start tokens.
        lexer->mark_end(lexer);

        // Save the first content char for context tracking. If we return
        // false later, the grammar will consume from here, and this char's
        // class tells us what the grammar's token class will be.
        int32_t line_start_char = lexer->lookahead;

        // Try annotation end marker: :: alone on a line (with optional whitespace)
        // Must check before annotation_marker to handle closing :: correctly
        if (valid_symbols[ANNOTATION_END_MARKER] && lexer->lookahead == ':') {
            lexer->advance(lexer, false);
            if (lexer->lookahead == ':') {
                lexer->advance(lexer, false);
                // Check that nothing else follows except whitespace and newline
                while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                    lexer->advance(lexer, false);
                }
                if (lexer->lookahead == '\n' || lexer->eof(lexer)) {
                    lexer->mark_end(lexer);
                    lexer->result_symbol = ANNOTATION_END_MARKER;
                    return true;
                }
            }
            // Not an end marker — but we already consumed ::
            // Try as annotation_marker instead
            if (valid_symbols[ANNOTATION_MARKER]) {
                lexer->mark_end(lexer);
                if (lexer->lookahead == ' ') {
                    lexer->advance(lexer, false);
                    lexer->mark_end(lexer);
                }
                lexer->result_symbol = ANNOTATION_MARKER;
                return true;
            }
            return false;
        }

        // Try annotation marker: :: at line start
        if (valid_symbols[ANNOTATION_MARKER] && lexer->lookahead == ':') {
            lexer->advance(lexer, false);
            if (lexer->lookahead == ':') {
                lexer->advance(lexer, false);
                lexer->mark_end(lexer);
                if (lexer->lookahead == ' ') {
                    lexer->advance(lexer, false);
                    lexer->mark_end(lexer);
                }
                lexer->result_symbol = ANNOTATION_MARKER;
                return true;
            }
            // Single colon — not an annotation, let grammar lexer handle it
            return false;
        }

        // When the first char is an emphasis delimiter (* or _), we need to
        // decide between emphasis and full-line tokens (subject_content).
        // Strategy: scan the line once to check for trailing :. Use mark_end
        // to control the emitted token's span — either 1 char (emphasis) or
        // the whole line (subject_content).
        if ((lexer->lookahead == '*' || lexer->lookahead == '_') &&
            (valid_symbols[STRONG_OPEN] || valid_symbols[EMPHASIS_OPEN])) {
            int32_t delimiter = lexer->lookahead;
            lexer->advance(lexer, false);        // skip delimiter (now at X+1)
            int32_t next_char = lexer->lookahead; // char after delimiter
            lexer->mark_end(lexer);               // mark at X+1 (1-char token)

            // Scan rest of line to check for trailing : and matching closer
            int32_t last_char = next_char;
            bool has_closer = false;
            while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
                if (lexer->lookahead == delimiter) has_closer = true;
                last_char = lexer->lookahead;
                lexer->advance(lexer, false);
            }

            if (last_char == ':' && valid_symbols[SUBJECT_CONTENT]) {
                // Subject line — re-mark end at EOL for full line
                lexer->mark_end(lexer);
                lexer->result_symbol = SUBJECT_CONTENT;
                return true;
            }

            // Not a subject. Try emphasis open — mark_end is at X+1.
            // Only emit if a matching closer exists on this line,
            // otherwise the unclosed delimiter produces ERROR nodes.
            if (has_closer && delimiter == '*' && valid_symbols[STRONG_OPEN] &&
                classify_char(next_char) == CHAR_CLASS_WORD) {
                scanner->last_char_class = CHAR_CLASS_PUNCTUATION;
                lexer->result_symbol = STRONG_OPEN;
                return true;
            }
            if (has_closer && delimiter == '_' && valid_symbols[EMPHASIS_OPEN] &&
                classify_char(next_char) == CHAR_CLASS_WORD) {
                scanner->last_char_class = CHAR_CLASS_PUNCTUATION;
                lexer->result_symbol = EMPHASIS_OPEN;
                return true;
            }

            // Neither subject nor valid emphasis. mark_end is at X+1.
            // Return false — position resets to after the delimiter.
            scanner->last_char_class = CHAR_CLASS_PUNCTUATION;
            return false;
        }

        // Try list marker: just the marker portion (- , 1. , a) , etc.)
        // Content after the marker is handled by the grammar's text_content
        // rule, which decomposes inline elements (bold, references, etc.).
        if (valid_symbols[LIST_MARKER]) {
            if (try_list_marker(lexer)) {
                lexer->mark_end(lexer);
                lexer->result_symbol = LIST_MARKER;
                // Marker ends with a space — set class for emphasis flanking
                scanner->last_char_class = CHAR_CLASS_WHITESPACE;
                return true;
            }
            // Not a list marker — fall through to subject_content check.
            // try_list_marker may have advanced the position, but the
            // subject_content scan continues from there to EOL.
        }

        // Try subject content: entire line ending with :
        if (valid_symbols[SUBJECT_CONTENT]) {
            int32_t last_char = 0;
            while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
                last_char = lexer->lookahead;
                lexer->advance(lexer, false);
            }
            if (last_char == ':') {
                lexer->mark_end(lexer);
                lexer->result_symbol = SUBJECT_CONTENT;
                return true;
            }
            // Line doesn't end with : — return false, position resets to
            // mark_end (line start). Set last_char_class for the grammar
            // token that will be consumed at that position.
            scanner->last_char_class = classify_char(line_start_char);
            return false;
        }

        scanner->last_char_class = classify_char(line_start_char);
        return false;
    }

    // === Not at line start ===

    // Try list marker (e.g., first line after INDENT in a definition body)
    if (valid_symbols[LIST_MARKER]) {
        lexer->mark_end(lexer);
        if (try_list_marker(lexer)) {
            lexer->mark_end(lexer);
            lexer->result_symbol = LIST_MARKER;
            scanner->last_char_class = CHAR_CLASS_WHITESPACE;
            return true;
        }
        // Not a list marker — fall through
    }

    // Try subject content: entire line ending with :
    if (valid_symbols[SUBJECT_CONTENT]) {
        lexer->mark_end(lexer);
        int32_t last_char = 0;
        while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
            last_char = lexer->lookahead;
            lexer->advance(lexer, false);
        }
        if (last_char == ':') {
            lexer->mark_end(lexer);
            lexer->result_symbol = SUBJECT_CONTENT;
            return true;
        }
        // Line doesn't end with : — return false, position resets
        return false;
    }

    // Check for mid-line annotation marker (the second ::)
    if (valid_symbols[ANNOTATION_MARKER] && lexer->lookahead == ':') {
        lexer->mark_end(lexer);
        lexer->advance(lexer, false);
        if (lexer->lookahead == ':') {
            lexer->advance(lexer, false);
            lexer->mark_end(lexer);
            // Skip trailing space after ::
            if (lexer->lookahead == ' ') {
                lexer->advance(lexer, false);
                lexer->mark_end(lexer);
            }
            lexer->result_symbol = ANNOTATION_MARKER;
            return true;
        }
        // Single colon — not an annotation marker, don't consume
        return false;
    }

    // ===== Emphasis delimiter detection =====

    // Opening * for strong
    if (valid_symbols[STRONG_OPEN] && lexer->lookahead == '*') {
        // Flanking: prev must not be word char, next must be word char
        if (scanner->last_char_class != CHAR_CLASS_WORD) {
            lexer->mark_end(lexer);
            lexer->advance(lexer, false);
            if (classify_char(lexer->lookahead) == CHAR_CLASS_WORD) {
                // Check that a matching * exists on the rest of the line.
                // Without this, unclosed * (e.g., "*List..." or "mass*accel")
                // would emit _strong_open and produce ERROR nodes.
                lexer->mark_end(lexer);
                if (has_matching_closer(lexer, '*')) {
                    lexer->result_symbol = STRONG_OPEN;
                    scanner->last_char_class = CHAR_CLASS_PUNCTUATION;
                    return true;
                }
                // No closer found — don't emit, let grammar handle * as text
            }
        }
    }

    // Closing * for strong
    if (valid_symbols[STRONG_CLOSE] && lexer->lookahead == '*') {
        // Flanking: prev must not be whitespace/none, next must not be word char
        if (scanner->last_char_class != CHAR_CLASS_NONE &&
            scanner->last_char_class != CHAR_CLASS_WHITESPACE) {
            lexer->mark_end(lexer);
            lexer->advance(lexer, false);
            if (classify_char(lexer->lookahead) != CHAR_CLASS_WORD) {
                lexer->mark_end(lexer);
                lexer->result_symbol = STRONG_CLOSE;
                scanner->last_char_class = CHAR_CLASS_PUNCTUATION;
                return true;
            }
        }
    }

    // Opening _ for emphasis
    if (valid_symbols[EMPHASIS_OPEN] && lexer->lookahead == '_') {
        if (scanner->last_char_class != CHAR_CLASS_WORD) {
            lexer->mark_end(lexer);
            lexer->advance(lexer, false);
            if (classify_char(lexer->lookahead) == CHAR_CLASS_WORD) {
                // Check that a matching _ exists on the rest of the line
                lexer->mark_end(lexer);
                if (has_matching_closer(lexer, '_')) {
                    lexer->result_symbol = EMPHASIS_OPEN;
                    scanner->last_char_class = CHAR_CLASS_PUNCTUATION;
                    return true;
                }
                // No closer found — don't emit, let grammar handle _ as text
            }
        }
    }

    // Closing _ for emphasis
    if (valid_symbols[EMPHASIS_CLOSE] && lexer->lookahead == '_') {
        if (scanner->last_char_class != CHAR_CLASS_NONE &&
            scanner->last_char_class != CHAR_CLASS_WHITESPACE) {
            lexer->mark_end(lexer);
            lexer->advance(lexer, false);
            if (classify_char(lexer->lookahead) != CHAR_CLASS_WORD) {
                lexer->mark_end(lexer);
                lexer->result_symbol = EMPHASIS_CLOSE;
                scanner->last_char_class = CHAR_CLASS_PUNCTUATION;
                return true;
            }
        }
    }

    // NEWLINE or EOF
    if (valid_symbols[NEWLINE]) {
        if (lexer->lookahead == '\n') {
            lexer->advance(lexer, false);
            lexer->result_symbol = NEWLINE;
            scanner->at_line_start = true;
            scanner->indent_measured = false;
            scanner->last_char_class = CHAR_CLASS_NONE;
            return true;
        }
        if (lexer->eof(lexer) && !scanner->emitted_eof_newline) {
            scanner->emitted_eof_newline = true;
            lexer->result_symbol = NEWLINE;
            scanner->at_line_start = true;
            scanner->indent_measured = false;
            scanner->last_char_class = CHAR_CLASS_NONE;
            return true;
        }
    }

    // No external token matched. The grammar lexer will consume the next token.
    // Infer last_char_class from lookahead — because the grammar's _word is split
    // into _word_alnum (ends with alnum → WORD), _word_space (ends with space →
    // WHITESPACE), and _word_other (ends with punct → PUNCTUATION), and all other
    // grammar inline tokens end with punctuation, classify_char(lookahead) predicts
    // the last-character class of whatever token the grammar will consume.
    scanner->last_char_class = classify_char(lexer->lookahead);

    return false;
}
