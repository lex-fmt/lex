//! Escape-aware structural splitting and finding.
//!
//! Structural Scanner Rules (for split/find on structural delimiters like `|`, `,`, `;`):
//!   - `\<sep>` is treated as a literal character (not a split point);
//!     the escaping backslash is stripped in the returned segment text.
//!   - `\\<sep>` counts as an escaped backslash followed by a structural `<sep>`
//!     (even number of backslashes → `<sep>` is structural).
//!   - Optionally, content inside balanced `literal_delim` pairs (e.g. backticks)
//!     is passed through verbatim: no split, no backslash stripping.

use super::structural::trailing_backslashes_before;
use std::borrow::Cow;

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
    let mut prev_backslashes = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        let is_escaped = prev_backslashes % 2 == 1;
        if let Some(delim) = literal_delim {
            if ch == delim && !is_escaped {
                in_literal = !in_literal;
                out.push(ch);
                prev_backslashes = 0;
                i += 1;
                continue;
            }
        }
        if !in_literal && ch == '\\' && chars.get(i + 1).copied() == Some(sep) {
            out.push(sep);
            prev_backslashes = 0;
            i += 2;
            continue;
        }
        out.push(ch);
        if ch == '\\' {
            prev_backslashes += 1;
        } else {
            prev_backslashes = 0;
        }
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
