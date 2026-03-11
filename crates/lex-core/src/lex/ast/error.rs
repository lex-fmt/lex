//! Error types for AST operations

use crate::lex::ast::range::Range;
use std::fmt;

#[cfg(test)]
use crate::lex::ast::range::Position;

/// Errors that can occur during AST position lookup operations
#[derive(Debug, Clone)]
pub enum PositionLookupError {
    /// Invalid position format string
    InvalidPositionFormat(String),
    /// Element not found at the specified position
    NotFound { line: usize, column: usize },
}

impl fmt::Display for PositionLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PositionLookupError::InvalidPositionFormat(msg) => {
                write!(f, "Invalid position format: {msg}")
            }
            PositionLookupError::NotFound { line, column } => {
                write!(f, "No element found at position {line}:{column}")
            }
        }
    }
}

impl std::error::Error for PositionLookupError {}

/// Errors that can occur during parsing and AST construction
#[derive(Debug, Clone)]
pub enum ParserError {
    /// Invalid nesting of elements (e.g., Session inside Definition)
    InvalidNesting {
        container: String,
        invalid_child: String,
        invalid_child_text: String,
        location: Range,
        source_context: String,
    },
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParserError::InvalidNesting {
                container,
                invalid_child,
                invalid_child_text,
                location: _,
                source_context,
            } => {
                writeln!(f, "Error: Invalid nesting in {container}")?;
                writeln!(f)?;
                writeln!(f, "{container} cannot contain {invalid_child} elements")?;
                writeln!(f)?;
                write!(f, "{source_context}")?;
                writeln!(f)?;
                writeln!(
                    f,
                    "The parser identified a {} (\"{}\") inside a {}, which violates lex grammar rules.",
                    invalid_child, invalid_child_text.trim(), container
                )
            }
        }
    }
}

impl std::error::Error for ParserError {}

/// Type alias for parser results with boxed errors (reduces stack size)
pub type ParserResult<T> = Result<T, Box<ParserError>>;

/// Format source code context around an error location
///
/// Shows 2 lines before the error, the error line with >> marker, and 2 lines after.
/// All lines are numbered for easy reference.
pub fn format_source_context(source: &str, range: &Range) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let error_line = range.start.line;

    let start_line = error_line.saturating_sub(2);
    let end_line = (error_line + 3).min(lines.len());

    let mut context = String::new();

    for line_num in start_line..end_line {
        let marker = if line_num == error_line { ">>" } else { "  " };
        let display_line_num = line_num + 1; // 1-indexed for display

        if line_num < lines.len() {
            context.push_str(&format!(
                "{} {:3} | {}\n",
                marker, display_line_num, lines[line_num]
            ));
        }
    }

    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_source_context() {
        let source = "line 1\nline 2\nline 3\nerror line\nline 5\nline 6\nline 7";
        let range = Range::new(20..30, Position::new(3, 0), Position::new(3, 10));

        let context = format_source_context(source, &range);

        // Should show lines 2-6 (0-indexed: 1-5)
        assert!(context.contains("line 2"));
        assert!(context.contains(">> "));
        assert!(context.contains("error line"));
        assert!(context.contains("line 5"));
    }
}
