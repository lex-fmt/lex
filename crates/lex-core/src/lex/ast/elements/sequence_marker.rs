//! Sequence Marker Element
//!
//! Represents the numbering/decoration style for ordered sequences (Lists and Sessions).
//!
//! # Overview
//!
//! Sequence markers formalize how lists and sessions are numbered or decorated in the source text.
//! This centralizes marker representation that was previously scattered across the codebase.
//!
//! # Design
//!
//! A sequence marker has three independent dimensions:
//!
//! 1. **Decoration Style**: What kind of marker (number, letter, roman numeral, or plain dash)
//! 2. **Separator**: How the marker is terminated (period, parenthesis, or double parenthesis)
//! 3. **Form**: Whether it's a simple marker or an extended nested index
//!
//! # Examples
//!
//! ```text
//! 1.          → Numerical, Period, Short
//! a)          → Alphabetical, Parenthesis, Short
//! (IV)        → Roman, DoubleParens, Short
//! 1.2.3       → Numerical, Period, Extended
//! -           → Plain, Period, Short (lists only)
//! ```
//!
//! # Usage
//!
//! Sequence markers are attached to `List` and `Session` nodes:
//!
//! ```rust,ignore
//! let list = parse_list("1. First\n2. Second\n");
//! assert_eq!(list.marker.style, DecorationStyle::Numerical);
//! assert_eq!(list.marker.separator, Separator::Period);
//! ```
//!
//! # Parsing Rules
//!
//! The marker style, separator, and form are determined by the **first item** in a list or
//! the session title. Subsequent items may use different markers in the source (the parser
//! allows this for flexibility during editing), but formatters will normalize to the first
//! item's style.
//!
//! # Differences: Lists vs Sessions
//!
//! - **Lists**: Support all decoration styles including `Plain` (dash)
//! - **Sessions**: Support all styles **except** `Plain`
//!
//! # Round-Trip Fidelity
//!
//! The `raw_text` field preserves the exact source marker for perfect round-trip conversion.
//! This allows formatters to reconstruct the original text even if the parsed representation
//! is normalized.

use super::super::range::{Position, Range};
use super::super::text_content::TextContent;
use super::super::traits::AstNode;
use std::fmt;

/// Decoration style for sequence markers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationStyle {
    /// Plain dash marker: `-` (lists only, not sessions)
    Plain,
    /// Numerical marker: `1`, `2`, `3`, etc.
    Numerical,
    /// Alphabetical marker: `a`, `b`, `c`, etc. (case-insensitive)
    Alphabetical,
    /// Roman numeral marker: `I`, `II`, `III`, `IV`, etc. (uppercase only)
    Roman,
}

/// Separator style for sequence markers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    /// Period separator: `1.`, `a.`, `I.`
    Period,
    /// Closing parenthesis: `1)`, `a)`, `I)`
    Parenthesis,
    /// Double parenthesis: `(1)`, `(a)`, `(I)`
    DoubleParens,
}

/// Form of sequence marker (simple vs extended)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Short form: single level marker (e.g., `1.`, `a)`)
    Short,
    /// Extended form: multi-level nested index (e.g., `1.2.3`, `I.a.2`)
    Extended,
}

/// A sequence marker representing numbering/decoration for lists and sessions
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceMarker {
    /// The decoration style (plain, numerical, alphabetical, roman)
    pub style: DecorationStyle,
    /// The separator style (period, parenthesis, double parenthesis)
    pub separator: Separator,
    /// The form (short or extended)
    pub form: Form,
    /// Raw text of the marker as it appears in source (for round-trip fidelity)
    pub raw_text: TextContent,
    /// Source location of the marker
    pub location: Range,
}

impl SequenceMarker {
    /// Create a new sequence marker
    pub fn new(
        style: DecorationStyle,
        separator: Separator,
        form: Form,
        raw_text: TextContent,
        location: Range,
    ) -> Self {
        Self {
            style,
            separator,
            form,
            raw_text,
            location,
        }
    }

    /// Parse a sequence marker from raw text
    ///
    /// # Arguments
    ///
    /// * `text` - The marker text (e.g., "1.", "a)", "(IV)", "1.2.3")
    /// * `location` - Optional source location
    ///
    /// # Returns
    ///
    /// `Some(SequenceMarker)` if the text is a valid marker, `None` otherwise
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let marker = SequenceMarker::parse("1.", None).unwrap();
    /// assert_eq!(marker.style, DecorationStyle::Numerical);
    /// assert_eq!(marker.separator, Separator::Period);
    /// assert_eq!(marker.form, Form::Short);
    /// ```
    pub fn parse(text: &str, location: Option<Range>) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }

        // Check for plain dash (lists only)
        if text == "-" {
            let loc = location
                .unwrap_or_else(|| Range::new(0..1, Position::new(0, 0), Position::new(0, 1)));
            return Some(Self::new(
                DecorationStyle::Plain,
                Separator::Period, // Plain doesn't really have a separator, but use Period as default
                Form::Short,
                TextContent::from_string(text.to_string(), Some(loc.clone())),
                loc,
            ));
        }

        // Check for double parenthesis: (X)
        if text.starts_with('(') && text.ends_with(')') {
            let inner = &text[1..text.len() - 1];
            if let Some(style) = Self::detect_style(inner) {
                let loc = location.unwrap_or_else(|| {
                    Range::new(
                        0..text.len(),
                        Position::new(0, 0),
                        Position::new(0, text.len()),
                    )
                });
                return Some(Self::new(
                    style,
                    Separator::DoubleParens,
                    Form::Short, // Double parens are always short form
                    TextContent::from_string(text.to_string(), Some(loc.clone())),
                    loc,
                ));
            }
        }

        // Check for period or parenthesis separator
        let (separator, has_separator) = if text.ends_with('.') {
            (Separator::Period, true)
        } else if text.ends_with(')') {
            (Separator::Parenthesis, true)
        } else {
            // No explicit separator - check if it looks like an extended form
            if text.contains('.') {
                (Separator::Period, false)
            } else {
                return None;
            }
        };

        // Remove the separator to get the marker content
        let content = if has_separator {
            &text[..text.len() - 1]
        } else {
            text
        };

        // Check if this is an extended form (contains periods)
        let parts: Vec<&str> = content.split('.').collect();
        let form = if parts.len() > 1 {
            Form::Extended
        } else {
            Form::Short
        };

        // Detect style from the first part
        let style = Self::detect_style(parts[0])?;

        // For extended forms, verify all parts are valid
        if form == Form::Extended {
            for part in &parts {
                Self::detect_style(part)?;
            }
        }

        let loc = location.unwrap_or_else(|| {
            Range::new(
                0..text.len(),
                Position::new(0, 0),
                Position::new(0, text.len()),
            )
        });

        Some(Self::new(
            style,
            separator,
            form,
            TextContent::from_string(text.to_string(), Some(loc.clone())),
            loc,
        ))
    }

    /// Detect the decoration style from a marker segment
    fn detect_style(segment: &str) -> Option<DecorationStyle> {
        if segment.is_empty() {
            return None;
        }

        // Check for numerical
        if segment.chars().all(|c| c.is_ascii_digit()) {
            return Some(DecorationStyle::Numerical);
        }

        // Check for roman numeral (uppercase I, V, X, L, C, D, M or lowercase i, v, x, l, c, d, m)
        // Check this BEFORE Alphabetical to correctly classify "I", "V", "ii", etc.
        let is_upper_roman = segment
            .chars()
            .all(|c| matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'));
        let is_lower_roman = segment
            .chars()
            .all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'));
        if is_upper_roman || is_lower_roman {
            return Some(DecorationStyle::Roman);
        }

        // Check for alphabetical (single letter)
        if segment.len() == 1 && segment.chars().next().unwrap().is_alphabetic() {
            return Some(DecorationStyle::Alphabetical);
        }

        None
    }

    /// Get the raw marker text as a string
    pub fn as_str(&self) -> &str {
        self.raw_text.as_string()
    }

    /// Check if this marker is valid for sessions (sessions don't support plain style)
    pub fn is_valid_for_session(&self) -> bool {
        !matches!(self.style, DecorationStyle::Plain)
    }

    /// Check if this marker is valid for lists (lists support all styles)
    pub fn is_valid_for_list(&self) -> bool {
        true
    }
}

impl AstNode for SequenceMarker {
    fn node_type(&self) -> &'static str {
        "SequenceMarker"
    }

    fn display_label(&self) -> String {
        self.as_str().to_string()
    }

    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, visitor: &mut dyn super::super::traits::Visitor) {
        // SequenceMarker is a leaf node, no children to visit
        let _ = visitor;
    }
}

impl fmt::Display for SequenceMarker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SequenceMarker('{}')", self.as_str())
    }
}

impl fmt::Display for DecorationStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecorationStyle::Plain => write!(f, "Plain"),
            DecorationStyle::Numerical => write!(f, "Numerical"),
            DecorationStyle::Alphabetical => write!(f, "Alphabetical"),
            DecorationStyle::Roman => write!(f, "Roman"),
        }
    }
}

impl fmt::Display for Separator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Separator::Period => write!(f, "Period"),
            Separator::Parenthesis => write!(f, "Parenthesis"),
            Separator::DoubleParens => write!(f, "DoubleParens"),
        }
    }
}

impl fmt::Display for Form {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Form::Short => write!(f, "Short"),
            Form::Extended => write!(f, "Extended"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plain_marker() {
        let marker = SequenceMarker::parse("-", None).unwrap();
        assert_eq!(marker.style, DecorationStyle::Plain);
        assert_eq!(marker.form, Form::Short);
        assert_eq!(marker.as_str(), "-");
    }

    #[test]
    fn test_parse_numerical_period() {
        let marker = SequenceMarker::parse("1.", None).unwrap();
        assert_eq!(marker.style, DecorationStyle::Numerical);
        assert_eq!(marker.separator, Separator::Period);
        assert_eq!(marker.form, Form::Short);
        assert_eq!(marker.as_str(), "1.");
    }

    #[test]
    fn test_parse_alphabetical_parenthesis() {
        let marker = SequenceMarker::parse("a)", None).unwrap();
        assert_eq!(marker.style, DecorationStyle::Alphabetical);
        assert_eq!(marker.separator, Separator::Parenthesis);
        assert_eq!(marker.form, Form::Short);
        assert_eq!(marker.as_str(), "a)");
    }

    #[test]
    fn test_parse_roman_double_parens() {
        let marker = SequenceMarker::parse("(IV)", None).unwrap();
        assert_eq!(marker.style, DecorationStyle::Roman);
        assert_eq!(marker.separator, Separator::DoubleParens);
        assert_eq!(marker.form, Form::Short);
        assert_eq!(marker.as_str(), "(IV)");
    }

    #[test]
    fn test_parse_extended_numerical() {
        let marker = SequenceMarker::parse("1.2.3.", None).unwrap();
        assert_eq!(marker.style, DecorationStyle::Numerical);
        assert_eq!(marker.separator, Separator::Period);
        assert_eq!(marker.form, Form::Extended);
        assert_eq!(marker.as_str(), "1.2.3.");
    }

    #[test]
    fn test_parse_extended_mixed() {
        let marker = SequenceMarker::parse("1.a.2)", None).unwrap();
        assert_eq!(marker.style, DecorationStyle::Numerical); // First part determines style
        assert_eq!(marker.separator, Separator::Parenthesis);
        assert_eq!(marker.form, Form::Extended);
        assert_eq!(marker.as_str(), "1.a.2)");
    }

    #[test]
    fn test_parse_lowercase_roman() {
        let marker = SequenceMarker::parse("ii.", None).unwrap();
        assert_eq!(marker.style, DecorationStyle::Roman);
        assert_eq!(marker.separator, Separator::Period);
        assert_eq!(marker.form, Form::Short);
    }

    #[test]
    fn test_parse_extended_with_lowercase_roman() {
        let marker = SequenceMarker::parse("1.a.ii.", None).unwrap();
        assert_eq!(marker.style, DecorationStyle::Numerical);
        assert_eq!(marker.form, Form::Extended);
        assert_eq!(marker.as_str(), "1.a.ii.");
    }

    #[test]
    fn test_parse_invalid_marker() {
        assert!(SequenceMarker::parse("", None).is_none());
        assert!(SequenceMarker::parse("abc", None).is_none());
        assert!(SequenceMarker::parse("1", None).is_none()); // No separator
        assert!(SequenceMarker::parse("()", None).is_none()); // Empty parens
    }

    #[test]
    fn test_session_validity() {
        let plain = SequenceMarker::parse("-", None).unwrap();
        assert!(!plain.is_valid_for_session());
        assert!(plain.is_valid_for_list());

        let numerical = SequenceMarker::parse("1.", None).unwrap();
        assert!(numerical.is_valid_for_session());
        assert!(numerical.is_valid_for_list());
    }

    #[test]
    fn test_display() {
        let marker = SequenceMarker::parse("1.2.3.", None).unwrap();
        assert_eq!(format!("{marker}"), "SequenceMarker('1.2.3.')");
        assert_eq!(format!("{}", marker.style), "Numerical");
        assert_eq!(format!("{}", marker.separator), "Period");
        assert_eq!(format!("{}", marker.form), "Extended");
    }
}
