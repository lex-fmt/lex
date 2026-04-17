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
//! Structural Scanner Rules (for split/find on structural delimiters like `|`, `,`, `;`):
//!   - `\<sep>` is treated as a literal character (not a split point);
//!     the escaping backslash is stripped in the returned segment text.
//!   - `\\<sep>` counts as an escaped backslash followed by a structural `<sep>`
//!     (even number of backslashes → `<sep>` is structural).
//!   - Optionally, content inside balanced `literal_delim` pairs (e.g. backticks)
//!     is passed through verbatim: no split, no backslash stripping.
//!
//! Verbatim blocks and labels have no character-level escaping.

use std::borrow::Cow;

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

// --- Escape-aware structural scanners ---

/// Count trailing backslashes immediately preceding byte position `pos` in `bytes`.
///
/// Returns 0 if `pos == 0` or the byte at `pos - 1` is not `\`.
fn trailing_backslashes_before(bytes: &[u8], pos: usize) -> usize {
    let mut n = 0usize;
    let mut i = pos;
    while i > 0 && bytes[i - 1] == b'\\' {
        n += 1;
        i -= 1;
    }
    n
}

/// Check whether the byte at `pos` in `bytes` is a structural delimiter — i.e.,
/// not preceded by an odd number of backslashes and (if `literal_delim` is provided)
/// not inside a balanced pair of literal delimiters starting from byte 0.
///
/// Literal delimiters nest flat (balanced pairs, no nesting), matching how
/// backtick-delimited inline code behaves in Lex.
pub fn is_structural_at(bytes: &[u8], pos: usize, literal_delim: Option<u8>) -> bool {
    if pos >= bytes.len() {
        return false;
    }
    // Escape check: odd backslashes before pos → escaped.
    if trailing_backslashes_before(bytes, pos) % 2 == 1 {
        return false;
    }
    // Literal-region check: scan from start, toggling on unescaped literal_delim.
    if let Some(delim) = literal_delim {
        let mut in_literal = false;
        let mut i = 0;
        while i < pos {
            if bytes[i] == delim && trailing_backslashes_before(bytes, i) % 2 == 0 {
                in_literal = !in_literal;
            }
            i += 1;
        }
        if in_literal {
            return false;
        }
    }
    true
}

/// Splits `s` on `sep`, treating `\<sep>` as a literal character (not a split point)
/// and stripping the escaping backslash from the returned segment text.
///
/// Returns `Cow::Borrowed` for segments with no escapes to strip (no allocation),
/// `Cow::Owned` otherwise.
///
/// Semantics:
/// - `\<sep>` → literal `<sep>` inside a segment, no split.
/// - `\\<sep>` → literal `\` followed by structural `<sep>` → splits.
/// - Other `\X` sequences inside a segment are preserved as-is (this scanner
///   only resolves `\<sep>`; inline-level escape resolution is a separate pass).
pub fn split_respecting_escape(s: &str, sep: char) -> Vec<Cow<'_, str>> {
    split_inner(s, sep, None)
}

/// Like [`split_respecting_escape`] but additionally treats content inside
/// balanced `literal_delim` pairs as non-splittable. Separators and escapes
/// inside literal regions are passed through verbatim.
pub fn split_respecting_escape_and_literals(
    s: &str,
    sep: char,
    literal_delim: char,
) -> Vec<Cow<'_, str>> {
    split_inner(s, sep, Some(literal_delim))
}

/// Like [`split_respecting_escape_and_literals`] but also returns the byte range
/// of each segment within the input (pre-strip, pre-trim positions).
///
/// Useful for parsers that need source-position tracking (e.g. diagnostic spans).
pub fn split_respecting_escape_with_ranges<'a>(
    s: &'a str,
    sep: char,
    literal_delim: Option<char>,
) -> Vec<(Cow<'a, str>, std::ops::Range<usize>)> {
    split_with_ranges_inner(s, sep, literal_delim)
}

/// Find the first position of `needle` in `s` that is structural — i.e.,
/// not preceded by an odd number of backslashes and (if `literal_delim` is
/// provided) not inside a balanced literal region.
///
/// Returns a byte offset into `s`.
pub fn find_respecting_escape(s: &str, needle: char) -> Option<usize> {
    find_inner(s, needle, None)
}

/// Like [`find_respecting_escape`] but respects balanced `literal_delim` regions.
pub fn find_respecting_escape_and_literals(
    s: &str,
    needle: char,
    literal_delim: char,
) -> Option<usize> {
    find_inner(s, needle, Some(literal_delim))
}

fn split_inner(s: &str, sep: char, literal_delim: Option<char>) -> Vec<Cow<'_, str>> {
    if s.is_empty() {
        return vec![Cow::Borrowed("")];
    }
    let bytes = s.as_bytes();
    let sep_is_ascii = sep.is_ascii();
    let literal_is_ascii = literal_delim.is_none_or(|c| c.is_ascii());
    // Fast path: if sep and literal_delim are ASCII, we can scan bytes directly.
    // Otherwise, fall back to char iteration.
    if sep_is_ascii && literal_is_ascii {
        split_inner_ascii(s, bytes, sep as u8, literal_delim.map(|c| c as u8))
    } else {
        split_inner_chars(s, sep, literal_delim)
    }
}

fn split_inner_ascii<'a>(
    s: &'a str,
    bytes: &[u8],
    sep: u8,
    literal_delim: Option<u8>,
) -> Vec<Cow<'a, str>> {
    let mut segments = Vec::new();
    let mut seg_start = 0usize;
    let mut in_literal = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(delim) = literal_delim {
            if b == delim && trailing_backslashes_before(bytes, i) % 2 == 0 {
                in_literal = !in_literal;
                i += 1;
                continue;
            }
        }
        if !in_literal && b == sep && trailing_backslashes_before(bytes, i) % 2 == 0 {
            segments.push(extract_segment(s, seg_start, i, sep, literal_delim));
            seg_start = i + 1;
        }
        i += 1;
    }
    segments.push(extract_segment(
        s,
        seg_start,
        bytes.len(),
        sep,
        literal_delim,
    ));
    segments
}

fn split_inner_chars<'a>(s: &'a str, sep: char, literal_delim: Option<char>) -> Vec<Cow<'a, str>> {
    let mut segments = Vec::new();
    let mut seg_start = 0usize;
    let mut in_literal = false;
    let mut prev_backslashes = 0usize;
    for (i, ch) in s.char_indices() {
        let is_escaped = prev_backslashes % 2 == 1;
        if let Some(delim) = literal_delim {
            if ch == delim && !is_escaped {
                in_literal = !in_literal;
                prev_backslashes = 0;
                continue;
            }
        }
        if !in_literal && ch == sep && !is_escaped {
            segments.push(extract_segment_char(s, seg_start, i, sep, literal_delim));
            seg_start = i + ch.len_utf8();
            prev_backslashes = 0;
            continue;
        }
        if ch == '\\' {
            prev_backslashes += 1;
        } else {
            prev_backslashes = 0;
        }
    }
    segments.push(extract_segment_char(
        s,
        seg_start,
        s.len(),
        sep,
        literal_delim,
    ));
    segments
}

/// Extract `s[start..end]` and strip escaping backslashes for `\<sep>` sequences
/// that occur outside literal regions.
fn extract_segment<'a>(
    s: &'a str,
    start: usize,
    end: usize,
    sep: u8,
    literal_delim: Option<u8>,
) -> Cow<'a, str> {
    let slice = &s[start..end];
    // Quick check: only need to strip if we find `\<sep>` outside any literal region.
    if !needs_strip_ascii(slice.as_bytes(), sep, literal_delim) {
        return Cow::Borrowed(slice);
    }
    Cow::Owned(strip_escapes_ascii(slice.as_bytes(), sep, literal_delim))
}

fn extract_segment_char<'a>(
    s: &'a str,
    start: usize,
    end: usize,
    sep: char,
    literal_delim: Option<char>,
) -> Cow<'a, str> {
    let slice = &s[start..end];
    if !needs_strip_char(slice, sep, literal_delim) {
        return Cow::Borrowed(slice);
    }
    Cow::Owned(strip_escapes_char(slice, sep, literal_delim))
}

fn needs_strip_ascii(bytes: &[u8], sep: u8, literal_delim: Option<u8>) -> bool {
    let mut in_literal = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(delim) = literal_delim {
            if b == delim && trailing_backslashes_before(bytes, i) % 2 == 0 {
                in_literal = !in_literal;
                i += 1;
                continue;
            }
        }
        if !in_literal && b == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == sep {
            return true;
        }
        i += 1;
    }
    false
}

fn strip_escapes_ascii(bytes: &[u8], sep: u8, literal_delim: Option<u8>) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut in_literal = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(delim) = literal_delim {
            if b == delim && trailing_backslashes_before(bytes, i) % 2 == 0 {
                in_literal = !in_literal;
                out.push(b);
                i += 1;
                continue;
            }
        }
        if !in_literal && b == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == sep {
            out.push(sep);
            i += 2;
            continue;
        }
        out.push(b);
        i += 1;
    }
    // Safe: input was valid UTF-8 and we only delete whole ASCII `\` bytes or
    // replace ASCII `\<sep>` with ASCII `<sep>`. Non-ASCII multi-byte sequences
    // are copied byte-for-byte intact, so the result stays valid UTF-8.
    String::from_utf8(out).expect("byte-level manipulations preserve UTF-8 validity")
}

fn needs_strip_char(slice: &str, sep: char, literal_delim: Option<char>) -> bool {
    let chars: Vec<char> = slice.chars().collect();
    let mut in_literal = false;
    let mut prev_backslashes = 0usize;
    for (i, &ch) in chars.iter().enumerate() {
        let is_escaped = prev_backslashes % 2 == 1;
        if let Some(delim) = literal_delim {
            if ch == delim && !is_escaped {
                in_literal = !in_literal;
                prev_backslashes = 0;
                continue;
            }
        }
        if !in_literal && ch == '\\' && chars.get(i + 1).copied() == Some(sep) {
            return true;
        }
        if ch == '\\' {
            prev_backslashes += 1;
        } else {
            prev_backslashes = 0;
        }
    }
    false
}

fn strip_escapes_char(slice: &str, sep: char, literal_delim: Option<char>) -> String {
    let chars: Vec<char> = slice.chars().collect();
    let mut out = String::with_capacity(slice.len());
    let mut in_literal = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if let Some(delim) = literal_delim {
            if ch == delim {
                in_literal = !in_literal;
                out.push(ch);
                i += 1;
                continue;
            }
        }
        if !in_literal && ch == '\\' && chars.get(i + 1).copied() == Some(sep) {
            out.push(sep);
            i += 2;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

fn split_with_ranges_inner<'a>(
    s: &'a str,
    sep: char,
    literal_delim: Option<char>,
) -> Vec<(Cow<'a, str>, std::ops::Range<usize>)> {
    if s.is_empty() {
        return vec![(Cow::Borrowed(""), 0..0)];
    }
    let bytes = s.as_bytes();
    let sep_is_ascii = sep.is_ascii();
    let literal_is_ascii = literal_delim.is_none_or(|c| c.is_ascii());
    if sep_is_ascii && literal_is_ascii {
        let mut segments = Vec::new();
        let mut seg_start = 0usize;
        let mut in_literal = false;
        let mut i = 0usize;
        let sep_byte = sep as u8;
        let literal_byte = literal_delim.map(|c| c as u8);
        while i < bytes.len() {
            let b = bytes[i];
            if let Some(delim) = literal_byte {
                if b == delim && trailing_backslashes_before(bytes, i) % 2 == 0 {
                    in_literal = !in_literal;
                    i += 1;
                    continue;
                }
            }
            if !in_literal && b == sep_byte && trailing_backslashes_before(bytes, i) % 2 == 0 {
                let seg = extract_segment(s, seg_start, i, sep_byte, literal_byte);
                segments.push((seg, seg_start..i));
                seg_start = i + 1;
            }
            i += 1;
        }
        let seg = extract_segment(s, seg_start, bytes.len(), sep_byte, literal_byte);
        segments.push((seg, seg_start..bytes.len()));
        segments
    } else {
        let mut segments = Vec::new();
        let mut seg_start = 0usize;
        let mut in_literal = false;
        let mut prev_backslashes = 0usize;
        for (i, ch) in s.char_indices() {
            let is_escaped = prev_backslashes % 2 == 1;
            if let Some(delim) = literal_delim {
                if ch == delim && !is_escaped {
                    in_literal = !in_literal;
                    prev_backslashes = 0;
                    continue;
                }
            }
            if !in_literal && ch == sep && !is_escaped {
                let seg = extract_segment_char(s, seg_start, i, sep, literal_delim);
                segments.push((seg, seg_start..i));
                seg_start = i + ch.len_utf8();
                prev_backslashes = 0;
                continue;
            }
            if ch == '\\' {
                prev_backslashes += 1;
            } else {
                prev_backslashes = 0;
            }
        }
        let seg = extract_segment_char(s, seg_start, s.len(), sep, literal_delim);
        segments.push((seg, seg_start..s.len()));
        segments
    }
}

fn find_inner(s: &str, needle: char, literal_delim: Option<char>) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_literal = false;
    for (i, ch) in s.char_indices() {
        if let Some(delim) = literal_delim {
            if ch == delim && trailing_backslashes_before(bytes, i) % 2 == 0 {
                in_literal = !in_literal;
                continue;
            }
        }
        if !in_literal && ch == needle && trailing_backslashes_before(bytes, i) % 2 == 0 {
            return Some(i);
        }
    }
    None
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

    // --- split_respecting_escape ---

    fn collect(segments: Vec<Cow<'_, str>>) -> Vec<String> {
        segments.into_iter().map(|s| s.into_owned()).collect()
    }

    #[test]
    fn split_no_separator() {
        assert_eq!(
            collect(split_respecting_escape("hello", '|')),
            vec!["hello"]
        );
    }

    #[test]
    fn split_empty_input() {
        assert_eq!(collect(split_respecting_escape("", '|')), vec![""]);
    }

    #[test]
    fn split_simple() {
        assert_eq!(
            collect(split_respecting_escape("a|b|c", '|')),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn split_trailing_empty() {
        assert_eq!(
            collect(split_respecting_escape("a|b|", '|')),
            vec!["a", "b", ""]
        );
    }

    #[test]
    fn split_leading_empty() {
        assert_eq!(
            collect(split_respecting_escape("|a|b", '|')),
            vec!["", "a", "b"]
        );
    }

    #[test]
    fn split_only_separators() {
        assert_eq!(
            collect(split_respecting_escape("|||", '|')),
            vec!["", "", "", ""]
        );
    }

    #[test]
    fn split_escaped_separator() {
        assert_eq!(
            collect(split_respecting_escape("a\\|b|c", '|')),
            vec!["a|b", "c"]
        );
    }

    #[test]
    fn split_double_backslash_then_sep_splits() {
        // \\| → literal \ + structural |
        assert_eq!(
            collect(split_respecting_escape("a\\\\|b", '|')),
            vec!["a\\\\", "b"]
        );
    }

    #[test]
    fn split_triple_backslash_then_sep_is_escaped() {
        // \\\| → literal \ + escaped |
        assert_eq!(
            collect(split_respecting_escape("a\\\\\\|b", '|')),
            vec!["a\\\\|b"]
        );
    }

    #[test]
    fn split_multiple_escapes_in_one_segment() {
        assert_eq!(
            collect(split_respecting_escape("\\|a\\|b\\|", '|')),
            vec!["|a|b|"]
        );
    }

    #[test]
    fn split_trailing_backslash_no_sep() {
        assert_eq!(
            collect(split_respecting_escape("abc\\", '|')),
            vec!["abc\\"]
        );
    }

    #[test]
    fn split_preserves_unrelated_backslashes() {
        // `\n` is unrelated to `|`, preserved verbatim (inline-level escaping handles that).
        assert_eq!(
            collect(split_respecting_escape("a\\n|b", '|')),
            vec!["a\\n", "b"]
        );
    }

    #[test]
    fn split_different_separator() {
        assert_eq!(
            collect(split_respecting_escape("a,b\\,c,d", ',')),
            vec!["a", "b,c", "d"]
        );
    }

    #[test]
    fn split_borrowed_when_no_strip() {
        // No escapes → segments should be Cow::Borrowed (no allocation).
        let segments = split_respecting_escape("a|b|c", '|');
        for seg in &segments {
            assert!(
                matches!(seg, Cow::Borrowed(_)),
                "expected Borrowed, got {seg:?}"
            );
        }
    }

    #[test]
    fn split_owned_when_strip_happens() {
        let segments = split_respecting_escape("a\\|b|c", '|');
        assert!(matches!(segments[0], Cow::Owned(_)));
        assert!(matches!(segments[1], Cow::Borrowed(_)));
    }

    #[test]
    fn split_unicode_content() {
        assert_eq!(
            collect(split_respecting_escape("α|β|γ", '|')),
            vec!["α", "β", "γ"]
        );
    }

    #[test]
    fn split_unicode_with_escape() {
        assert_eq!(
            collect(split_respecting_escape("α\\|β|γ", '|')),
            vec!["α|β", "γ"]
        );
    }

    #[test]
    fn split_non_ascii_separator() {
        assert_eq!(
            collect(split_respecting_escape("a→b→c", '→')),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn split_non_ascii_separator_with_escape() {
        assert_eq!(
            collect(split_respecting_escape("a\\→b→c", '→')),
            vec!["a→b", "c"]
        );
    }

    // --- split_respecting_escape_and_literals ---

    #[test]
    fn split_literal_region_protects_separator() {
        assert_eq!(
            collect(split_respecting_escape_and_literals("a|`b|c`|d", '|', '`')),
            vec!["a", "`b|c`", "d"]
        );
    }

    #[test]
    fn split_literal_region_multiple_pipes() {
        assert_eq!(
            collect(split_respecting_escape_and_literals(
                "a|`x|y|z`|b",
                '|',
                '`'
            )),
            vec!["a", "`x|y|z`", "b"]
        );
    }

    #[test]
    fn split_escape_outside_literal_still_works() {
        assert_eq!(
            collect(split_respecting_escape_and_literals(
                "a\\|b|`c|d`|e",
                '|',
                '`'
            )),
            vec!["a|b", "`c|d`", "e"]
        );
    }

    #[test]
    fn split_unbalanced_literal_delim() {
        // Only one backtick → rest of input is treated as inside literal region.
        assert_eq!(
            collect(split_respecting_escape_and_literals("a|`b|c", '|', '`')),
            vec!["a", "`b|c"]
        );
    }

    #[test]
    fn split_escaped_literal_delim_does_not_open_region() {
        // `\` before backtick means the backtick is not a structural literal delimiter.
        assert_eq!(
            collect(split_respecting_escape_and_literals("a|\\`b|c", '|', '`')),
            vec!["a", "\\`b", "c"]
        );
    }

    #[test]
    fn split_empty_cells_between_literal_regions() {
        assert_eq!(
            collect(split_respecting_escape_and_literals("`a`|`b`", '|', '`')),
            vec!["`a`", "`b`"]
        );
    }

    // --- find_respecting_escape ---

    #[test]
    fn find_first_unescaped() {
        assert_eq!(find_respecting_escape("a|b|c", '|'), Some(1));
    }

    #[test]
    fn find_skips_escaped() {
        assert_eq!(find_respecting_escape("a\\|b|c", '|'), Some(4));
    }

    #[test]
    fn find_none_when_only_escaped() {
        assert_eq!(find_respecting_escape("a\\|b\\|c", '|'), None);
    }

    #[test]
    fn find_respects_literal_region() {
        assert_eq!(
            find_respecting_escape_and_literals("`a|b`|c", '|', '`'),
            Some(5)
        );
    }

    #[test]
    fn find_empty() {
        assert_eq!(find_respecting_escape("", '|'), None);
    }

    // --- is_structural_at ---

    #[test]
    fn structural_at_unescaped() {
        assert!(is_structural_at(b"a|b", 1, None));
    }

    #[test]
    fn structural_at_escaped() {
        assert!(!is_structural_at(b"a\\|b", 2, None));
    }

    #[test]
    fn structural_at_double_escape() {
        // \\| at pos 2 means "|" at pos 2, preceded by two backslashes → structural
        assert!(is_structural_at(b"a\\\\|b", 3, None));
    }

    #[test]
    fn structural_at_inside_literal() {
        // backtick-a-pipe-b-backtick: pipe at pos 2 is inside literal region
        assert!(!is_structural_at(b"`a|b`", 2, Some(b'`')));
    }

    #[test]
    fn structural_at_outside_literal() {
        assert!(is_structural_at(b"`a`|b", 3, Some(b'`')));
    }

    #[test]
    fn structural_at_out_of_bounds() {
        assert!(!is_structural_at(b"abc", 3, None));
        assert!(!is_structural_at(b"", 0, None));
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
