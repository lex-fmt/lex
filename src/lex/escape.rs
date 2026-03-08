//! Centralized escape/unescape logic for Lex inline content
//!
//! Rules:
//!   - Backslash before non-alphanumeric: escapes the character (backslash removed)
//!   - Backslash before alphanumeric: backslash preserved (for paths like C:\Users)
//!   - Double backslash (\\): produces a single backslash
//!   - Trailing backslash at end of input: preserved
//!
//! These rules apply to inline text content only. Verbatim blocks and labels
//! have no character-level escaping.

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

#[cfg(test)]
mod tests {
    use super::*;

    // --- unescape_inline ---

    #[test]
    fn unescape_plain_text_unchanged() {
        assert_eq!(unescape_inline("hello world"), "hello world");
    }

    #[test]
    fn unescape_empty_string() {
        assert_eq!(unescape_inline(""), "");
    }

    #[test]
    fn unescape_asterisk() {
        assert_eq!(unescape_inline("\\*literal\\*"), "*literal*");
    }

    #[test]
    fn unescape_underscore() {
        assert_eq!(unescape_inline("\\_not emphasis\\_"), "_not emphasis_");
    }

    #[test]
    fn unescape_backtick() {
        assert_eq!(unescape_inline("\\`not code\\`"), "`not code`");
    }

    #[test]
    fn unescape_hash() {
        assert_eq!(unescape_inline("\\#not math\\#"), "#not math#");
    }

    #[test]
    fn unescape_brackets() {
        assert_eq!(unescape_inline("\\[not a ref\\]"), "[not a ref]");
    }

    #[test]
    fn unescape_backslash_before_alphanumeric_preserved() {
        assert_eq!(unescape_inline("C:\\Users\\name"), "C:\\Users\\name");
    }

    #[test]
    fn unescape_double_backslash() {
        assert_eq!(unescape_inline("C:\\\\Users\\\\name"), "C:\\Users\\name");
    }

    #[test]
    fn unescape_trailing_backslash() {
        assert_eq!(unescape_inline("text\\"), "text\\");
    }

    #[test]
    fn unescape_backslash_before_space() {
        assert_eq!(unescape_inline("hello\\ world"), "hello world");
    }

    #[test]
    fn unescape_backslash_before_punctuation() {
        assert_eq!(unescape_inline("\\!\\?\\,\\."), "!?,.");
    }

    #[test]
    fn unescape_multiple_consecutive_backslashes() {
        // \\\\ = 4 backslashes → 2 backslashes (each pair escapes to one)
        assert_eq!(unescape_inline("\\\\\\\\"), "\\\\");
    }

    #[test]
    fn unescape_triple_backslash_then_star() {
        // \\\\\\* = \\\* → \\ produces \, then \* produces *
        assert_eq!(unescape_inline("\\\\\\*"), "\\*");
    }

    #[test]
    fn unescape_mixed_escaped_and_plain() {
        assert_eq!(
            unescape_inline("plain \\*escaped\\* plain"),
            "plain *escaped* plain"
        );
    }

    #[test]
    fn unescape_backslash_before_digit_preserved() {
        assert_eq!(unescape_inline("item\\1"), "item\\1");
    }

    #[test]
    fn unescape_backslash_before_unicode_letter_preserved() {
        assert_eq!(unescape_inline("path\\ñ"), "path\\ñ");
    }

    #[test]
    fn unescape_backslash_before_non_ascii_symbol() {
        // Non-alphanumeric non-ASCII: backslash removed
        assert_eq!(unescape_inline("\\→"), "→");
    }

    // --- escape_inline ---

    #[test]
    fn escape_plain_text_unchanged() {
        assert_eq!(escape_inline("hello world"), "hello world");
    }

    #[test]
    fn escape_empty_string() {
        assert_eq!(escape_inline(""), "");
    }

    #[test]
    fn escape_special_chars() {
        assert_eq!(escape_inline("*bold*"), "\\*bold\\*");
        assert_eq!(escape_inline("_emph_"), "\\_emph\\_");
        assert_eq!(escape_inline("`code`"), "\\`code\\`");
        assert_eq!(escape_inline("#math#"), "\\#math\\#");
        assert_eq!(escape_inline("[ref]"), "\\[ref\\]");
    }

    #[test]
    fn escape_backslash() {
        assert_eq!(escape_inline("C:\\Users"), "C:\\\\Users");
    }

    // --- roundtrip ---

    #[test]
    fn roundtrip_plain_text() {
        let original = "hello world";
        assert_eq!(unescape_inline(&escape_inline(original)), original);
    }

    #[test]
    fn roundtrip_special_chars() {
        let original = "*bold* and _emph_ and `code` and #math# and [ref]";
        assert_eq!(unescape_inline(&escape_inline(original)), original);
    }

    #[test]
    fn roundtrip_backslashes() {
        let original = "C:\\Users\\name";
        assert_eq!(unescape_inline(&escape_inline(original)), original);
    }

    #[test]
    fn roundtrip_mixed() {
        let original = "path\\file *bold* and \\more";
        assert_eq!(unescape_inline(&escape_inline(original)), original);
    }
}
