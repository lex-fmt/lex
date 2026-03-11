//! Document Start Marker Transformation
//!
//! Injects a synthetic `DocumentStart` line token to mark the boundary between
//! document-level metadata (annotations) and document content.
//!
//! This transformation enables grammar rules to reason about document structure
//! and position, particularly for document title parsing.
//!
//! Placement rules:
//! - If no document-level annotations exist: position 0
//! - If document-level annotations exist: immediately after the last annotation
//!
//! The DocumentStart token is synthetic (like Indent/Dedent) - it has no source
//! text but carries structural meaning.

use crate::lex::token::line::{LineToken, LineType};

/// Transformation that injects a DocumentStart marker into the line token stream.
pub struct DocumentStartMarker;

impl DocumentStartMarker {
    pub fn new() -> Self {
        Self
    }

    /// Inject DocumentStart marker into the line token stream.
    ///
    /// The marker is placed:
    /// - At position 0 if there are no document-level annotations
    /// - Immediately after the last document-level annotation otherwise
    ///
    /// Document-level annotations are identified as AnnotationStartLine at indentation
    /// level 0 (not nested within any container).
    pub fn mark(line_tokens: Vec<LineToken>) -> Vec<LineToken> {
        if line_tokens.is_empty() {
            return vec![Self::synthetic_document_start()];
        }

        // Find the position after document-level annotations
        // Document-level annotations are at root level (not indented) and come first
        let insert_pos = Self::find_content_start(&line_tokens);

        let mut result = Vec::with_capacity(line_tokens.len() + 1);

        // Insert tokens before DocumentStart
        result.extend(line_tokens[..insert_pos].iter().cloned());

        // Insert the DocumentStart marker
        result.push(Self::synthetic_document_start());

        // Insert remaining tokens
        result.extend(line_tokens[insert_pos..].iter().cloned());

        result
    }

    /// Find the position where document content starts (after any document-level annotations).
    ///
    /// This scans for the pattern of document-level annotations at the start.
    /// Document-level annotations are:
    /// - AnnotationStartLine at root level
    /// - Followed by their content (possibly including Indent/Dedent for nested content)
    /// - Optionally followed by AnnotationEndLine
    /// - Possibly followed by BlankLines
    fn find_content_start(tokens: &[LineToken]) -> usize {
        let mut pos = 0;
        let mut indent_depth: usize = 0;

        while pos < tokens.len() {
            let line_type = tokens[pos].line_type;

            match line_type {
                // Track indentation to know when we exit an annotation block
                LineType::Indent => {
                    indent_depth += 1;
                    pos += 1;
                }
                LineType::Dedent => {
                    indent_depth = indent_depth.saturating_sub(1);
                    pos += 1;
                }

                // Annotation at root level - skip it and its content
                LineType::AnnotationStartLine if indent_depth == 0 => {
                    pos += 1;
                    // Continue to consume the annotation's content
                }

                // Annotation end at root level - part of document metadata
                LineType::AnnotationEndLine if indent_depth == 0 => {
                    pos += 1;
                }

                // Blank lines between annotations at root level - skip
                LineType::BlankLine if indent_depth == 0 => {
                    // Check if there's another annotation coming after blank lines
                    let mut lookahead = pos + 1;
                    while lookahead < tokens.len()
                        && tokens[lookahead].line_type == LineType::BlankLine
                    {
                        lookahead += 1;
                    }

                    // If next non-blank is an annotation, skip all blanks
                    if lookahead < tokens.len()
                        && tokens[lookahead].line_type == LineType::AnnotationStartLine
                    {
                        pos = lookahead;
                    } else {
                        // Blank lines before content - content starts here
                        break;
                    }
                }

                // Content inside an annotation block - continue
                _ if indent_depth > 0 => {
                    pos += 1;
                }

                // Any other token at root level - this is where content starts
                _ => {
                    break;
                }
            }
        }

        pos
    }

    /// Create a synthetic DocumentStart line token.
    fn synthetic_document_start() -> LineToken {
        LineToken {
            source_tokens: vec![],
            token_spans: vec![],
            line_type: LineType::DocumentStart,
        }
    }
}

impl Default for DocumentStartMarker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::token::Token;

    #[allow(clippy::single_range_in_vec_init)]
    fn make_line(line_type: LineType) -> LineToken {
        LineToken {
            source_tokens: vec![Token::Text("test".to_string())],
            token_spans: vec![0..4],
            line_type,
        }
    }

    #[allow(clippy::single_range_in_vec_init)]
    fn make_blank() -> LineToken {
        LineToken {
            source_tokens: vec![Token::BlankLine(Some("\n".to_string()))],
            token_spans: vec![0..1],
            line_type: LineType::BlankLine,
        }
    }

    fn make_indent() -> LineToken {
        LineToken {
            source_tokens: vec![],
            token_spans: vec![],
            line_type: LineType::Indent,
        }
    }

    fn make_dedent() -> LineToken {
        LineToken {
            source_tokens: vec![],
            token_spans: vec![],
            line_type: LineType::Dedent,
        }
    }

    #[test]
    fn test_empty_document() {
        let tokens: Vec<LineToken> = vec![];
        let result = DocumentStartMarker::mark(tokens);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line_type, LineType::DocumentStart);
    }

    #[test]
    fn test_no_annotations() {
        // Document: ParagraphLine, BlankLine, ParagraphLine
        let tokens = vec![
            make_line(LineType::ParagraphLine),
            make_blank(),
            make_line(LineType::ParagraphLine),
        ];

        let result = DocumentStartMarker::mark(tokens);

        // DocumentStart should be at position 0
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].line_type, LineType::DocumentStart);
        assert_eq!(result[1].line_type, LineType::ParagraphLine);
        assert_eq!(result[2].line_type, LineType::BlankLine);
        assert_eq!(result[3].line_type, LineType::ParagraphLine);
    }

    #[test]
    fn test_single_annotation_then_content() {
        // Document: AnnotationStartLine, Indent, ParagraphLine, Dedent, ParagraphLine
        let tokens = vec![
            make_line(LineType::AnnotationStartLine),
            make_indent(),
            make_line(LineType::ParagraphLine),
            make_dedent(),
            make_line(LineType::ParagraphLine),
        ];

        let result = DocumentStartMarker::mark(tokens);

        // DocumentStart should be after the annotation block (position 4)
        assert_eq!(result.len(), 6);
        assert_eq!(result[0].line_type, LineType::AnnotationStartLine);
        assert_eq!(result[1].line_type, LineType::Indent);
        assert_eq!(result[2].line_type, LineType::ParagraphLine);
        assert_eq!(result[3].line_type, LineType::Dedent);
        assert_eq!(result[4].line_type, LineType::DocumentStart);
        assert_eq!(result[5].line_type, LineType::ParagraphLine);
    }

    #[test]
    fn test_annotation_with_end_marker() {
        // Document: AnnotationStartLine, Indent, content, Dedent, AnnotationEndLine, content
        let tokens = vec![
            make_line(LineType::AnnotationStartLine),
            make_indent(),
            make_line(LineType::ParagraphLine),
            make_dedent(),
            make_line(LineType::AnnotationEndLine),
            make_line(LineType::ParagraphLine),
        ];

        let result = DocumentStartMarker::mark(tokens);

        // DocumentStart should be after AnnotationEndLine
        assert_eq!(result.len(), 7);
        assert_eq!(result[4].line_type, LineType::AnnotationEndLine);
        assert_eq!(result[5].line_type, LineType::DocumentStart);
        assert_eq!(result[6].line_type, LineType::ParagraphLine);
    }

    #[test]
    fn test_multiple_annotations() {
        // Document: Annotation1, BlankLine, Annotation2, content
        let tokens = vec![
            make_line(LineType::AnnotationStartLine),
            make_indent(),
            make_line(LineType::ParagraphLine),
            make_dedent(),
            make_blank(),
            make_line(LineType::AnnotationStartLine),
            make_indent(),
            make_line(LineType::ParagraphLine),
            make_dedent(),
            make_line(LineType::ParagraphLine),
        ];

        let result = DocumentStartMarker::mark(tokens);

        // DocumentStart should be after both annotations
        assert_eq!(result.len(), 11);
        assert_eq!(result[9].line_type, LineType::DocumentStart);
        assert_eq!(result[10].line_type, LineType::ParagraphLine);
    }

    #[test]
    fn test_blank_lines_before_content() {
        // Document: AnnotationStartLine, content, BlankLine, ParagraphLine
        let tokens = vec![
            make_line(LineType::AnnotationStartLine),
            make_indent(),
            make_line(LineType::ParagraphLine),
            make_dedent(),
            make_blank(),
            make_line(LineType::ParagraphLine),
        ];

        let result = DocumentStartMarker::mark(tokens);

        // DocumentStart should be at position 4 (before blank line)
        // because blank line is part of content, not metadata
        assert_eq!(result.len(), 7);
        assert_eq!(result[4].line_type, LineType::DocumentStart);
        assert_eq!(result[5].line_type, LineType::BlankLine);
        assert_eq!(result[6].line_type, LineType::ParagraphLine);
    }

    #[test]
    fn test_only_annotations() {
        // Document: only annotations, no content
        let tokens = vec![
            make_line(LineType::AnnotationStartLine),
            make_indent(),
            make_line(LineType::ParagraphLine),
            make_dedent(),
        ];

        let result = DocumentStartMarker::mark(tokens);

        // DocumentStart should be at the end
        assert_eq!(result.len(), 5);
        assert_eq!(result[4].line_type, LineType::DocumentStart);
    }

    #[test]
    fn test_synthetic_token_has_no_source() {
        let tokens = vec![make_line(LineType::ParagraphLine)];
        let result = DocumentStartMarker::mark(tokens);

        // The DocumentStart token should have no source tokens
        assert_eq!(result[0].line_type, LineType::DocumentStart);
        assert!(result[0].source_tokens.is_empty());
        assert!(result[0].token_spans.is_empty());
    }
}
