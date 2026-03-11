use serde::{Deserialize, Serialize};

/// Configuration for the Lex formatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormattingRules {
    /// Number of blank lines before a session title
    pub session_blank_lines_before: usize,

    /// Number of blank lines after a session title
    pub session_blank_lines_after: usize,

    /// Whether to normalize list markers (e.g. all bullets to '-')
    pub normalize_seq_markers: bool,

    /// The character to use for unordered list markers
    pub unordered_seq_marker: char,

    /// Maximum number of consecutive blank lines allowed
    pub max_blank_lines: usize,

    /// String to use for indentation (usually 4 spaces)
    pub indent_string: String,

    /// Whether to preserve trailing blank lines at the end of the document
    pub preserve_trailing_blanks: bool,

    /// Whether to normalize verbatim markers to `::`
    pub normalize_verbatim_markers: bool,
}

impl Default for FormattingRules {
    fn default() -> Self {
        Self {
            session_blank_lines_before: 1,
            session_blank_lines_after: 1,
            normalize_seq_markers: true,
            unordered_seq_marker: '-',
            max_blank_lines: 2,
            indent_string: "    ".to_string(),
            preserve_trailing_blanks: false,
            normalize_verbatim_markers: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rules() {
        let rules = FormattingRules::default();
        assert_eq!(rules.session_blank_lines_before, 1);
        assert_eq!(rules.indent_string, "    ");
    }
}
