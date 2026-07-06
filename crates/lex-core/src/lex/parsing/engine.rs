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
use crate::lex::building::ast_tree::{AstTreeBuilder, BuildOutput};
use crate::lex::parsing::ir::{NodeType, ParseNode};
use crate::lex::token::{to_line_container, LineContainer};

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
) -> Result<BuildOutput, String> {
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
    parse_grammar_tree(tree, source)
}

/// Parse a LineContainer tree into the AST using the declarative grammar engine.
///
/// This is the production parser core: given the LineContainer tree produced by
/// the lexing/grouping pipeline, it runs the declarative grammar matcher and
/// recursive-descent parser, then builds the AST.
///
/// # Arguments
/// * `tree` - The token tree from the lexer (LineContainerToken)
/// * `source` - The original source text (for location tracking)
///
/// # Returns
/// The root session tree if successful
pub fn parse_grammar_tree(tree: LineContainer, source: &str) -> Result<BuildOutput, String> {
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

    // Test helper: group a flat token stream and run the main parser. Applies the
    // same LineTokenGroupingMapper step the lexing pipeline performs before
    // `parse_from_grouped_stream`.
    fn parse_flat(
        tokens: Vec<(crate::lex::token::Token, std::ops::Range<usize>)>,
        source: &str,
    ) -> Result<BuildOutput, String> {
        use crate::lex::lexing::transformations::LineTokenGroupingMapper;

        let mut mapper = LineTokenGroupingMapper::new();
        let grouped_tokens = mapper.map(tokens);
        parse_from_grouped_stream(grouped_tokens, source)
    }

    #[test]
    fn test_parse_simple_paragraphs() {
        // Use tokens from the lexer pipeline
        let source = "Simple paragraph\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let result = parse_flat(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap().root;
        // Should have 1 paragraph with 1 line
        assert!(!root.children.is_empty(), "Should have content");
        assert!(matches!(root.children[0], ContentItem::Paragraph(_)));
    }

    #[test]
    fn test_parse_definition() {
        // Use tokens from the lexer pipeline
        let source = "Definition:\n    This is the definition content\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let result = parse_flat(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap().root;
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

        let result = parse_flat(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap().root;
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

        let result = parse_flat(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap().root;
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

        let result = parse_flat(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap().root;
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

        let root = parse_flat(tokens, source).expect("Parser failed").root;

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

        let root = parse_flat(tokens, source).expect("Parser failed").root;

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
        let source = ":: test.note ::\n";
        let tokens = lex_helper(source).expect("Failed to tokenize");

        let result = parse_flat(tokens, source);
        assert!(result.is_ok(), "Parser should succeed");

        let root = result.unwrap().root;
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

:: test.info ::

Paragraph before session.

1. Session with annotation inside

    :: test.note author="system" ::
        This is an annotated note within a session

    - List item 1
    - List item 2

    Another paragraph in session.

:: test.warning severity=high ::
    - Item in annotated warning
    - Important item

Final paragraph.
"#;

        let tokens = lex_helper(source).expect("Failed to tokenize");

        let root = parse_flat(tokens, source).expect("Parser failed").root;

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

    // ── Document title model (ADR-0002) ────────────────────────────────────

    #[test]
    fn title_promoted_despite_leading_blank_lines() {
        // ADR-0002 drops the leading-blank suppression special case: a lone first
        // paragraph is the title regardless of any leading blank lines.
        let source = "\n\nTitle line\n\nBody paragraph.\n";
        let tokens = lex_helper(source).expect("tokenize");
        let out = parse_flat(tokens, source).expect("parse");
        assert_eq!(
            out.title.as_ref().map(|t| t.as_str()),
            Some("Title line"),
            "leading blanks must no longer suppress the title"
        );
    }

    #[test]
    fn title_promoted_without_leading_blank() {
        let source = "Title line\n\nBody paragraph.\n";
        let tokens = lex_helper(source).expect("tokenize");
        let out = parse_flat(tokens, source).expect("parse");
        assert_eq!(out.title.as_ref().map(|t| t.as_str()), Some("Title line"));
    }

    #[test]
    fn doc_untitled_marker_suppresses_title() {
        // A `:: doc.untitled ::` among the leading document-level annotations
        // suppresses promotion: no title, first paragraph stays in the body.
        let source = ":: doc.untitled ::\n\nFirst paragraph.\n\nSecond paragraph.\n";
        let tokens = lex_helper(source).expect("tokenize");
        let out = parse_flat(tokens, source).expect("parse");
        assert!(out.title.is_none(), "doc.untitled must suppress the title");
        let paragraphs = out
            .root
            .children
            .iter()
            .filter(|c| matches!(c, ContentItem::Paragraph(_)))
            .count();
        assert_eq!(paragraphs, 2, "both paragraphs stay in the body");
    }

    #[test]
    fn first_non_paragraph_element_is_not_a_title() {
        // A document that opens with a session (a title line + indented body)
        // has no document title — that element starts the document.
        let source = "Section:\n\n    Body content.\n";
        let tokens = lex_helper(source).expect("tokenize");
        let out = parse_flat(tokens, source).expect("parse");
        assert!(
            out.title.is_none(),
            "a leading session is not promoted to a title"
        );
    }

    #[test]
    fn colon_first_line_promotes_title_and_subtitle() {
        // A first line ending in a colon + a second line is title + subtitle;
        // the structural colon is stripped from the title content.
        let source = "Sapiens:\nA Brief History of Humankind\n\nBody.\n";
        let tokens = lex_helper(source).expect("tokenize");
        let out = parse_flat(tokens, source).expect("parse");
        let title = out.title.expect("title present");
        assert_eq!(title.as_str(), "Sapiens");
        assert_eq!(title.subtitle_str(), Some("A Brief History of Humankind"));
    }

    #[test]
    fn test_parse_empty_input() {
        let source = "";
        let tokens = lex_helper(source).expect("Failed to tokenize");
        let result = parse_flat(tokens, source);

        assert!(result.is_ok(), "Empty input should parse successfully");
        let root = result.unwrap().root;
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
        let result = parse_flat(tokens, source);

        assert!(
            result.is_ok(),
            "Whitespace-only input should parse successfully"
        );
    }

    #[test]
    fn test_parse_incomplete_annotation_block() {
        let source = r#"
:: test.warning ::
    This is content

No closing marker
"#;
        let tokens = lex_helper(source).expect("Failed to tokenize");
        let result = parse_flat(tokens, source);

        assert!(
            result.is_ok(),
            "Parser should handle incomplete annotations gracefully"
        );
    }
}
