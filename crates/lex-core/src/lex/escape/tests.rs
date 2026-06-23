use super::inline::is_quote_escaped_by_prev_token;
use super::*;
use std::borrow::Cow;

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
    // :: test.note foo=":: value" ::
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
fn structural_markers_single_opening_with_quoted_marker() {
    use crate::lex::token::Token;
    // :: test.note foo=":: value"  (no closing ::)
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
    // :: test.note foo="value with \" inside" ::
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
    // :: test.note foo="val\\" ::
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
fn split_escaped_literal_delim_before_escaped_sep_non_ascii() {
    // Regression: char-path `strip_escapes_char` used to toggle `in_literal`
    // on every literal_delim occurrence, including escaped ones. With a
    // non-ASCII literal_delim that forces the char path, an escaped `\α`
    // inside a segment falsely "opened" a literal region, which then
    // swallowed the following `\|` and blocked escape stripping.
    let segments = split_respecting_escape_and_literals("a\\α\\|b", '|', 'α');
    assert_eq!(
        segments.len(),
        1,
        "escaped pipe must not split; got segments={segments:?}"
    );
    assert_eq!(
        segments[0].as_ref(),
        "a\\α|b",
        "escaped pipe must be stripped; escaped alpha must not open a literal region"
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
