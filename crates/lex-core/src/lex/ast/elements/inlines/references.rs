//! Inline AST nodes shared across formatting, literal, and reference elements.
//!
//! These nodes are intentionally lightweight so the inline parser can be used
//! from unit tests before it is integrated into the higher level AST builders.

/// Sequence of inline nodes produced from a [`TextContent`](crate::lex::ast::TextContent).
/// Reference inline node with raw content and classified type.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceInline {
    pub raw: String,
    pub reference_type: ReferenceType,
}

impl ReferenceInline {
    pub fn new(raw: String) -> Self {
        Self {
            raw,
            reference_type: ReferenceType::NotSure,
        }
    }
}

/// Reference type classification derived from its content.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceType {
    /// `[TK]` or `[TK-identifier]`
    ToCome { identifier: Option<String> },
    /// `[@citation]` with structured citation data.
    Citation(CitationData),
    /// `[^note]`
    FootnoteLabeled { label: String },
    /// `[12]`
    FootnoteNumber { number: u32 },
    /// `[#42]`
    Session { target: String },
    /// `[https://example.com]`
    Url { target: String },
    /// `[./file.txt]`
    File { target: String },
    /// `[Introduction]` or other document references.
    General { target: String },
    /// Unable to classify.
    NotSure,
}

/// Structured citation payload capturing parsed information.
#[derive(Debug, Clone, PartialEq)]
pub struct CitationData {
    pub keys: Vec<String>,
    pub locator: Option<CitationLocator>,
}

/// Citation locator derived from the `p.` / `pp.` segment.
#[derive(Debug, Clone, PartialEq)]
pub struct CitationLocator {
    pub format: PageFormat,
    pub ranges: Vec<PageRange>,
    /// Raw locator string as authored (e.g. `p.45-46`).
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PageFormat {
    P,
    Pp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageRange {
    pub start: u32,
    pub end: Option<u32>,
}
