/**
 * External scanner for tree-sitter-lex.
 *
 * Handles:
 * - Indentation-based structure (INDENT/DEDENT tokens)
 * - NEWLINE emission at line boundaries and EOF
 * - Line-start detection for annotation markers (::) and list items (full line)
 *
 * Lex uses 4-space indentation units (or 1 tab = 1 level).
 *
 * Externals order (must match grammar.js):
 *   0: _indent
 *   1: _dedent
 *   2: _newline
 *   3: annotation_marker
 *   4: annotation_end_marker
 *   5: list_item_line
 */

#include "tree_sitter/parser.h"

#include <string.h>

#define MAX_INDENT_DEPTH 64
#define INDENT_WIDTH 4

enum TokenType {
    INDENT,
    DEDENT,
    NEWLINE,
    ANNOTATION_MARKER,
    ANNOTATION_END_MARKER,
    LIST_ITEM_LINE,
};

typedef struct {
    int indent_stack[MAX_INDENT_DEPTH];
    int indent_depth;
    int pending_dedents;
    bool at_line_start;
    bool emitted_eof_newline; // prevents infinite NEWLINE emission at EOF
} Scanner;

void *tree_sitter_lex_external_scanner_create(void) {
    Scanner *scanner = calloc(1, sizeof(Scanner));
    scanner->indent_stack[0] = 0;
    scanner->indent_depth = 0;
    scanner->pending_dedents = 0;
    scanner->at_line_start = true;
    scanner->emitted_eof_newline = false;
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
        return;
    }

    unsigned offset = 0;
    scanner->indent_depth = (int)(unsigned char)buffer[offset++];
    scanner->pending_dedents = (int)(unsigned char)buffer[offset++];
    scanner->at_line_start = (bool)buffer[offset++];
    scanner->emitted_eof_newline = (bool)buffer[offset++];

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

bool tree_sitter_lex_external_scanner_scan(void *payload, TSLexer *lexer,
                                            const bool *valid_symbols) {
    Scanner *scanner = (Scanner *)payload;

    // Emit pending DEDENT tokens
    if (scanner->pending_dedents > 0 && valid_symbols[DEDENT]) {
        scanner->pending_dedents--;
        lexer->result_symbol = DEDENT;
        return true;
    }

    // At line start, calculate indentation and detect line-start tokens
    if (scanner->at_line_start) {
        int indent = 0;
        while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            if (lexer->lookahead == '\t') {
                indent += INDENT_WIDTH;
            } else {
                indent++;
            }
            lexer->advance(lexer, true);
        }

        // Blank line — emit NEWLINE
        if (lexer->lookahead == '\n') {
            if (valid_symbols[NEWLINE]) {
                lexer->advance(lexer, false);
                lexer->result_symbol = NEWLINE;
                scanner->at_line_start = true;
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

        // After handling indentation, try to detect line-start tokens.
        lexer->mark_end(lexer);

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

        // Try list item line: marker + rest of line (full line token)
        if (valid_symbols[LIST_ITEM_LINE]) {
            if (try_list_marker(lexer)) {
                // Marker matched — consume the rest of the line
                consume_rest_of_line(lexer);
                lexer->mark_end(lexer);
                lexer->result_symbol = LIST_ITEM_LINE;
                return true;
            }
            // Not a list marker — let grammar lexer handle it
            return false;
        }

        return false;
    }

    // Not at line start — check for mid-line annotation marker (the second ::)
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

    // Not at line start — look for NEWLINE or EOF
    if (valid_symbols[NEWLINE]) {
        if (lexer->lookahead == '\n') {
            lexer->advance(lexer, false);
            lexer->result_symbol = NEWLINE;
            scanner->at_line_start = true;
            return true;
        }
        // At EOF without trailing newline — emit one synthetic NEWLINE
        if (lexer->eof(lexer) && !scanner->emitted_eof_newline) {
            scanner->emitted_eof_newline = true;
            lexer->result_symbol = NEWLINE;
            scanner->at_line_start = true;
            return true;
        }
    }

    return false;
}
