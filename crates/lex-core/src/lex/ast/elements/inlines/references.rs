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
    /// Resolved word anchor for an inline reference (one that shares its line
    /// with other text). Populated by the anchor-resolution pass after inline
    /// parsing; `None` for a freshly built node, for a reference that is the
    /// only token on its line (those are handled as reference lines), or for a
    /// reference with no word to anchor (an empty line). See
    /// `specs/.../references-general.lex` §2.3.1.
    pub word_anchor: Option<WordAnchor>,
}

impl ReferenceInline {
    pub fn new(raw: String) -> Self {
        Self {
            raw,
            reference_type: ReferenceType::NotSure,
            word_anchor: None,
        }
    }
}

/// The word a single inline reference anchors, per §2.3.1.
///
/// The anchor is a single word taken from the same line as the reference:
/// the word immediately *preceding* it (default), or — when the reference is
/// the first token on the line and text follows — the word immediately
/// *following* it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordAnchor {
    /// The anchored word text (no surrounding whitespace or punctuation kept
    /// beyond the word itself).
    pub word: String,
    /// Which side of the reference the word was taken from.
    pub direction: AnchorDirection,
}

/// Side of an inline reference its word anchor was taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorDirection {
    /// The word immediately before the reference (default).
    Preceding,
    /// The word immediately after the reference (reference is first on line).
    Following,
}

/// Anchor capability of a reference type, per §2.3.4.
///
/// Whole-element anchoring (reference lines) applies only to *link-like*
/// references — those that render as a span of linked text. Marker-style
/// references render as markers/superscripts, so a whole-element anchor has no
/// visual meaning for them; on a reference line they self-link or resolve as
/// usual. This split is a property of the reference *type*, not of position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    /// Renders as a span of linked text: may take a whole-element anchor when
    /// it appears as a reference line. (Url, File, Session, General.)
    WholeLineCapable,
    /// Renders as a marker/superscript: never takes a whole-element anchor.
    /// (FootnoteNumber, Citation, AnnotationReference, and the unclassified
    /// ToCome / NotSure placeholders.)
    MarkerOnly,
}

/// Reference type classification derived from its content.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceType {
    /// `[TK]` or `[TK-identifier]`
    ToCome { identifier: Option<String> },
    /// `[@citation]` with structured citation data.
    Citation(CitationData),
    /// `[::note]` — pointer to an annotation by label.
    AnnotationReference { label: String },
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

impl ReferenceType {
    /// Classify this reference type's anchoring capability, per §2.3.4.
    ///
    /// Link-like types (`Url`, `File`, `Session`, `General`) render as a span
    /// of linked text and are therefore [`AnchorKind::WholeLineCapable`].
    /// Marker-style types (`FootnoteNumber`, `Citation`, `AnnotationReference`)
    /// render as markers/superscripts and are [`AnchorKind::MarkerOnly`]. The
    /// unclassified placeholders (`ToCome`, `NotSure`) are treated as
    /// `MarkerOnly`: a `[TK]` placeholder has no destination to link and an
    /// unclassifiable reference must not silently consume a whole element.
    pub fn anchoring(&self) -> AnchorKind {
        match self {
            ReferenceType::Url { .. }
            | ReferenceType::File { .. }
            | ReferenceType::Session { .. }
            | ReferenceType::General { .. } => AnchorKind::WholeLineCapable,
            ReferenceType::FootnoteNumber { .. }
            | ReferenceType::Citation(_)
            | ReferenceType::AnnotationReference { .. }
            | ReferenceType::ToCome { .. }
            | ReferenceType::NotSure => AnchorKind::MarkerOnly,
        }
    }
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
