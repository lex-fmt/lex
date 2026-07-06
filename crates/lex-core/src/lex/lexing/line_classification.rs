//! Line Classification
//!
//!     Core classification logic for determining line types based on token patterns. This module
//!     contains the classifiers used by the lexer to categorize lines.
//!
//!     Since the grammar operates mostly over lines, and each line must be tokenized into one
//!     category during the lexing stage, classification is crucial. In the real world, a line might
//!     be more than one possible category. For example a line might have a sequence marker and a
//!     subject marker (for example "1. Recap:").
//!
//!     For this reason, line tokens can be OR tokens at times (like SubjectOrListItemLine), and at
//!     other times the order of line categorization is crucial to getting the right result. While
//!     there are only a few consequential marks in lines (blank, data, subject, list) having them
//!     denormalized is required to have parsing simpler.
//!
//!     The definitive set is the LineType enum. See the [line](crate::lex::token::line) module for
//!     the complete list of line types.
//!
//! Classification Order
//!
//!     Classification follows this specific order (important for correctness):
//!         1. Blank lines
//!         2. Data marker lines (:: label params? ::, closed form)
//!         3. Data lines (:: label params? without closing ::)
//!         4. List lines starting with list marker AND ending with colon -> SubjectOrListItemLine
//!         5. List lines (starting with list marker)
//!         6. Subject lines (ending with colon)
//!         7. Default to paragraph
//!
//!     This ordering ensures that more specific patterns (like data marker lines) are matched before
//!     more general ones (like subject lines).

use crate::lex::annotation::analyze_annotation_header_tokens;
use crate::lex::ast::elements::sequence_marker::{DecorationStyle, Form, Separator};
use crate::lex::escape::find_structural_lex_markers;
use crate::lex::lexing::base_tokenization::tokenize;
use crate::lex::token::{LineType, Token};
use std::borrow::Cow;

/// Parsed details about a list marker at the start of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedListMarker {
    /// Index of the first marker token (after any indentation/leading whitespace)
    pub marker_start: usize,
    /// Index one past the last marker token (excludes the required separating whitespace)
    pub marker_end: usize,
    /// Index of the first body token after the marker's separating whitespace
    pub body_start: usize,
    /// The decoration style of the marker (plain, numerical, alphabetical, roman)
    pub style: DecorationStyle,
    /// The separator style (period, parenthesis, double parenthesis)
    pub separator: Separator,
    /// The form (short or extended)
    pub form: Form,
}

/// Determine the type of a line based on its tokens.
///
/// Classification follows this specific order (important for correctness):
/// 1. Blank lines
/// 2. Data marker lines (:: label params? ::, closed form)
/// 3. List lines starting with list marker AND ending with colon -> SubjectOrListItemLine
/// 4. List lines (starting with list marker)
/// 5. Subject lines (ending with colon)
/// 6. Default to paragraph
///
/// Note: there is no "open form" data line. A `:: label` with no closing `::` is
/// not a recognized element, so it falls through to a paragraph like any other
/// unrecognized text — Lex keeps content rather than dropping it.
pub fn classify_line_tokens(tokens: &[Token]) -> LineType {
    if tokens.is_empty() {
        return LineType::ParagraphLine;
    }

    // BLANK_LINE: Only whitespace and newline tokens
    if is_blank_line(tokens) {
        return LineType::BlankLine;
    }

    // DATA_MARKER_LINE: Data marker in closed form (:: label params? ::)
    if is_data_marker_line(tokens) {
        return LineType::DataMarkerLine;
    }

    // Check if line both starts with list marker AND ends with colon
    let has_seq_marker = parse_seq_marker(tokens).is_some();
    let has_colon = ends_with_colon(tokens);

    if has_seq_marker && has_colon {
        return LineType::SubjectOrListItemLine;
    }

    // LIST_LINE: Starts with list marker
    if has_seq_marker {
        return LineType::ListLine;
    }

    // SUBJECT_LINE: Ends with colon
    if has_colon {
        return LineType::SubjectLine;
    }

    // Default: PARAGRAPH_LINE
    LineType::ParagraphLine
}

/// Check if line is blank (only whitespace and newline)
///
/// Blank lines are semantically significant in Lex (they separate paragraphs and are required
/// before/after session titles), but only their existence matters, not the exact whitespace content.
fn is_blank_line(tokens: &[Token]) -> bool {
    tokens.iter().all(|t| {
        matches!(
            t,
            Token::Whitespace(_) | Token::Indentation | Token::BlankLine(_)
        )
    })
}

/// Check if line is a data marker line in closed form: :: label params? ::
/// Grammar: <lex-marker><space><label>(<space><parameters>)? <lex-marker> <content>?
///
/// Uses quote-aware marker detection so that `::` inside quoted parameter
/// values (e.g., `:: test.note msg=":: value" ::`) is not misidentified as a
/// structural delimiter.
fn is_data_marker_line(tokens: &[Token]) -> bool {
    if tokens.is_empty() {
        return false;
    }

    // Find structural markers (outside quoted regions)
    let structural = find_structural_lex_markers(tokens);
    if structural.len() < 2 {
        return false;
    }

    let first_marker_idx = structural[0];

    // First marker must be at the start (after optional whitespace/indentation)
    for token in &tokens[..first_marker_idx] {
        if !matches!(token, Token::Indentation | Token::Whitespace(_)) {
            return false;
        }
    }

    // After first marker, must have whitespace (or be end of line)
    if first_marker_idx + 1 < tokens.len()
        && !matches!(tokens[first_marker_idx + 1], Token::Whitespace(_))
    {
        return false;
    }

    let second_marker_idx = structural[1];

    // Require a label between the markers
    let header_tokens = &tokens[first_marker_idx + 1..second_marker_idx];
    analyze_annotation_header_tokens(header_tokens).has_label
}

/// Parse a list marker at the start of a line (after optional indentation).
///
/// Supported marker forms:
/// - Plain dash: "- "
/// - Ordered single-part: "1.", "1)", "a.", "I." (with trailing space)
/// - Ordered extended: multi-part sequences like "4.3.2" or "IV.2.1)" (with trailing space)
/// - Parenthetical: "(1)", "(a)", "(I)" (with trailing space)
pub fn parse_seq_marker(tokens: &[Token]) -> Option<ParsedListMarker> {
    let mut i = 0;

    // Skip leading indentation and whitespace
    while i < tokens.len() && matches!(tokens[i], Token::Indentation | Token::Whitespace(_)) {
        i += 1;
    }

    if i >= tokens.len() {
        return None;
    }

    // Helper: ensure at least one whitespace after the marker and return ParsedListMarker
    let finish_with_whitespace = |marker_end: usize,
                                  style: DecorationStyle,
                                  separator: Separator,
                                  form: Form|
     -> Option<ParsedListMarker> {
        let mut body_start = marker_end;
        let mut saw_ws = false;
        while body_start < tokens.len() {
            if matches!(tokens[body_start], Token::Whitespace(_)) {
                saw_ws = true;
                body_start += 1;
                continue;
            }
            break;
        }

        if !saw_ws {
            return None;
        }

        Some(ParsedListMarker {
            marker_start: i,
            marker_end,
            body_start,
            style,
            separator,
            form,
        })
    };

    // Check for plain list marker: Dash Whitespace
    if matches!(tokens[i], Token::Dash) {
        return finish_with_whitespace(
            i + 1,
            DecorationStyle::Plain,
            Separator::Period,
            Form::Short,
        );
    }

    // Check for parenthetical list marker: (Number | Letter | RomanNumeral)
    if i + 2 < tokens.len()
        && matches!(tokens[i], Token::OpenParen)
        && matches!(tokens[i + 2], Token::CloseParen)
        && is_segment(&tokens[i + 1])
    {
        let style = detect_segment_style(&tokens[i + 1]);
        return finish_with_whitespace(i + 3, style, Separator::DoubleParens, Form::Short);
    }

    // Extended numeric/alpha sequence: e.g. 4.3.2 or IV.2.1)
    // Check this BEFORE single-part to avoid early matching
    if is_segment(&tokens[i]) {
        let mut idx = i + 1;
        let mut segments = 1;

        while idx + 1 < tokens.len()
            && matches!(tokens[idx], Token::Period)
            && is_segment(&tokens[idx + 1])
        {
            segments += 1;
            idx += 2;
        }

        if segments >= 2 {
            // Determine separator: check if there's a closing paren or period at the end
            let separator = if idx < tokens.len() && matches!(tokens[idx], Token::CloseParen) {
                idx += 1;
                Separator::Parenthesis
            } else if idx < tokens.len() && matches!(tokens[idx], Token::Period) {
                idx += 1;
                Separator::Period
            } else {
                // No explicit final separator, default to Period for extended forms
                Separator::Period
            };

            let style = detect_segment_style(&tokens[i]);
            return finish_with_whitespace(idx, style, separator, Form::Extended);
        }
    }

    // Ordered single-part: (Number | Letter | RomanNumeral) (Period | CloseParen)
    // This must come AFTER extended form check
    if i + 1 < tokens.len()
        && is_segment(&tokens[i])
        && matches!(tokens[i + 1], Token::Period | Token::CloseParen)
    {
        let style = detect_segment_style(&tokens[i]);
        let separator = if matches!(tokens[i + 1], Token::Period) {
            Separator::Period
        } else {
            Separator::Parenthesis
        };
        return finish_with_whitespace(i + 2, style, separator, Form::Short);
    }

    None
}

/// Check if line starts with a list marker (after optional indentation)
pub fn has_seq_marker(tokens: &[Token]) -> bool {
    parse_seq_marker(tokens).is_some()
}

/// Whether `title`, read as a would-be session-title line, leads with a token
/// sequence that [`parse_seq_marker`] classifies as a *non-Plain* session
/// sequence marker (Numerical / Alphabetical / Roman, in any separator or form).
/// Sessions reject Plain (dash) markers, so a leading dash is never a session
/// marker and reports `false`.
fn leads_with_session_marker(title: &str) -> bool {
    let tokens: Vec<Token> = tokenize(title).into_iter().map(|(t, _)| t).collect();
    matches!(parse_seq_marker(&tokens), Some(pm) if pm.style != DecorationStyle::Plain)
}

/// Serializer guard (escaping.lex §3.4, lex#795): a style-less session whose
/// title text begins with a marker-like token (`1.`, `a)`, `IV.`, `(1)`,
/// `1.2.3`, …) would, if serialized verbatim, re-parse as a session *with* that
/// sequence-marker style — a Faithfulness violation, since the source AST had no
/// marker. Insert a single escaping backslash before the marker's first
/// structural (non-alphanumeric) character so the leading token no longer reads
/// as a marker. The backslash is stripped again on re-parse
/// ([`unescape_session_title_marker_guard`]) and at render time (inline
/// escaping, §1), so the title *text* is unchanged. Titles that do not lead with
/// a session marker are returned untouched.
///
/// Callers must apply this only when the session carries no explicit marker: a
/// genuinely numbered session keeps its real marker and must never be escaped.
pub fn escape_session_title_marker_guard(title: &str) -> Cow<'_, str> {
    if !leads_with_session_marker(title) {
        return Cow::Borrowed(title);
    }
    // The marker's first structural character — the byte the guard backslash
    // must precede — is the first non-alphanumeric byte of the title. For `1.`,
    // `IV.`, or `a)` that byte follows an ASCII-alphanumeric run (digits, a
    // single letter, or Roman numerals); for a parenthetical marker like `(1)`
    // it is the leading `(` at position 0.
    match title.bytes().position(|b| !b.is_ascii_alphanumeric()) {
        Some(sep) => {
            let mut escaped = String::with_capacity(title.len() + 1);
            escaped.push_str(&title[..sep]);
            escaped.push('\\');
            escaped.push_str(&title[sep..]);
            Cow::Owned(escaped)
        }
        // Unreachable in practice: a matched marker always has a separator, so a
        // non-alphanumeric byte exists. Fall back to the unescaped title.
        None => Cow::Borrowed(title),
    }
}

/// Parser counterpart to [`escape_session_title_marker_guard`] (escaping.lex
/// §3.4: the structural layer that consumes a delimiter must strip its own
/// escaping backslash). If `title` is an *escaped* session marker — a single
/// backslash guarding the marker's first structural character whose removal
/// yields a non-Plain session marker — return the title with that backslash
/// removed (the marker stays suppressed, the text is restored). Returns `None`
/// when `title` is not an escaped marker, so genuine content is left untouched,
/// including `\<alnum>` sequences like `C:\Users` that §1 keeps literal.
pub fn unescape_session_title_marker_guard(title: &str) -> Option<String> {
    let bytes = title.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b' ' {
            // The marker region ends at the first space; a guard backslash is
            // always inside the marker, so stop looking.
            break;
        }
        if b != b'\\' {
            continue;
        }
        // §1: a backslash before an alphanumeric character is literal text, never
        // a marker guard (the serializer only ever escapes the marker's
        // non-alphanumeric separator). Anything else is not our escape.
        match bytes.get(i + 1) {
            Some(next) if !next.is_ascii_alphanumeric() => {}
            _ => return None,
        }
        let mut candidate = String::with_capacity(title.len() - 1);
        candidate.push_str(&title[..i]);
        candidate.push_str(&title[i + 1..]);
        return leads_with_session_marker(&candidate).then_some(candidate);
    }
    None
}

/// Check if a string is a single letter (a-z, A-Z)
fn is_single_letter(s: &str) -> bool {
    s.len() == 1 && s.chars().next().is_some_and(|c| c.is_alphabetic())
}

fn is_segment(token: &Token) -> bool {
    matches!(token, Token::Number(_))
        || matches!(token, Token::Text(ref s) if is_single_letter(s) || is_roman_numeral(s))
}

/// Check if a string is a Roman numeral (I, II, III, IV, V, etc.)
/// Supports both uppercase (I, V, X, L, C, D, M) and lowercase (i, v, x, l, c, d, m).
fn is_roman_numeral(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // All characters must be valid Roman numeral characters, in a single case
    let is_upper = s
        .chars()
        .all(|c| matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'));
    let is_lower = s
        .chars()
        .all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'));
    is_upper || is_lower
}

/// Detect the decoration style from a marker segment token
fn detect_segment_style(token: &Token) -> DecorationStyle {
    match token {
        Token::Number(_) => DecorationStyle::Numerical,
        Token::Text(s) if is_roman_numeral(s) => DecorationStyle::Roman,
        Token::Text(s) if is_single_letter(s) => DecorationStyle::Alphabetical,
        _ => DecorationStyle::Numerical, // Default fallback
    }
}

/// Check if line ends with colon (ignoring trailing whitespace and newline)
///
/// Subject lines (for definitions, verbatim blocks, and sessions) end with a colon.
/// Trailing whitespace and newlines are ignored when checking for the colon.
pub fn ends_with_colon(tokens: &[Token]) -> bool {
    // Find last non-whitespace token before newline
    let mut i = tokens.len() as i32 - 1;

    while i >= 0 {
        let token = &tokens[i as usize];
        match token {
            Token::BlankLine(_) | Token::Whitespace(_) => {
                i -= 1;
            }
            Token::Colon => return true,
            _ => return false,
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_paragraph_line() {
        let tokens = vec![
            Token::Text("Hello".to_string()),
            Token::Whitespace(1),
            Token::Text("world".to_string()),
            Token::BlankLine(Some("\n".to_string())),
        ];
        assert_eq!(classify_line_tokens(&tokens), LineType::ParagraphLine);
    }

    #[test]
    fn test_classify_subject_line() {
        let tokens = vec![
            Token::Text("Title".to_string()),
            Token::Colon,
            Token::BlankLine(Some("\n".to_string())),
        ];
        assert_eq!(classify_line_tokens(&tokens), LineType::SubjectLine);
    }

    #[test]
    fn test_classify_list_line() {
        let tokens = vec![
            Token::Dash,
            Token::Whitespace(1),
            Token::Text("Item".to_string()),
            Token::BlankLine(Some("\n".to_string())),
        ];
        assert_eq!(classify_line_tokens(&tokens), LineType::ListLine);
    }

    #[test]
    fn test_classify_blank_line() {
        let tokens = vec![
            Token::Whitespace(1),
            Token::BlankLine(Some("\n".to_string())),
        ];
        assert_eq!(classify_line_tokens(&tokens), LineType::BlankLine);
    }

    #[test]
    fn test_classify_annotation_start_line() {
        let tokens = vec![
            Token::LexMarker,
            Token::Whitespace(1),
            Token::Text("label".to_string()),
            Token::Whitespace(1),
            Token::LexMarker,
            Token::BlankLine(Some("\n".to_string())),
        ];
        assert_eq!(classify_line_tokens(&tokens), LineType::DataMarkerLine);
    }

    #[test]
    fn test_open_form_data_marker_is_paragraph() {
        // `:: label` with no closing `::` is not a recognized element (there is
        // no "open form"). It falls through to a paragraph so its content is
        // preserved rather than dropped (lex#700).
        let tokens = vec![
            Token::LexMarker,
            Token::Whitespace(1),
            Token::Text("label".to_string()),
            Token::BlankLine(Some("\n".to_string())),
        ];
        assert_eq!(classify_line_tokens(&tokens), LineType::ParagraphLine);
    }

    #[test]
    fn test_annotation_line_without_label_falls_back_to_paragraph() {
        let tokens = vec![
            Token::LexMarker,
            Token::Whitespace(1),
            Token::Text("version".to_string()),
            Token::Equals,
            Token::Number("3.11".to_string()),
            Token::Whitespace(1),
            Token::LexMarker,
            Token::BlankLine(Some("\n".to_string())),
        ];

        assert_eq!(classify_line_tokens(&tokens), LineType::ParagraphLine);
    }

    #[test]
    fn test_classify_subject_or_list_item_line() {
        let tokens = vec![
            Token::Dash,
            Token::Whitespace(1),
            Token::Text("Item".to_string()),
            Token::Colon,
            Token::BlankLine(Some("\n".to_string())),
        ];
        assert_eq!(
            classify_line_tokens(&tokens),
            LineType::SubjectOrListItemLine
        );
    }

    #[test]
    fn test_ordered_seq_markers() {
        // Number-based
        let tokens = vec![
            Token::Number("1".to_string()),
            Token::Period,
            Token::Whitespace(1),
            Token::Text("Item".to_string()),
        ];
        assert!(has_seq_marker(&tokens));

        // Letter-based
        let tokens = vec![
            Token::Text("a".to_string()),
            Token::Period,
            Token::Whitespace(1),
            Token::Text("Item".to_string()),
        ];
        assert!(has_seq_marker(&tokens));

        // Roman numeral
        let tokens = vec![
            Token::Text("I".to_string()),
            Token::Period,
            Token::Whitespace(1),
            Token::Text("Item".to_string()),
        ];
        assert!(has_seq_marker(&tokens));

        // With close paren
        let tokens = vec![
            Token::Number("1".to_string()),
            Token::CloseParen,
            Token::Whitespace(1),
            Token::Text("Item".to_string()),
        ];
        assert!(has_seq_marker(&tokens));
    }

    #[test]
    fn test_extended_ordered_seq_marker() {
        let tokens = vec![
            Token::Number("4".to_string()),
            Token::Period,
            Token::Number("3".to_string()),
            Token::Period,
            Token::Number("2".to_string()),
            Token::Whitespace(1),
            Token::Text("Item".to_string()),
        ];

        let parsed = parse_seq_marker(&tokens).expect("expected list marker");
        assert_eq!(parsed.marker_start, 0);
        assert_eq!(parsed.marker_end, 5);
        assert_eq!(parsed.body_start, 6);

        assert_eq!(classify_line_tokens(&tokens), LineType::ListLine);
    }

    #[test]
    fn test_extended_marker_with_lowercase_roman() {
        // 1.a.ii. Item  — extended form with mixed styles including lowercase roman
        let tokens = vec![
            Token::Number("1".to_string()),
            Token::Period,
            Token::Text("a".to_string()),
            Token::Period,
            Token::Text("ii".to_string()),
            Token::Period,
            Token::Whitespace(1),
            Token::Text("Item".to_string()),
        ];

        let parsed = parse_seq_marker(&tokens).expect("expected extended marker");
        assert_eq!(parsed.form, Form::Extended);
        assert_eq!(parsed.style, DecorationStyle::Numerical);
        assert_eq!(parsed.separator, Separator::Period);
        assert_eq!(classify_line_tokens(&tokens), LineType::ListLine);
    }

    #[test]
    fn test_lowercase_roman_short_marker() {
        // ii. Item
        let tokens = vec![
            Token::Text("ii".to_string()),
            Token::Period,
            Token::Whitespace(1),
            Token::Text("Item".to_string()),
        ];

        let parsed = parse_seq_marker(&tokens).expect("expected short roman marker");
        assert_eq!(parsed.form, Form::Short);
        assert_eq!(parsed.style, DecorationStyle::Roman);
        assert_eq!(classify_line_tokens(&tokens), LineType::ListLine);
    }

    #[test]
    fn test_lex_marker_inside_quoted_value_is_annotation_start() {
        // :: test.note foo=":: jane" ::
        let tokens = vec![
            Token::LexMarker,
            Token::Whitespace(1),
            Token::Text("note".to_string()),
            Token::Whitespace(1),
            Token::Text("foo".to_string()),
            Token::Equals,
            Token::Quote,
            Token::LexMarker, // :: inside quotes
            Token::Whitespace(1),
            Token::Text("jane".to_string()),
            Token::Quote,
            Token::Whitespace(1),
            Token::LexMarker, // closing ::
            Token::BlankLine(Some("\n".to_string())),
        ];
        assert_eq!(classify_line_tokens(&tokens), LineType::DataMarkerLine);
    }

    #[test]
    fn test_lex_marker_inside_quoted_value_not_a_closing_marker() {
        // :: test.note foo=":: value"  (no real closing :: — the inner one is
        // inside a quoted value). It is not a closed-form data marker, and with
        // the open form gone it falls through to a paragraph (lex#700). The
        // point of this regression is that the quoted `::` is NOT mistaken for a
        // structural closing marker (which would make it a DataMarkerLine).
        let tokens = vec![
            Token::LexMarker,
            Token::Whitespace(1),
            Token::Text("note".to_string()),
            Token::Whitespace(1),
            Token::Text("foo".to_string()),
            Token::Equals,
            Token::Quote,
            Token::LexMarker, // :: inside quotes
            Token::Whitespace(1),
            Token::Text("value".to_string()),
            Token::Quote,
            Token::BlankLine(Some("\n".to_string())),
        ];
        assert_eq!(classify_line_tokens(&tokens), LineType::ParagraphLine);
    }

    // ── lex#795: session-title marker-guard escaping (escaping.lex §3.4) ──────

    #[test]
    fn escape_guards_every_session_marker_form() {
        // A style-less title whose text begins with a marker-like token gets a
        // backslash before the marker's first structural character.
        assert_eq!(
            escape_session_title_marker_guard("1. Primary"),
            "1\\. Primary"
        );
        assert_eq!(escape_session_title_marker_guard("a. Alpha"), "a\\. Alpha");
        assert_eq!(
            escape_session_title_marker_guard("IV. Roman"),
            "IV\\. Roman"
        );
        assert_eq!(escape_session_title_marker_guard("1) Paren"), "1\\) Paren");
        assert_eq!(
            escape_session_title_marker_guard("(a) Double"),
            "\\(a) Double"
        );
        assert_eq!(
            escape_session_title_marker_guard("1.2.3 Extended"),
            "1\\.2.3 Extended"
        );
    }

    #[test]
    fn escape_leaves_non_marker_titles_untouched() {
        // No leading marker token → no guard. Plain (dash) markers are not valid
        // session markers, so a leading dash is left alone too.
        assert_eq!(
            escape_session_title_marker_guard("Introduction"),
            "Introduction"
        );
        assert_eq!(
            escape_session_title_marker_guard("Version 2.0 notes"),
            "Version 2.0 notes"
        );
        assert_eq!(
            escape_session_title_marker_guard("- Not a session marker"),
            "- Not a session marker"
        );
        // `1.Primary` (no space after the separator) is not a marker.
        assert_eq!(escape_session_title_marker_guard("1.Primary"), "1.Primary");
    }

    #[test]
    fn unescape_reverses_the_guard_for_every_form() {
        assert_eq!(
            unescape_session_title_marker_guard("1\\. Primary").as_deref(),
            Some("1. Primary")
        );
        assert_eq!(
            unescape_session_title_marker_guard("a\\. Alpha").as_deref(),
            Some("a. Alpha")
        );
        assert_eq!(
            unescape_session_title_marker_guard("IV\\. Roman").as_deref(),
            Some("IV. Roman")
        );
        assert_eq!(
            unescape_session_title_marker_guard("1\\) Paren").as_deref(),
            Some("1) Paren")
        );
        assert_eq!(
            unescape_session_title_marker_guard("\\(a) Double").as_deref(),
            Some("(a) Double")
        );
        assert_eq!(
            unescape_session_title_marker_guard("1\\.2.3 Extended").as_deref(),
            Some("1.2.3 Extended")
        );
    }

    #[test]
    fn unescape_ignores_backslashes_that_are_not_marker_guards() {
        // `\` before an alphanumeric is literal (escaping.lex §1, e.g. a path);
        // removing it would not create a marker anyway.
        assert_eq!(unescape_session_title_marker_guard("C:\\Users"), None);
        assert_eq!(
            unescape_session_title_marker_guard("\\1. literal-backslash-title"),
            None
        );
        // A backslash before a non-marker separator is left alone.
        assert_eq!(
            unescape_session_title_marker_guard("Version 2\\.0 notes"),
            None
        );
        // No backslash at all.
        assert_eq!(unescape_session_title_marker_guard("1. Primary"), None);
        assert_eq!(unescape_session_title_marker_guard("Introduction"), None);
    }

    #[test]
    fn escape_then_unescape_is_identity() {
        for title in [
            "1. Primary",
            "a) Second",
            "IV. Historical",
            "(1) Appendix",
            "1.2.3 Deep",
        ] {
            let escaped = escape_session_title_marker_guard(title);
            assert_ne!(
                escaped.as_ref(),
                title,
                "guard must change a marker-like title"
            );
            assert_eq!(
                unescape_session_title_marker_guard(&escaped).as_deref(),
                Some(title),
                "unescape must restore the original title text for {title:?}",
            );
        }
    }
}
