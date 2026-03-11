//! Intermediate Representation for Parsers
//!
//! This module defines the Intermediate Representation (IR) that parsers produce.
//! The IR is a tree of `ParseNode`s, which describes the desired AST structure
//! without coupling the parser to the AST building logic.

use crate::lex::lexing::Token;
use crate::lex::token::LineToken;
use std::ops::Range;

/// Type alias for token with location
pub type TokenLocation = (Token, Range<usize>);

/// The type of a node in the parse tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeType {
    Document,
    DocumentStart,
    Paragraph,
    Session,
    ListItem,
    List,
    Definition,
    Annotation,
    VerbatimBlock,
    BlankLineGroup,
}

/// Additional payload carried by specific parse nodes.
#[derive(Debug, Clone)]
pub enum ParseNodePayload {
    /// Raw line tokens needed to build a verbatim block (subject + content lines + closing data)
    VerbatimBlock {
        subject: LineToken,
        content_lines: Vec<LineToken>,
        closing_data_tokens: Vec<TokenLocation>,
    },
}

/// A node in the parse tree.
#[derive(Debug, Clone)]
pub struct ParseNode {
    pub node_type: NodeType,
    pub tokens: Vec<TokenLocation>,
    pub children: Vec<ParseNode>,
    pub payload: Option<ParseNodePayload>,
}

impl ParseNode {
    /// Creates a new `ParseNode`.
    pub fn new(node_type: NodeType, tokens: Vec<TokenLocation>, children: Vec<ParseNode>) -> Self {
        Self {
            node_type,
            tokens,
            children,
            payload: None,
        }
    }

    /// Attach a payload to this node.
    pub fn with_payload(mut self, payload: ParseNodePayload) -> Self {
        self.payload = Some(payload);
        self
    }
}
