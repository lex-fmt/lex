//! Inline character escaping for Lex content.
//!
//! Inline Escaping Rules:
//!   - Backslash before non-alphanumeric: escapes the character (backslash removed)
//!   - Backslash before alphanumeric: backslash preserved (for paths like C:\Users)
//!   - Double backslash (\\): produces a single backslash
//!   - Trailing backslash at end of input: preserved

/// Result of processing a backslash at position `i` in a character stream.
pub enum EscapeAction {
    /// Backslash escapes the next character; consume 2 chars, emit the given char.
    Escape(char),
    /// Backslash is literal (before alphanumeric or at end); consume 1 char, emit `\`.
    Literal,
}

/// Decide what to do with a backslash at the current position.
///
/// `next` is the character after the backslash, if any.
/// Used by the inline parser to handle escapes character-by-character.
pub fn unescape_inline_char(next: Option<char>) -> EscapeAction {
    match next {
        Some(ch) if !ch.is_alphanumeric() => EscapeAction::Escape(ch),
        _ => EscapeAction::Literal,
    }
}

/// Process escape sequences in inline text content.
///
/// Applies backslash escaping rules:
/// - `\*` → `*` (non-alphanumeric: escape removes backslash)
/// - `\n` → `\n` (alphanumeric: backslash preserved, where n is a letter)
/// - `\\` → `\` (backslash is non-alphanumeric, so it escapes itself)
/// - trailing `\` → `\` (no character follows, preserved)
pub fn unescape_inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\\' {
            if let Some(&next) = chars.get(i + 1) {
                if next.is_alphanumeric() {
                    // Preserve backslash before alphanumeric (e.g. C:\Users)
                    result.push('\\');
                    i += 1;
                } else {
                    // Escape: consume backslash, emit next char
                    result.push(next);
                    i += 2;
                }
            } else {
                // Trailing backslash: preserve
                result.push('\\');
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Escape special inline characters so they won't be parsed as inline markup.
///
/// This is the inverse of `unescape_inline`: given plain text, produce escaped text
/// that round-trips through unescape back to the original.
///
/// Escapes: `\`, `*`, `_`, `` ` ``, `#`, `[`, `]`
pub fn escape_inline(text: &str) -> String {
    let mut result = String::with_capacity(text.len());

    for ch in text.chars() {
        if is_inline_special(ch) {
            result.push('\\');
        }
        result.push(ch);
    }

    result
}

/// Characters that have special meaning in inline parsing and need escaping.
fn is_inline_special(ch: char) -> bool {
    matches!(ch, '\\' | '*' | '_' | '`' | '#' | '[' | ']')
}

/// Check whether the token immediately before a `Quote` is a `Text` ending
/// with an odd number of backslashes, which means the quote is escaped.
pub(super) fn is_quote_escaped_by_prev_token(prev: Option<&crate::lex::token::Token>) -> bool {
    use crate::lex::token::Token;
    match prev {
        Some(Token::Text(s)) => {
            let trailing = s.bytes().rev().take_while(|&b| b == b'\\').count();
            trailing % 2 == 1
        }
        _ => false,
    }
}
