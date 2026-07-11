//! Structural marker detection and quoted-parameter-value escaping.
//!
//! Quoted Parameter Value Escaping Rules:
//!   - `\"` inside a quoted value: literal quote (backslash removed)
//!   - `\\` inside a quoted value: literal backslash
//!   - Only `"` and `\` can be escaped; other backslashes are literal
//!
//! Structural marker detection identifies `LexMarker` tokens that are *not*
//! inside a quoted context, and locates structural delimiters in raw byte
//! streams while respecting backslash escapes and balanced literal regions.

use super::inline::is_quote_escaped_by_prev_token;

// --- Structural marker detection ---

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
pub(super) fn trailing_backslashes_before(bytes: &[u8], pos: usize) -> usize {
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
            if bytes[i] == delim && trailing_backslashes_before(bytes, i).is_multiple_of(2) {
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
