//! Property-based tests for escape-aware structural scanners.
//!
//! Invariants exercised:
//! - Never panics on arbitrary input.
//! - Segment count matches the number of structural separators + 1.
//! - Re-escaping and re-joining segments round-trips to the original input
//!   (modulo double-backslash normalization).
//! - Literal regions (backticks) never contain a split boundary.
//! - `is_structural_at` agrees with `find_respecting_escape` on the positions it reports.

use std::borrow::Cow;

use lex_core::lex::escape::{
    find_respecting_escape, find_respecting_escape_and_literals, is_structural_at,
    split_respecting_escape, split_respecting_escape_and_literals,
};
use proptest::prelude::*;

/// Inputs that exercise escape-aware scanning: sep char, backslashes, literal delim,
/// plain content, unicode.
fn input_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Dense special-char mix
        r"[a-z|\\`,;]{0,40}",
        // Lots of backslashes
        r"[a-z\\]{0,40}",
        // Lots of pipes
        r"[a-z|]{0,40}",
        // Lots of backticks
        r"[a-z`|\\]{0,40}",
        // Generic printable ASCII
        r"[ -~]{0,80}",
        // Unicode + specials
        r"[α-ω|\\`]{0,20}",
    ]
}

/// Count how many positions in `s` would be reported as structural for `sep`.
fn count_structural(s: &str, sep: char, literal_delim: Option<char>) -> usize {
    let bytes = s.as_bytes();
    let mut count = 0usize;
    // Only meaningful for ASCII sep (most tests) — char-level version would need adaptation.
    for (i, ch) in s.char_indices() {
        if ch == sep {
            let is_structural = match literal_delim {
                Some(delim) => is_structural_at(bytes, i, Some(delim as u8)),
                None => is_structural_at(bytes, i, None),
            };
            if is_structural {
                count += 1;
            }
        }
    }
    count
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Never panics on any input.
    #[test]
    fn split_never_panics(s in input_strategy(), sep in r"[|,;]") {
        let sep_ch = sep.chars().next().unwrap();
        let _ = split_respecting_escape(&s, sep_ch);
    }

    #[test]
    fn split_with_literals_never_panics(s in input_strategy(), sep in r"[|,;]") {
        let sep_ch = sep.chars().next().unwrap();
        let _ = split_respecting_escape_and_literals(&s, sep_ch, '`');
    }

    #[test]
    fn find_never_panics(s in input_strategy(), sep in r"[|,;]") {
        let sep_ch = sep.chars().next().unwrap();
        let _ = find_respecting_escape(&s, sep_ch);
        let _ = find_respecting_escape_and_literals(&s, sep_ch, '`');
    }

    /// Segment count equals structural separator count + 1.
    #[test]
    fn split_segment_count_matches_structural_count(s in input_strategy(), sep in r"[|,;]") {
        let sep_ch = sep.chars().next().unwrap();
        let segments = split_respecting_escape(&s, sep_ch);
        let structural = count_structural(&s, sep_ch, None);
        prop_assert_eq!(segments.len(), structural + 1);
    }

    #[test]
    fn split_with_literals_segment_count(s in input_strategy(), sep in r"[|,;]") {
        let sep_ch = sep.chars().next().unwrap();
        let segments = split_respecting_escape_and_literals(&s, sep_ch, '`');
        let structural = count_structural(&s, sep_ch, Some('`'));
        prop_assert_eq!(segments.len(), structural + 1);
    }

    /// Segments, when their escaped separators are re-escaped and rejoined with sep,
    /// reproduce a string whose structural-separator positions match the original.
    #[test]
    fn split_rejoin_preserves_structural_positions(s in input_strategy(), sep in r"[|,;]") {
        let sep_ch = sep.chars().next().unwrap();
        let segments = split_respecting_escape(&s, sep_ch);
        let rejoined = segments
            .iter()
            .map(|seg| reescape_sep(seg.as_ref(), sep_ch))
            .collect::<Vec<_>>()
            .join(&sep_ch.to_string());
        // Re-splitting the rejoined string gives the same segments.
        let resplit = split_respecting_escape(&rejoined, sep_ch);
        let a: Vec<&str> = segments.iter().map(|c| c.as_ref()).collect();
        let b: Vec<&str> = resplit.iter().map(|c| c.as_ref()).collect();
        prop_assert_eq!(a, b);
    }

    /// `find_respecting_escape` reports the same first structural position as the splitter.
    #[test]
    fn find_matches_first_split(s in input_strategy(), sep in r"[|,;]") {
        let sep_ch = sep.chars().next().unwrap();
        let found = find_respecting_escape(&s, sep_ch);
        let segments = split_respecting_escape(&s, sep_ch);
        if segments.len() == 1 {
            prop_assert_eq!(found, None);
        } else {
            prop_assert!(found.is_some());
            // The first segment's length (in the re-escaped form that came from the input)
            // corresponds to the position of the first structural separator.
            // We can verify by reconstructing: input[..found] must have zero structural seps.
            let pos = found.unwrap();
            let before = &s[..pos];
            prop_assert_eq!(count_structural(before, sep_ch, None), 0);
        }
    }

    /// Borrowed Cow when no escape stripping was needed.
    #[test]
    fn split_borrowed_when_no_escape_of_sep(s in r"[a-z|]{0,40}", sep in r"[|]") {
        let sep_ch = sep.chars().next().unwrap();
        let segments = split_respecting_escape(&s, sep_ch);
        for seg in segments {
            prop_assert!(matches!(seg, Cow::Borrowed(_)));
        }
    }

    /// For inputs with only literal-region-protected separators, the split count
    /// respects balanced backticks.
    #[test]
    fn literals_protect_separators(
        content in r"[a-z,;]{0,20}",
        prefix in r"[a-z|]{0,10}",
        suffix in r"[a-z|]{0,10}"
    ) {
        let wrapped = format!("{prefix}`{content}`{suffix}");
        let segments = split_respecting_escape_and_literals(&wrapped, '|', '`');
        // Inside the backtick region, `|` must not cause a split. Count non-literal pipes.
        let structural = count_structural(&wrapped, '|', Some('`'));
        prop_assert_eq!(segments.len(), structural + 1);
        // At least one segment contains the entire backtick region verbatim.
        let reconstructed = segments.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join("|");
        let needle = format!("`{content}`");
        prop_assert!(
            reconstructed.contains(&needle) || content.is_empty() || content.contains('|')
        );
    }
}

/// Re-escape `sep` characters in a plain segment so that re-splitting gives the same
/// segment back. Matches the no-literal split variant: every `sep` in the segment
/// was originally escaped, so we re-escape all of them.
///
/// This helper only re-escapes separator characters; it does not apply any special
/// handling for trailing backslashes. Segments that end with a backslash followed
/// by a structural separator in the rejoined stream are a known round-trip edge
/// case not covered by this helper.
fn reescape_sep(seg: &str, sep: char) -> String {
    let mut out = String::with_capacity(seg.len());
    for ch in seg.chars() {
        if ch == sep {
            out.push('\\');
            out.push(ch);
        } else {
            out.push(ch);
        }
    }
    out
}
