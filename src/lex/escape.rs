//! Centralized escape/unescape logic for Lex content
//!
//! Inline Escaping Rules:
//!   - Backslash before non-alphanumeric: escapes the character (backslash removed)
//!   - Backslash before alphanumeric: backslash preserved (for paths like C:\Users)
//!   - Double backslash (\\): produces a single backslash
//!   - Trailing backslash at end of input: preserved
//!
//! Quoted Parameter Value Escaping Rules:
//!   - `\"` inside a quoted value: literal quote (backslash removed)
//!   - `\\` inside a quoted value: literal backslash
//!   - Only `"` and `\` can be escaped; other backslashes are literal
//!
//! Verbatim blocks and labels have no character-level escaping.

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

// --- Structural marker detection ---

/// Check whether the token immediately before a `Quote` is a `Text` ending
/// with an odd number of backslashes, which means the quote is escaped.
fn is_quote_escaped_by_prev_token(prev: Option<&crate::lex::token::Token>) -> bool {
    use crate::lex::token::Token;
    match prev {
        Some(Token::Text(s)) => {
            let trailing = s.bytes().rev().take_while(|&b| b == b'\\').count();
            trailing % 2 == 1
        }
        _ => false,
    }
}

/// Find positions of `LexMarker` tokens that are NOT inside a quoted context.
///
/// Tracks quote state by toggling on each `Quote` token. LexMarkers inside
/// quoted regions are treated as content, not structural delimiters.
/// Escaped quotes (`\"`) do not toggle quote state.
///
/// Works with bare `Token` slices (no byte ranges needed).
pub fn find_structural_lex_markers(tokens: &[crate::lex::token::Token]) -> Vec<usize> {
    use crate::lex::token::Token;
    let mut markers = Vec::new();
    let mut in_quotes = false;
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::Quote => {
                if !is_quote_escaped_by_prev_token(if i > 0 { Some(&tokens[i - 1]) } else { None })
                {
                    in_quotes = !in_quotes;
                }
            }
            Token::LexMarker if !in_quotes => markers.push(i),
            _ => {}
        }
    }
    markers
}

/// Find positions of structural `LexMarker` tokens in a paired token/span slice.
///
/// Same logic as `find_structural_lex_markers` but for `(Token, Range)` pairs.
/// Escaped quotes (`\"`) do not toggle quote state.
pub fn find_structural_lex_marker_pairs<R>(tokens: &[(crate::lex::token::Token, R)]) -> Vec<usize> {
    use crate::lex::token::Token;
    let mut markers = Vec::new();
    let mut in_quotes = false;
    for (i, (token, _)) in tokens.iter().enumerate() {
        match token {
            Token::Quote => {
                let prev = if i > 0 { Some(&tokens[i - 1].0) } else { None };
                if !is_quote_escaped_by_prev_token(prev) {
                    in_quotes = !in_quotes;
                }
            }
            Token::LexMarker if !in_quotes => markers.push(i),
            _ => {}
        }
    }
    markers
}

// --- Quoted parameter value escaping ---

/// Check whether a quote at `pos` in `source` is escaped by a preceding backslash.
///
/// Correctly handles chains of backslashes: `\\"` is NOT escaped (even backslashes),
/// `\\\"` IS escaped (odd backslashes before the quote).
pub fn is_quote_escaped(source: &[u8], pos: usize) -> bool {
    let mut backslash_count = 0;
    let mut check = pos;
    while check > 0 && source[check - 1] == b'\\' {
        backslash_count += 1;
        check -= 1;
    }
    backslash_count % 2 == 1
}

/// Unescape a quoted parameter value.
///
/// Input should be the raw stored value including outer quotes (e.g., `"Hello World"`).
/// Returns the semantic content with escapes resolved and outer quotes stripped.
///
/// Escapes: `\"` → `"`, `\\` → `\`. Other backslashes are literal.
pub fn unescape_quoted(raw: &str) -> String {
    // Strip outer quotes if present
    let inner = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        &raw[1..raw.len() - 1]
    } else {
        raw
    };

    let mut result = String::with_capacity(inner.len());
    let chars: Vec<char> = inner.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\\' {
            if let Some(&next) = chars.get(i + 1) {
                if next == '"' || next == '\\' {
                    result.push(next);
                    i += 2;
                    continue;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Escape a string for use as a quoted parameter value.
///
/// Escapes `\` → `\\` and `"` → `\"`. Does NOT add outer quotes.
pub fn escape_quoted(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '\\' || ch == '"' {
            result.push('\\');
        }
        result.push(ch);
    }
    result
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

    // --- unescape_quoted ---

    #[test]
    fn unescape_quoted_simple() {
        assert_eq!(unescape_quoted("\"Hello World\""), "Hello World");
    }

    #[test]
    fn unescape_quoted_with_escaped_quote() {
        assert_eq!(unescape_quoted("\"say \\\"hello\\\"\""), "say \"hello\"");
    }

    #[test]
    fn unescape_quoted_with_escaped_backslash() {
        assert_eq!(unescape_quoted("\"path\\\\to\""), "path\\to");
    }

    #[test]
    fn unescape_quoted_escaped_backslash_before_quote() {
        // \\\\" = escaped backslash then real closing quote
        assert_eq!(unescape_quoted("\"end\\\\\""), "end\\");
    }

    #[test]
    fn unescape_quoted_other_backslash_literal() {
        // \n is not a recognized escape, backslash preserved
        assert_eq!(unescape_quoted("\"hello\\nworld\""), "hello\\nworld");
    }

    #[test]
    fn unescape_quoted_empty() {
        assert_eq!(unescape_quoted("\"\""), "");
    }

    #[test]
    fn unescape_quoted_no_quotes() {
        // Unquoted values pass through (backslash handling still applies)
        assert_eq!(unescape_quoted("simple"), "simple");
    }

    // --- escape_quoted ---

    #[test]
    fn escape_quoted_simple() {
        assert_eq!(escape_quoted("Hello World"), "Hello World");
    }

    #[test]
    fn escape_quoted_with_quote() {
        assert_eq!(escape_quoted("say \"hello\""), "say \\\"hello\\\"");
    }

    #[test]
    fn escape_quoted_with_backslash() {
        assert_eq!(escape_quoted("path\\to"), "path\\\\to");
    }

    #[test]
    fn escape_quoted_empty() {
        assert_eq!(escape_quoted(""), "");
    }

    // --- quoted roundtrip ---

    #[test]
    fn roundtrip_quoted_simple() {
        let original = "Hello World";
        let escaped = format!("\"{}\"", escape_quoted(original));
        assert_eq!(unescape_quoted(&escaped), original);
    }

    #[test]
    fn roundtrip_quoted_with_quotes() {
        let original = "say \"hello\" and \"bye\"";
        let escaped = format!("\"{}\"", escape_quoted(original));
        assert_eq!(unescape_quoted(&escaped), original);
    }

    #[test]
    fn roundtrip_quoted_with_backslashes() {
        let original = "C:\\Users\\name";
        let escaped = format!("\"{}\"", escape_quoted(original));
        assert_eq!(unescape_quoted(&escaped), original);
    }

    #[test]
    fn roundtrip_quoted_with_both() {
        let original = "path\\to \"file\"";
        let escaped = format!("\"{}\"", escape_quoted(original));
        assert_eq!(unescape_quoted(&escaped), original);
    }

    // --- is_quote_escaped ---

    #[test]
    fn is_quote_escaped_no_backslash() {
        assert!(!is_quote_escaped(b"hello\"", 5));
    }

    #[test]
    fn is_quote_escaped_single_backslash() {
        assert!(is_quote_escaped(b"hello\\\"", 6));
    }

    #[test]
    fn is_quote_escaped_double_backslash() {
        assert!(!is_quote_escaped(b"hello\\\\\"", 7));
    }

    #[test]
    fn is_quote_escaped_triple_backslash() {
        assert!(is_quote_escaped(b"hello\\\\\\\"", 8));
    }

    #[test]
    fn is_quote_escaped_at_start() {
        assert!(!is_quote_escaped(b"\"", 0));
    }

    // --- find_structural_lex_markers ---

    #[test]
    fn structural_markers_no_quotes() {
        use crate::lex::token::Token;
        let tokens = vec![
            Token::LexMarker,
            Token::Whitespace(1),
            Token::Text("note".into()),
            Token::Whitespace(1),
            Token::LexMarker,
        ];
        assert_eq!(find_structural_lex_markers(&tokens), vec![0, 4]);
    }

    #[test]
    fn structural_markers_with_quoted_marker() {
        use crate::lex::token::Token;
        // :: note foo=":: value" ::
        let tokens = vec![
            Token::LexMarker, // 0: structural
            Token::Whitespace(1),
            Token::Text("note".into()),
            Token::Whitespace(1),
            Token::Text("foo".into()),
            Token::Equals,
            Token::Quote,     // 6: opens quote
            Token::LexMarker, // 7: inside quotes — NOT structural
            Token::Whitespace(1),
            Token::Text("value".into()),
            Token::Quote, // 10: closes quote
            Token::Whitespace(1),
            Token::LexMarker, // 12: structural
        ];
        assert_eq!(find_structural_lex_markers(&tokens), vec![0, 12]);
    }

    #[test]
    fn structural_markers_data_line_with_quoted_marker() {
        use crate::lex::token::Token;
        // :: note foo=":: value"  (no closing ::)
        let tokens = vec![
            Token::LexMarker, // 0: structural
            Token::Whitespace(1),
            Token::Text("note".into()),
            Token::Equals,
            Token::Quote,
            Token::LexMarker, // inside quotes
            Token::Text("value".into()),
            Token::Quote,
        ];
        // Only one structural marker (opening)
        assert_eq!(find_structural_lex_markers(&tokens), vec![0]);
    }

    #[test]
    fn structural_markers_escaped_quote_does_not_toggle() {
        use crate::lex::token::Token;
        // :: note foo="value with \" inside" ::
        // The \" should NOT toggle quote state
        let tokens = vec![
            Token::LexMarker, // 0: structural
            Token::Whitespace(1),
            Token::Text("note".into()),
            Token::Whitespace(1),
            Token::Text("foo".into()),
            Token::Equals,
            Token::Quote,                        // 6: opens quote
            Token::Text("value with \\".into()), // 7: text ending in backslash
            Token::Quote,                        // 8: escaped quote (preceded by \)
            Token::Text(" inside".into()),       // 9
            Token::Quote,                        // 10: real closing quote
            Token::Whitespace(1),
            Token::LexMarker, // 12: structural
        ];
        assert_eq!(find_structural_lex_markers(&tokens), vec![0, 12]);
    }

    #[test]
    fn structural_markers_double_backslash_before_quote_not_escaped() {
        use crate::lex::token::Token;
        // :: note foo="val\\" ::
        // \\\\ (double backslash) before quote means the backslashes escape each other,
        // so the quote IS a real closing quote
        let tokens = vec![
            Token::LexMarker, // 0: structural
            Token::Whitespace(1),
            Token::Text("note".into()),
            Token::Whitespace(1),
            Token::Text("foo".into()),
            Token::Equals,
            Token::Quote,                  // 6: opens quote
            Token::Text("val\\\\".into()), // 7: text ending in \\
            Token::Quote,                  // 8: real closing quote (even backslashes)
            Token::Whitespace(1),
            Token::LexMarker, // 10: structural
        ];
        assert_eq!(find_structural_lex_markers(&tokens), vec![0, 10]);
    }

    #[test]
    fn is_quote_escaped_by_prev_token_tests() {
        use crate::lex::token::Token;
        // No prev token
        assert!(!is_quote_escaped_by_prev_token(None));
        // Non-text prev
        assert!(!is_quote_escaped_by_prev_token(Some(&Token::Whitespace(1))));
        // Text not ending in backslash
        assert!(!is_quote_escaped_by_prev_token(Some(&Token::Text(
            "hello".into()
        ))));
        // Text ending in single backslash (escaped)
        assert!(is_quote_escaped_by_prev_token(Some(&Token::Text(
            "hello\\".into()
        ))));
        // Text ending in double backslash (not escaped)
        assert!(!is_quote_escaped_by_prev_token(Some(&Token::Text(
            "hello\\\\".into()
        ))));
        // Text ending in triple backslash (escaped)
        assert!(is_quote_escaped_by_prev_token(Some(&Token::Text(
            "hello\\\\\\".into()
        ))));
    }
}
