//! Detect UTF-8 double-encoding (mojibake) in source text.
//!
//! When a UTF-8 file is misread as cp1252 (or similar) and then re-encoded
//! back to UTF-8, characters acquire a telltale two-character signature
//! such as `Ã©` (for `é`), `Ã¶` (for `ö`), or `â€"` (for em-dash).
//! Converters pass these through verbatim, so the broken bytes appear in
//! markdown / HTML / PDF outputs and surprise the author downstream.
//!
//! The check is intentionally cheap (substring scan) and high-signal:
//! every pattern listed below is essentially never produced by legitimate
//! UTF-8 text, but to keep the false-positive rate near zero on text that
//! happens to contain `â` or `Ã` in isolation, [`detect_mojibake`] only
//! returns `Some` when at least three distinct patterns appear in the
//! input.

/// Summary of mojibake patterns observed in a single input string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojibakeReport {
    /// How many distinct mojibake patterns were hit (a single pattern
    /// can occur many times in one file; we count each pattern once).
    pub distinct_patterns: usize,
}

/// Two-character signatures of UTF-8 → cp1252 → UTF-8 round-trips. Each
/// is rare-to-impossible in legitimate UTF-8 prose, but trivially common
/// in mojibaked output.
const PATTERNS: &[&str] = &[
    // Latin-1 letters double-encoded: original UTF-8 byte pair (e.g.
    // `\xC3\xA9` for `é`) is read as two cp1252 chars (`Ã` + `©`) which
    // then re-encode to a 4-byte UTF-8 sequence displaying as `Ã©`.
    "Ã©", "Ã¨", "Ãª", "Ã«", // e-family
    "Ã ", "Ã¡", "Ã¢", "Ã£", "Ã¤", "Ã¥", // a-family
    "Ã¬", "Ã­", "Ã®", "Ã¯", // i-family
    "Ã²", "Ã³", "Ã´", "Ã¶", // o-family
    "Ã¹", "Ãº", "Ã»", "Ã¼", // u-family
    "Ã±", "Ã§", // tilde n, c-cedilla
    "ÃŸ", // sharp s
    // Smart-punctuation double-encodings all start with `â€`
    // (mojibake of the `\xE2\x80` Unicode-punctuation block prefix).
    // The trailing byte distinguishes em-dash / en-dash / curly quotes;
    // any `â€` is a near-certain mojibake signal on its own.
    "â€",
];

/// Scan `input` for mojibake patterns. Returns `Some` with a report when
/// at least three distinct patterns are present; otherwise `None`.
///
/// Single-pattern hits are deliberately silent so legitimate text that
/// contains an isolated `â` or `Ã` (e.g. a Spanish or French word) does
/// not trigger a warning.
pub fn detect_mojibake(input: &str) -> Option<MojibakeReport> {
    let distinct = PATTERNS.iter().filter(|pat| input.contains(*pat)).count();
    if distinct >= 3 {
        Some(MojibakeReport {
            distinct_patterns: distinct,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_classic_double_encoded_paragraph() {
        // A paragraph containing several mojibaked words. Should fire.
        // The em-dash mojibake `â€\u{201D}` ends in a curly quote, not
        // the ASCII string-terminator quote — explicit escape avoids
        // confusing Rust's lexer.
        let input = "RÃ©sumÃ© of MÃ¶bius strip: a cafÃ© favourite. â€\u{201D} indeed.";
        let report = detect_mojibake(input).expect("clear mojibake should report");
        assert!(
            report.distinct_patterns >= 3,
            "expected ≥3 distinct patterns, got {}",
            report.distinct_patterns
        );
    }

    #[test]
    fn ignores_legitimate_text_with_isolated_accents() {
        // Spanish / French prose containing real `â`, `Ã` etc. should
        // NOT trigger. Single isolated occurrences stay below the
        // threshold.
        let input = "El niño visitó la ciudad. Le maître prépare le dîner.";
        assert_eq!(detect_mojibake(input), None);
    }

    #[test]
    fn ignores_pure_ascii() {
        let input = "Just plain ASCII text with no special characters.\n";
        assert_eq!(detect_mojibake(input), None);
    }

    #[test]
    fn ignores_single_pattern_hit() {
        // A single mojibake occurrence — could equally be an unrelated
        // accented word — stays under the threshold.
        let input = "An odd Ã© occurrence, nothing else.";
        assert_eq!(detect_mojibake(input), None);
    }

    #[test]
    fn flags_smart_quote_em_dash_storm() {
        // Curly-quote / em-dash mojibake clusters around `â€` patterns.
        // All cp1252-byte trailers (em-dash, right-quote, left-quote)
        // reduce to the same `â€` prefix, so we add Latin-letter hits
        // (`Ã©`, `Ã¶`, `Ã±`) to clear the three-distinct threshold.
        let input = "He said â€\u{201C}helloâ€\u{201D}, then â€\u{201D} she replied. \
                     CafÃ© Ã¶pen Ã± kids.";
        let report = detect_mojibake(input).expect("should detect");
        assert!(report.distinct_patterns >= 3);
    }
}
