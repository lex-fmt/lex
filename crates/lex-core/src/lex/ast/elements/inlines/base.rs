//! Inline AST nodes shared across formatting, literal, and reference elements.
//!
//! These nodes are intentionally lightweight so the inline parser can be used
//! from unit tests before it is integrated into the higher level AST builders.
//!
//! # Annotations
//!
//! All inline nodes support annotations - metadata that can be attached to provide
//! additional structured information about the node. This is particularly useful when
//! processing inline content into other formats (e.g., parsing math expressions into
//! MathML).
//!
//! For format conversion use cases, annotations should use the `doc.data` label with
//! a `type` parameter indicating the target format:
//!
//! ```rust,ignore
//! use lex_parser::lex::ast::elements::{Annotation, Label, Parameter};
//!
//! let annotation = Annotation::with_parameters(
//!     Label::new("doc.data".to_string()),
//!     vec![Parameter::new("type".to_string(), "mathml".to_string())]
//! );
//! ```

use super::super::annotation::Annotation;
use super::references::ReferenceInline;

/// Sequence of inline nodes produced from a [`TextContent`](crate::lex::ast::TextContent).
pub type InlineContent = Vec<InlineNode>;

/// Inline node variants supported by the initial flat inline parser.
///
/// All variants include an `annotations` field for attaching metadata. Post-processors
/// can populate this field when transforming inline content (e.g., parsing math to MathML).
#[derive(Debug, Clone, PartialEq)]
pub enum InlineNode {
    /// Plain text segment with no formatting.
    Plain {
        text: String,
        annotations: Vec<Annotation>,
    },
    /// Strong emphasis delimited by `*`.
    Strong {
        content: InlineContent,
        annotations: Vec<Annotation>,
    },
    /// Emphasis delimited by `_`.
    Emphasis {
        content: InlineContent,
        annotations: Vec<Annotation>,
    },
    /// Inline code delimited by `` ` ``.
    Code {
        text: String,
        annotations: Vec<Annotation>,
    },
    /// Simple math span delimited by `#`.
    Math {
        text: String,
        annotations: Vec<Annotation>,
    },
    /// Reference enclosed by square brackets.
    Reference {
        data: ReferenceInline,
        annotations: Vec<Annotation>,
    },
}

impl InlineNode {
    /// Creates a plain text node without annotations.
    pub fn plain(text: String) -> Self {
        InlineNode::Plain {
            text,
            annotations: Vec::new(),
        }
    }

    /// Creates a code node without annotations.
    pub fn code(text: String) -> Self {
        InlineNode::Code {
            text,
            annotations: Vec::new(),
        }
    }

    /// Creates a math node without annotations.
    pub fn math(text: String) -> Self {
        InlineNode::Math {
            text,
            annotations: Vec::new(),
        }
    }

    /// Creates a strong node without annotations.
    pub fn strong(content: InlineContent) -> Self {
        InlineNode::Strong {
            content,
            annotations: Vec::new(),
        }
    }

    /// Creates an emphasis node without annotations.
    pub fn emphasis(content: InlineContent) -> Self {
        InlineNode::Emphasis {
            content,
            annotations: Vec::new(),
        }
    }

    /// Creates a reference node without annotations.
    pub fn reference(data: ReferenceInline) -> Self {
        InlineNode::Reference {
            data,
            annotations: Vec::new(),
        }
    }

    /// Returns the plain text from this node when available.
    pub fn as_plain(&self) -> Option<&str> {
        match self {
            InlineNode::Plain { text, .. } => Some(text),
            InlineNode::Code { text, .. } => Some(text),
            InlineNode::Math { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Returns nested inline content for container nodes (strong/emphasis).
    pub fn children(&self) -> Option<&InlineContent> {
        match self {
            InlineNode::Strong { content, .. } | InlineNode::Emphasis { content, .. } => {
                Some(content)
            }
            _ => None,
        }
    }

    /// Returns `true` when this node is plain text.
    pub fn is_plain(&self) -> bool {
        matches!(self, InlineNode::Plain { .. })
    }

    /// Returns a reference to this node's annotations.
    pub fn annotations(&self) -> &[Annotation] {
        match self {
            InlineNode::Plain { annotations, .. }
            | InlineNode::Strong { annotations, .. }
            | InlineNode::Emphasis { annotations, .. }
            | InlineNode::Code { annotations, .. }
            | InlineNode::Math { annotations, .. }
            | InlineNode::Reference { annotations, .. } => annotations,
        }
    }

    /// Returns a mutable reference to this node's annotations.
    pub fn annotations_mut(&mut self) -> &mut Vec<Annotation> {
        match self {
            InlineNode::Plain { annotations, .. }
            | InlineNode::Strong { annotations, .. }
            | InlineNode::Emphasis { annotations, .. }
            | InlineNode::Code { annotations, .. }
            | InlineNode::Math { annotations, .. }
            | InlineNode::Reference { annotations, .. } => annotations,
        }
    }

    /// Adds an annotation to this node.
    pub fn with_annotation(mut self, annotation: Annotation) -> Self {
        self.annotations_mut().push(annotation);
        self
    }

    /// Adds multiple annotations to this node.
    pub fn with_annotations(mut self, mut annotations: Vec<Annotation>) -> Self {
        self.annotations_mut().append(&mut annotations);
        self
    }
}
