//! Parser Engine - Tree Walker and Orchestrator
//!
//!     This module implements the main parsing orchestrator that performs semantic analysis
//!     on line tokens to produce IR nodes.
//!
//!     At the very beginning of parsing we will group line tokens into a tree of LineContainers.
//!     What this gives us is the ability to parse each level in isolation. Because we don't
//!     need to know what a LineContainer has, but only that it is a line container, we can
//!     parse each level with a regular regex. We simply print token names and match the grammar
//!     patterns against them.
//!
//!     When tokens are matched, we create intermediate representation nodes, which carry only
//!     two bits of information: the node matched and which tokens it uses.
//!
//!     This allows us to separate the semantic analysis from the ast building. This is a good
//!     thing overall, but was instrumental during development, as we ran multiple parsers in
//!     parallel and the ast building had to be unified (correct parsing would result in the
//!     same node types + tokens).
//!
//!     The tree walking is completely decoupled from grammar/pattern matching, making it testable
//!     and maintainable independently.
//!
//!     See [grammar](crate::lex::parsing::parser::grammar) for the grammar pattern definitions
//!     used for matching.
use super::parser;
use crate::lex::building::ast_tree::AstTreeBuilder;
use crate::lex::parsing::ir::{NodeType, ParseNode};
use crate::lex::parsing::Session;
use crate::lex::token::{to_line_container, LineContainer, Token};
use std::ops::Range as ByteRange;

/// Parse from grouped token stream (main entry point).
///
/// This entry point accepts TokenStream::Grouped from the lexing pipeline.
/// The pipeline should have applied LineTokenGroupingMapper to group tokens into lines.
///
/// # Arguments
/// * `stream` - TokenStream::Grouped from lexing pipeline
/// * `source` - The original source text (for location tracking)
///
/// # Returns
/// The root session tree if successful
use crate::lex::lexing::transformations::line_token_grouping::GroupedTokens;

pub fn parse_from_grouped_stream(
    grouped_tokens: Vec<GroupedTokens>,
    source: &str,
) -> Result<Session, String> {
    use crate::lex::lexing::transformations::DocumentStartMarker;

    // Convert grouped tokens to line tokens
    let line_tokens: Vec<_> = grouped_tokens
        .into_iter()
        .map(GroupedTokens::into_line_token)
        .collect();

    // Inject DocumentStart marker to mark metadata/content boundary
    let line_tokens = DocumentStartMarker::mark(line_tokens);

    // Build LineContainer tree from line tokens
    let tree = to_line_container::build_line_container(line_tokens);

    // Parse using existing logic
    parse_experimental_v2(tree, source)
}

/// Parse from flat token stream (legacy/test entry point).
///
/// This entry point is kept for backward compatibility with existing tests.
/// Production code should use parse_from_grouped_stream instead.
///
/// # Arguments
/// * `tokens` - Flat vector of (Token, Range) pairs
/// * `source` - The original source text (for location tracking)
///
/// # Returns
/// The root session tree if successful
pub fn parse_from_flat_tokens(
    tokens: Vec<(Token, ByteRange<usize>)>,
    source: &str,
) -> Result<Session, String> {
    // Apply grouping transformation inline for tests/legacy code
    use crate::lex::lexing::transformations::LineTokenGroupingMapper;

    let mut mapper = LineTokenGroupingMapper::new();
    let grouped_tokens = mapper.map(tokens);

    parse_from_grouped_stream(grouped_tokens, source)
}

/// Parse using the new declarative grammar engine (Delivery 2).
///
/// This is the main entry point for the parser using LineContainerToken.
/// It uses the declarative grammar matcher and recursive descent parser.
///
/// # Arguments
/// * `tree` - The token tree from the lexer (LineContainerToken)
/// * `source` - The original source text (for location tracking)
///
/// # Returns
/// The root session tree if successful
pub fn parse_experimental_v2(tree: LineContainer, source: &str) -> Result<Session, String> {
    // Extract children from root container
    let children = match tree {
        LineContainer::Container { children, .. } => children,
        LineContainer::Token(_) => {
            return Err("Expected root container, found single token".to_string())
        }
    };

    // Use declarative grammar engine to parse
    let content = parser::parse_with_declarative_grammar(children, source)?;
    let root_node = ParseNode::new(NodeType::Document, vec![], content);
    let builder = AstTreeBuilder::new(source);
    builder.build(root_node).map_err(|e| format!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::parsing::ContentItem;

    // Helper to prepare flat token stream
    fn lex_helper(
        source: &str,
    ) -> Result<Vec<(crate::lex::token::Token, std::ops::Range<usize>)>, String> {
        let tokens = crate::lex::lexing::tokenize(source);
        Ok(crate::lex::lexing::lex(tokens)?)
    }

    #[test]
    fn test_parse_simple_paragraphs() {
        // Use tokens from the lexer pipeline
        let source = "Simple paragraph\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let result = parse_from_flat_tokens(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap();
        // Should have 1 paragraph with 1 line
        assert!(!root.children.is_empty(), "Should have content");
        assert!(matches!(root.children[0], ContentItem::Paragraph(_)));
    }

    #[test]
    fn test_parse_definition() {
        // Use tokens from the lexer pipeline
        let source = "Definition:\n    This is the definition content\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let result = parse_from_flat_tokens(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap();
        // Should have Definition at root level
        let has_definition = root
            .children
            .iter()
            .any(|item| matches!(item, ContentItem::Definition(_)));
        assert!(has_definition, "Should contain Definition node");
    }

    #[test]
    fn test_parse_session() {
        // Use tokens from the lexer pipeline
        let source = "Session:\n\n    Session content here\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let result = parse_from_flat_tokens(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap();
        // Should have Session at root level (with blank line before content)
        let has_session = root
            .children
            .iter()
            .any(|item| matches!(item, ContentItem::Session(_)));
        assert!(has_session, "Should contain a Session node");
    }

    #[test]
    fn test_parse_session_with_multiple_blank_lines() {
        // Sessions should work with 2+ blank lines between title and content
        let source = "Title Two\n\n\n    Content with two blank lines.\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let result = parse_from_flat_tokens(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap();
        let has_session = root
            .children
            .iter()
            .any(|item| matches!(item, ContentItem::Session(_)));
        assert!(
            has_session,
            "Should parse as Session even with 2+ blank lines. Got: {:?}",
            root.children
                .iter()
                .map(|c| match c {
                    ContentItem::Paragraph(_) => "Paragraph",
                    ContentItem::Session(_) => "Session",
                    ContentItem::BlankLineGroup(_) => "BlankLineGroup",
                    _ => "Other",
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_parse_session_with_three_blank_lines() {
        let source = "Title Three\n\n\n\n    Content with three blank lines.\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let result = parse_from_flat_tokens(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap();
        let has_session = root
            .children
            .iter()
            .any(|item| matches!(item, ContentItem::Session(_)));
        assert!(has_session, "Should parse as Session with 3 blank lines");
    }

    #[test]
    fn test_verbatim_with_double_closing_marker() {
        let source =
            "Code Example:\n\n    function hello() {\n        return \"world\";\n    }\n\n:: javascript ::\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let root = parse_from_flat_tokens(tokens, source).expect("Parser failed");

        let has_verbatim = root
            .children
            .iter()
            .any(|item| matches!(item, ContentItem::VerbatimBlock(_)));
        assert!(
            has_verbatim,
            "Should contain a Verbatim block. Got: {:?}",
            root.children
                .iter()
                .map(|c| match c {
                    ContentItem::Paragraph(_) => "Paragraph",
                    ContentItem::Session(_) => "Session",
                    ContentItem::VerbatimBlock(_) => "Verbatim",
                    ContentItem::BlankLineGroup(_) => "BlankLineGroup",
                    ContentItem::Annotation(_) => "Annotation",
                    _ => "Other",
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_annotations_inside_session() {
        let source = "1. Session\n\n    Some content.\n\n    :: note-editor :: Maybe this could be better rephrased?\n    :: note.author :: Done keeping it simple\n\n    More content.\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let root = parse_from_flat_tokens(tokens, source).expect("Parser failed");

        // Should have a session
        let session = root
            .children
            .iter()
            .find(|item| matches!(item, ContentItem::Session(_)));
        assert!(session.is_some(), "Should contain a Session");

        if let Some(ContentItem::Session(s)) = session {
            // Session should contain annotations
            let annotation_count = s
                .children
                .iter()
                .filter(|item| matches!(item, ContentItem::Annotation(_)))
                .count();
            assert!(
                annotation_count >= 2,
                "Session should contain at least 2 annotations, got {}. Children: {:?}",
                annotation_count,
                s.children
                    .iter()
                    .map(|c| match c {
                        ContentItem::Paragraph(_) => "Paragraph",
                        ContentItem::Annotation(_) => "Annotation",
                        ContentItem::BlankLineGroup(_) => "BlankLineGroup",
                        ContentItem::Session(_) => "Session",
                        _ => "Other",
                    })
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_parse_annotation() {
        // Use tokens from the lexer pipeline
        let source = ":: note ::\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let result = parse_from_flat_tokens(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap();
        // Should have Annotation at root level
        let has_annotation = root
            .children
            .iter()
            .any(|item| matches!(item, ContentItem::Annotation(_)));
        assert!(has_annotation, "Should contain an Annotation node");
    }

    #[test]
    fn test_annotations_combined_trifecta() {
        // Test annotations combined with paragraphs, lists, and sessions
        let source = r#"Document with annotations and trifecta

:: info ::

Paragraph before session.

1. Session with annotation inside

    :: note author="system" ::
        This is an annotated note within a session
    ::

    - List item 1
    - List item 2

    Another paragraph in session.

:: warning severity=high ::
    - Item in annotated warning
    - Important item
::

Final paragraph.
"#;

        let tokens = lex_helper(source).expect("Failed to tokenize");

        let root = parse_from_flat_tokens(tokens, source).expect("Parser failed");

        eprintln!("\n=== ANNOTATIONS + TRIFECTA COMBINED ===");
        eprintln!("Root items count: {}", root.children.len());
        for (i, item) in root.children.iter().enumerate() {
            match item {
                ContentItem::Paragraph(p) => {
                    eprintln!("  [{}] Paragraph: {} lines", i, p.lines.len())
                }
                ContentItem::Annotation(a) => {
                    eprintln!(
                        "  [{}] Annotation: label='{}' content={} items",
                        i,
                        a.data.label.value,
                        a.children.len()
                    )
                }
                ContentItem::Session(s) => {
                    eprintln!("  [{}] Session: {} items", i, s.children.len())
                }
                ContentItem::List(l) => eprintln!("  [{}] List: {} items", i, l.items.len()),
                _ => eprintln!("  [{i}] Other"),
            }
        }

        // Verify mixed content
        let has_annotations = root
            .children
            .iter()
            .any(|item| matches!(item, ContentItem::Annotation(_)));
        let has_paragraphs = root
            .children
            .iter()
            .any(|item| matches!(item, ContentItem::Paragraph(_)));
        let has_sessions = root
            .children
            .iter()
            .any(|item| matches!(item, ContentItem::Session(_)));

        assert!(has_annotations, "Should contain annotations");
        assert!(has_paragraphs, "Should contain paragraphs");
        assert!(has_sessions, "Should contain sessions");
    }

    #[test]
    fn test_parse_empty_input() {
        let source = "";
        let tokens = lex_helper(source).expect("Failed to tokenize");
        let result = parse_from_flat_tokens(tokens, source);

        assert!(result.is_ok(), "Empty input should parse successfully");
        let root = result.unwrap();
        assert_eq!(
            root.children.len(),
            0,
            "Empty document should have no children"
        );
    }

    #[test]
    fn test_parse_only_whitespace() {
        let source = "   \n\n   \n";
        let tokens = lex_helper(source).expect("Failed to tokenize");
        let result = parse_from_flat_tokens(tokens, source);

        assert!(
            result.is_ok(),
            "Whitespace-only input should parse successfully"
        );
    }

    #[test]
    fn test_parse_incomplete_annotation_block() {
        let source = r#"
:: warning ::
    This is content

No closing marker
"#;
        let tokens = lex_helper(source).expect("Failed to tokenize");
        let result = parse_from_flat_tokens(tokens, source);

        assert!(
            result.is_ok(),
            "Parser should handle incomplete annotations gracefully"
        );
    }
}
