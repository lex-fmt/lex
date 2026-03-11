//! AST Builder from ParseNode IR
//!
//!     This module contains the `AstTreeBuilder`, which walks the `ParseNode` tree produced
//!     by the parser and constructs the final AST.
//!
//!     From the IR nodes, we build the actual AST nodes. During this step:
//!         1. We unroll source tokens so that ast nodes have access to token values.
//!         2. The location from tokens is used to calculate the location for the ast node.
//!         3. The location is transformed from byte range to a dual byte range + line:column
//!            position.
//!
//!     At this stage we create the root session tree that will later be attached to the
//!     `Document` node during assembling.
//!
//!     See [location](super::location) for location calculation utilities.

use crate::lex::ast::elements::typed_content::{self, ContentElement, SessionContent};
use crate::lex::ast::error::{format_source_context, ParserError, ParserResult};
use crate::lex::ast::range::SourceLocation;
use crate::lex::ast::text_content::TextContent;
use crate::lex::ast::{AstNode, ContentItem, ListItem, Range, Session};
use crate::lex::building::api as ast_api;
use crate::lex::building::location::compute_location_from_locations;
use crate::lex::parsing::ir::{NodeType, ParseNode, ParseNodePayload, TokenLocation};

/// A builder that constructs an AST from a `ParseNode` tree.
pub struct AstTreeBuilder<'a> {
    source: &'a str,
    source_location: SourceLocation,
}

impl<'a> AstTreeBuilder<'a> {
    /// Creates a new `AstTreeBuilder`.
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            source_location: SourceLocation::new(source),
        }
    }

    /// Builds the document's root Session from a root `ParseNode`.
    pub fn build(&self, root_node: ParseNode) -> ParserResult<Session> {
        if root_node.node_type != NodeType::Document {
            panic!("Expected a Document node at the root");
        }

        // Extract document title if present
        let (document_title, title_skip_range) = self.extract_document_title(&root_node.children);

        // Build content items, filtering out structural markers and the title node
        let filtered_children: Vec<ParseNode> = root_node
            .children
            .into_iter()
            .enumerate()
            .filter(|(idx, node)| {
                // Skip DocumentStart
                if node.node_type == NodeType::DocumentStart {
                    return false;
                }
                // Skip the nodes used for title
                if let Some(range) = &title_skip_range {
                    if range.contains(idx) {
                        return false;
                    }
                }
                true
            })
            .map(|(_, node)| node)
            .collect();

        let content = self.build_content_items(filtered_children)?;
        let content_locations: Vec<Range> =
            content.iter().map(|item| item.range().clone()).collect();
        let root_location = compute_location_from_locations(&content_locations);
        let session_content = typed_content::into_session_contents(content);
        let root = Session::new(document_title, session_content);
        Ok(root.at(root_location))
    }

    /// Extract document title from the parsed children.
    ///
    /// The document title is determined by the following pattern at the start of content (after DocumentStart):
    /// 1. A single Paragraph node
    /// 2. Followed by a BlankLineGroup
    /// 3. NOT followed by a Container (implicitly handled because if it were a container, it wouldn't be a sibling Paragraph)
    ///
    /// If this pattern matches, the text of the Paragraph is used as the title, and the Paragraph node
    /// should be skipped during content assembly.
    fn extract_document_title(
        &self,
        children: &[ParseNode],
    ) -> (TextContent, Option<std::ops::Range<usize>>) {
        let mut start_idx = 0;

        // Skip DocumentStart if present
        if start_idx < children.len() && children[start_idx].node_type == NodeType::DocumentStart {
            start_idx += 1;
        }

        // Check for Paragraph + BlankLineGroup pattern
        if start_idx + 1 < children.len() {
            let first = &children[start_idx];
            let second = &children[start_idx + 1];

            if first.node_type == NodeType::Paragraph
                && second.node_type == NodeType::BlankLineGroup
            {
                // Found document title!
                let title = ast_api::text_content_from_tokens(
                    first.tokens.clone(),
                    self.source,
                    &self.source_location,
                );
                // Return title and the range of nodes to skip (paragraph + blank line group)
                return (title, Some(start_idx..start_idx + 2));
            }
        }

        (
            TextContent::from_string(String::new(), None::<Range>),
            None::<std::ops::Range<usize>>,
        )
    }

    /// Builds a vector of `ContentItem`s from a vector of `ParseNode`s.
    fn build_content_items(&self, nodes: Vec<ParseNode>) -> ParserResult<Vec<ContentItem>> {
        nodes
            .into_iter()
            .map(|node| self.build_content_item(node))
            .collect()
    }

    /// Builds a single `ContentItem` from a `ParseNode`.
    fn build_content_item(&self, node: ParseNode) -> ParserResult<ContentItem> {
        match node.node_type {
            NodeType::Paragraph => Ok(self.build_paragraph(node)),
            NodeType::Session => self.build_session(node),
            NodeType::List => self.build_list(node),
            NodeType::Definition => self.build_definition(node),
            NodeType::Annotation => self.build_annotation(node),
            NodeType::VerbatimBlock => Ok(self.build_verbatim_block(node)),
            NodeType::BlankLineGroup => Ok(self.build_blank_line_group(node)),
            _ => panic!("Unexpected node type"),
        }
    }

    fn build_paragraph(&self, node: ParseNode) -> ContentItem {
        let token_lines = group_tokens_by_line(node.tokens);
        ast_api::paragraph_from_token_lines(token_lines, self.source, &self.source_location)
    }

    fn build_session(&self, node: ParseNode) -> ParserResult<ContentItem> {
        let title_tokens = node.tokens;
        let content = self.build_session_content(node.children)?;
        Ok(ast_api::session_from_tokens(
            title_tokens,
            content,
            self.source,
            &self.source_location,
        ))
    }

    fn build_definition(&self, node: ParseNode) -> ParserResult<ContentItem> {
        let subject_tokens = node.tokens;
        let content = self.build_general_content(node.children, "Definition")?;
        Ok(ast_api::definition_from_tokens(
            subject_tokens,
            content,
            self.source,
            &self.source_location,
        ))
    }

    fn build_list(&self, node: ParseNode) -> ParserResult<ContentItem> {
        let list_items: Result<Vec<_>, _> = node
            .children
            .into_iter()
            .map(|child_node| self.build_list_item(child_node))
            .collect();
        Ok(ast_api::list_from_items(list_items?))
    }

    fn build_list_item(&self, node: ParseNode) -> ParserResult<ListItem> {
        let marker_tokens = node.tokens;
        let content = self.build_general_content(node.children, "ListItem")?;
        Ok(ast_api::list_item_from_tokens(
            marker_tokens,
            content,
            self.source,
            &self.source_location,
        ))
    }

    fn build_annotation(&self, node: ParseNode) -> ParserResult<ContentItem> {
        let header_tokens = node.tokens;
        let content = self.build_general_content(node.children, "Annotation")?;
        Ok(ast_api::annotation_from_tokens(
            header_tokens,
            content,
            self.source,
            &self.source_location,
        ))
    }

    fn build_verbatim_block(&self, mut node: ParseNode) -> ContentItem {
        let payload = node
            .payload
            .take()
            .expect("Parser must attach verbatim payload");
        let ParseNodePayload::VerbatimBlock {
            subject,
            content_lines,
            closing_data_tokens,
        } = payload;

        let closing_data =
            ast_api::data_from_tokens(closing_data_tokens, self.source, &self.source_location);

        ast_api::verbatim_block_from_lines(
            &subject,
            &content_lines,
            closing_data,
            self.source,
            &self.source_location,
        )
    }

    fn build_blank_line_group(&self, node: ParseNode) -> ContentItem {
        ast_api::blank_line_group_from_tokens(node.tokens, self.source, &self.source_location)
    }

    fn build_session_content(&self, nodes: Vec<ParseNode>) -> ParserResult<Vec<SessionContent>> {
        nodes
            .into_iter()
            .map(|node| self.build_content_item(node).map(SessionContent::from))
            .collect()
    }

    fn build_general_content(
        &self,
        nodes: Vec<ParseNode>,
        context: &str,
    ) -> ParserResult<Vec<ContentElement>> {
        nodes
            .into_iter()
            .map(|node| {
                self.build_content_item(node).and_then(|item| {
                    let location = item.range().clone();

                    // Extract text snippet from source for the invalid item (Session title)
                    // Get the line at the start of the error location
                    let source_lines: Vec<&str> = self.source.lines().collect();
                    let error_line_num = location.start.line;
                    let session_title = if error_line_num < source_lines.len() {
                        source_lines[error_line_num]
                    } else {
                        ""
                    };

                    ContentElement::try_from(item).map_err(|_| {
                        Box::new(ParserError::InvalidNesting {
                            container: context.to_string(),
                            invalid_child: "Session".to_string(),
                            invalid_child_text: session_title.to_string(),
                            location: location.clone(),
                            source_context: format_source_context(self.source, &location),
                        })
                    })
                })
            })
            .collect()
    }
}

/// Group a flat vector of tokens into lines (split by Newline tokens).
fn group_tokens_by_line(tokens: Vec<TokenLocation>) -> Vec<Vec<TokenLocation>> {
    if tokens.is_empty() {
        return vec![];
    }

    let mut lines: Vec<Vec<TokenLocation>> = vec![];
    let mut current_line: Vec<TokenLocation> = vec![];

    for token_location in tokens {
        if matches!(token_location.0, crate::lex::lexing::Token::BlankLine(_)) {
            lines.push(current_line);
            current_line = vec![];
        } else {
            current_line.push(token_location);
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::token::{LineToken, LineType, Token};

    fn parse_node(
        node_type: NodeType,
        tokens: Vec<TokenLocation>,
        children: Vec<ParseNode>,
    ) -> ParseNode {
        ParseNode {
            node_type,
            tokens,
            children,
            payload: None,
        }
    }

    #[test]
    fn build_general_content_rejects_nested_session() {
        let source = "Term\nchild\n";
        let builder = AstTreeBuilder::new(source);

        let nested_session = parse_node(
            NodeType::Session,
            vec![(Token::Text("child".into()), 5..10)],
            vec![],
        );

        let err = builder
            .build_general_content(vec![nested_session], "Definition")
            .expect_err("sessions should not be allowed in general content");

        match *err {
            ParserError::InvalidNesting {
                ref container,
                ref invalid_child,
                ref invalid_child_text,
                ref location,
                ..
            } => {
                assert_eq!(container, "Definition");
                assert_eq!(invalid_child, "Session");
                assert_eq!(invalid_child_text.trim(), "child");
                assert_eq!(location.start.line, 1);
            }
        }
    }

    #[test]
    fn group_tokens_by_line_handles_blank_boundaries() {
        let tokens = vec![
            (Token::Text("a".into()), 0..1),
            (Token::BlankLine(Some("\n".into())), 1..2),
            (Token::BlankLine(Some("\n".into())), 2..3),
            (Token::Text("b".into()), 3..4),
        ];

        let lines = group_tokens_by_line(tokens);

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].len(), 1); // before blank line
        assert!(lines[1].is_empty()); // consecutive blank line produces empty bucket
        assert_eq!(lines[2].len(), 1); // after blanks
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn build_verbatim_block_preserves_payload_data() {
        let source = "subject\ncontent\nclose\n";
        let builder = AstTreeBuilder::new(source);

        let subject_token = LineToken {
            source_tokens: vec![Token::Text("subject".into())],
            token_spans: vec![0..7],
            line_type: LineType::SubjectLine,
        };

        let content_line = LineToken {
            source_tokens: vec![Token::Text("content".into())],
            token_spans: vec![8..15],
            line_type: LineType::ParagraphLine,
        };

        let payload = ParseNodePayload::VerbatimBlock {
            subject: subject_token,
            content_lines: vec![content_line],
            closing_data_tokens: vec![(Token::Text("close".into()), 16..21)],
        };

        let node = ParseNode {
            node_type: NodeType::VerbatimBlock,
            tokens: vec![],
            children: vec![],
            payload: Some(payload),
        };

        let item = builder.build_verbatim_block(node);

        if let ContentItem::VerbatimBlock(verbatim) = item {
            assert_eq!(verbatim.subject.as_string(), "subject");
            assert_eq!(verbatim.children.len(), 1);
            assert_eq!(verbatim.closing_data.label.value, "close");
        } else {
            panic!("expected verbatim block");
        }
    }

    #[test]
    fn test_document_title_parsing() {
        // Test that the AST builder correctly extracts title
        let source = "My Document Title\n\nContent paragraph.\n";
        let builder = AstTreeBuilder::new(source);

        let content_tokens = vec![
            (Token::Text("Content paragraph.".into()), 19..37),
            (Token::BlankLine(Some("\n".into())), 37..38),
        ];

        let root_node = ParseNode {
            node_type: NodeType::Document,
            tokens: vec![],
            children: vec![
                ParseNode {
                    node_type: NodeType::Paragraph,
                    tokens: vec![(Token::Text("My Document Title".to_string()), 0..17)],
                    children: vec![],
                    payload: None,
                },
                ParseNode {
                    node_type: NodeType::BlankLineGroup,
                    tokens: vec![],
                    children: vec![],
                    payload: None,
                },
                ParseNode {
                    node_type: NodeType::Paragraph,
                    tokens: content_tokens,
                    children: vec![],
                    payload: None,
                },
            ],
            payload: None,
        };

        let session = builder.build(root_node).expect("Builder failed");

        assert_eq!(session.title.as_string(), "My Document Title");
        assert_eq!(session.children.len(), 1);
        if let ContentItem::Paragraph(p) = &session.children[0] {
            assert_eq!(p.text(), "Content paragraph.");
        } else {
            panic!("Expected paragraph");
        }
    }

    #[test]
    fn test_document_title_parsing_no_title() {
        // Test that documents starting with a Session do not have a document title extracted
        let source = "# Section 1\n\nContent.\n";
        let builder = AstTreeBuilder::new(source);

        let root_node = ParseNode {
            node_type: NodeType::Document,
            tokens: vec![],
            children: vec![ParseNode {
                node_type: NodeType::Session,
                tokens: vec![(Token::Text("Section 1".into()), 2..11)],
                children: vec![
                    ParseNode {
                        node_type: NodeType::BlankLineGroup,
                        tokens: vec![],
                        children: vec![],
                        payload: None,
                    },
                    ParseNode {
                        node_type: NodeType::Paragraph,
                        tokens: vec![(Token::Text("Content.".into()), 13..21)],
                        children: vec![],
                        payload: None,
                    },
                ],
                payload: None,
            }],
            payload: None,
        };

        let session = builder.build(root_node).expect("Builder failed");

        assert_eq!(session.title.as_string(), "");
        assert_eq!(session.children.len(), 1); // 1 session
    }
}
