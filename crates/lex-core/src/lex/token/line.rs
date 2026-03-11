//! Line-based token types for the lexer pipeline
//!
//!     This module contains token types specific to the line-based lexer pipeline. Being line
//!     based, all the grammar needs is to have line tokens in order to parse any level of elements.
//!     Only annotations and end of verbatim blocks use data nodes, that means that pretty much all
//!     of Lex needs to be parsed from naturally occurring text lines, indentation and blank lines.
//!
//!     Since this still is happening in the lexing stage, each line must be tokenized into one
//!     category. In the real world, a line might be more than one possible category. For example a
//!     line might have a sequence marker and a subject marker (for example "1. Recap:").
//!
//!     For this reason, line tokens can be OR tokens at times, and at other times the order of
//!     line categorization is crucial to getting the right result. While there are only a few
//!     consequential marks in lines (blank, data, subject, list) having them denormalized is
//!     required to have parsing simpler.
//!
//!     The LineType enum is the definitive set: blank, annotation start/end, data, subject, list,
//!     subject-or-list-item, paragraph, dialog, indent, dedent. Containers are a separate
//!     structural node, not a line token.
//!
//! Line Types
//!
//!     These are the line tokens:
//!
//!         - BlankLine: empty or whitespace only
//!         - AnnotationEndLine: a line starting with :: marker and having no further content
//!         - AnnotationStartLine: a data node + lex marker
//!         - DataLine: :: label params? (no closing :: marker)
//!         - SubjectLine: Line ending with colon (could be subject/definition/session title)
//!         - ListLine: Line starting with list marker (-, 1., a., I., etc.)
//!         - SubjectOrListItemLine: Line starting with list marker and ending with colon
//!         - ParagraphLine: Any other line (paragraph text)
//!         - DialogLine: a line that starts with a dash, but is marked not to be a list item.
//!         - Indent / Dedent: structural markers passed through from indentation handling.
//!         - DocumentStart: synthetic marker for document content boundary.
//!
//!     And to represent a group of lines at the same level, there is a LineContainer.
//!
//!     See [classify_line_tokens](crate::lex::lexing::line_classification::classify_line_tokens)
//!     for the classification logic and ordering.

use std::fmt;

use super::core::Token;

/// A line token represents one logical line created from grouped raw tokens.
///
/// Line tokens are produced by the line token transformation,
/// which groups raw tokens into semantic line units. Each line token stores:
/// - The original raw tokens that created it (for location information and AST construction)
/// - The line type (what kind of line this is)
/// - Individual token spans (to enable byte-accurate text extraction from token subsets)
///
/// By preserving raw tokens and their individual spans, we can later
/// pass them directly to existing AST constructors (using the same unified approach as the
/// the parser), which handles all location tracking and AST node creation automatically.
///
/// Note: LineToken does NOT store an aggregate source_span. The AST construction facade
/// will compute bounding boxes from the individual token_spans when needed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LineToken {
    /// The original raw tokens that comprise this line
    pub source_tokens: Vec<Token>,

    /// The byte range in source code for each token
    /// Must be the same length as source_tokens
    pub token_spans: Vec<std::ops::Range<usize>>,

    /// The type/classification of this line
    pub line_type: LineType,
}

impl LineToken {
    /// Get source tokens as (Token, Range<usize>) pairs.
    ///
    /// This creates owned pairs from the separate source_tokens and token_spans vectors.
    /// Used by the AST construction facade to get tokens in the format expected by
    /// the token processing utilities.
    ///
    /// Note: LineToken stores tokens and spans separately for serialization efficiency.
    /// This method creates the paired format needed for location tracking.
    pub fn source_token_pairs(&self) -> Vec<(Token, std::ops::Range<usize>)> {
        self.source_tokens
            .iter()
            .zip(self.token_spans.iter())
            .map(|(token, span)| (token.clone(), span.clone()))
            .collect()
    }
}

/// The classification of a line token
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LineType {
    /// Blank line (empty or whitespace only)
    BlankLine,

    /// Annotation end line: a line starting with :: marker and having no further content
    AnnotationEndLine,

    /// Annotation start line: follows annotation grammar <lex-marker><space><label>(<space><parameters>)? <lex-marker> <content>?
    AnnotationStartLine,

    /// Data line: :: label params? (no closing :: marker)
    DataLine,

    /// Line ending with colon (could be subject/definition/session title)
    SubjectLine,

    /// Line starting with list marker (-, 1., a., I., etc.)
    ListLine,

    /// Line starting with list marker and ending with colon (subject and list item combined)
    SubjectOrListItemLine,

    /// Any other line (paragraph text)
    ParagraphLine,

    /// Line that is part of a dialog
    DialogLine,

    /// Indentation marker (pass-through from prior transformation)
    Indent,

    /// Dedentation marker (pass-through from prior transformation)
    Dedent,

    /// Document start marker (synthetic)
    ///
    /// Marks the boundary between document-level metadata (annotations) and document content.
    /// Injected by DocumentStartMarker transformation at:
    /// - Position 0 if no document-level annotations
    /// - Immediately after the last document-level annotation otherwise
    ///
    /// This enables grammar rules to reason about document structure and position.
    DocumentStart,
}

impl fmt::Display for LineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            LineType::BlankLine => "BLANK_LINE",
            LineType::AnnotationEndLine => "ANNOTATION_END_LINE",
            LineType::AnnotationStartLine => "ANNOTATION_START_LINE",
            LineType::DataLine => "DATA_LINE",
            LineType::SubjectLine => "SUBJECT_LINE",
            LineType::ListLine => "LIST_LINE",
            LineType::SubjectOrListItemLine => "SUBJECT_OR_LIST_ITEM_LINE",
            LineType::ParagraphLine => "PARAGRAPH_LINE",
            LineType::DialogLine => "DIALOG_LINE",
            LineType::Indent => "INDENT",
            LineType::Dedent => "DEDENT",
            LineType::DocumentStart => "DOCUMENT_START",
        };
        write!(f, "{name}")
    }
}

impl LineType {
    /// Format token type as grammar notation: `<token-name>`
    ///
    /// Converts UPPER_CASE_WITH_UNDERSCORES to <lower-case-with-dashes>
    ///
    /// Examples:
    /// - BlankLine -> `<blank-line>`
    /// - AnnotationStartLine -> `<annotation-start-line>`
    /// - SubjectLine -> `<subject-line>`
    pub fn to_grammar_string(&self) -> String {
        let name = match self {
            LineType::BlankLine => "blank-line",
            LineType::AnnotationEndLine => "annotation-end-line",
            LineType::AnnotationStartLine => "annotation-start-line",
            LineType::DataLine => "data-line",
            LineType::SubjectLine => "subject-line",
            LineType::ListLine => "list-line",
            LineType::SubjectOrListItemLine => "subject-or-list-item-line",
            LineType::ParagraphLine => "paragraph-line",
            LineType::DialogLine => "dialog-line",
            LineType::Indent => "indent",
            LineType::Dedent => "dedent",
            LineType::DocumentStart => "document-start-line",
        };
        format!("<{name}>")
    }
}

/// The primary tree structure for the lexer output.
///
/// This is a recursive enum representing the complete hierarchical structure of line tokens.
/// Every node in the tree is either a line token or a container of child nodes.
///
/// The tree is built by processing Indent/Dedent markers:
/// - Token variant: A single line token (e.g., SubjectLine, ParagraphLine, ListLine)
/// - Container variant: A grouped set of child nodes at a deeper indentation level
///
/// This structure allows the parser to match patterns by checking token types while
/// maintaining the complete source structure (source tokens, nesting).
///
/// Note: Container does NOT store an aggregate source_span. The AST construction facade
/// will compute bounding boxes by recursively unrolling children to their source tokens.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LineContainer {
    /// A single line token
    Token(LineToken),

    /// A container of child nodes (represents indented content or grouped lines at same level)
    Container { children: Vec<LineContainer> },
}

impl LineContainer {
    /// Check if this container is empty (only valid for root containers)
    pub fn is_empty(&self) -> bool {
        match self {
            LineContainer::Token(_) => false,
            LineContainer::Container { children, .. } => children.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_indented_marker() {
        use crate::lex::lexing::tokenize;
        let source = "  ::";
        let tokens_with_range = tokenize(source);
        let tokens: Vec<crate::lex::token::Token> =
            tokens_with_range.into_iter().map(|(t, _)| t).collect();
        println!("Tokens: {tokens:?}");
    }

    #[test]
    fn test_token_type_to_grammar_string() {
        assert_eq!(LineType::BlankLine.to_grammar_string(), "<blank-line>");
        assert_eq!(
            LineType::AnnotationStartLine.to_grammar_string(),
            "<annotation-start-line>"
        );
        assert_eq!(
            LineType::AnnotationEndLine.to_grammar_string(),
            "<annotation-end-line>"
        );
        assert_eq!(LineType::SubjectLine.to_grammar_string(), "<subject-line>");
        assert_eq!(LineType::ListLine.to_grammar_string(), "<list-line>");
        assert_eq!(
            LineType::SubjectOrListItemLine.to_grammar_string(),
            "<subject-or-list-item-line>"
        );
        assert_eq!(
            LineType::ParagraphLine.to_grammar_string(),
            "<paragraph-line>"
        );
        assert_eq!(LineType::Indent.to_grammar_string(), "<indent>");
        assert_eq!(LineType::Dedent.to_grammar_string(), "<dedent>");
        assert_eq!(
            LineType::DocumentStart.to_grammar_string(),
            "<document-start-line>"
        );
    }

    #[test]
    fn test_token_sequence_formatting() {
        // Test creating a sequence of tokens and formatting them
        let tokens = [
            LineType::SubjectLine,
            LineType::Indent,
            LineType::ParagraphLine,
            LineType::Dedent,
        ];

        let formatted = tokens
            .iter()
            .map(|t| t.to_grammar_string())
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(formatted, "<subject-line><indent><paragraph-line><dedent>");
    }

    #[test]
    fn test_blank_line_group_formatting() {
        let tokens = [
            LineType::BlankLine,
            LineType::BlankLine,
            LineType::BlankLine,
        ];

        let formatted = tokens
            .iter()
            .map(|t| t.to_grammar_string())
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(formatted, "<blank-line><blank-line><blank-line>");
    }

    #[test]
    fn test_complex_pattern_formatting() {
        // Session pattern: blank + content + blank + container
        let tokens = [
            LineType::BlankLine,
            LineType::SubjectLine,
            LineType::BlankLine,
            LineType::Indent,
            LineType::ParagraphLine,
            LineType::Dedent,
        ];

        let formatted = tokens
            .iter()
            .map(|t| t.to_grammar_string())
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(
            formatted,
            "<blank-line><subject-line><blank-line><indent><paragraph-line><dedent>"
        );
    }

    #[test]
    fn test_line_token_source_token_pairs() {
        // Test that LineToken can provide source tokens in paired format
        let line_token = LineToken {
            source_tokens: vec![
                Token::Text("hello".to_string()),
                Token::Whitespace(1),
                Token::Text("world".to_string()),
            ],
            token_spans: vec![0..5, 5..6, 6..11],
            line_type: LineType::ParagraphLine,
        };

        let pairs = line_token.source_token_pairs();
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0].1, 0..5);
        assert_eq!(pairs[1].1, 5..6);
        assert_eq!(pairs[2].1, 6..11);

        // Verify tokens match
        match &pairs[0].0 {
            Token::Text(s) => assert_eq!(s, "hello"),
            _ => panic!("Expected Text token"),
        }
    }
}
