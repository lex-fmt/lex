//! Core data structures for the Intermediate Representation (IR).

pub use lex_core::lex::ast::elements::inlines::{
    CitationData, CitationLocator, PageFormat, PageRange, ReferenceType,
};
pub use lex_core::lex::ast::elements::label::LabelForm;

/// A universal, semantic representation of a document node.
#[derive(Debug, Clone, PartialEq)]
pub enum DocNode {
    Document(Document),
    Heading(Heading),
    Paragraph(Paragraph),
    List(List),
    ListItem(ListItem),
    Definition(Definition),
    Verbatim(Verbatim),
    Annotation(Annotation),
    Inline(InlineContent),
    Table(Table),
    Image(Image),
    Video(Video),
    Audio(Audio),
}

/// Represents the root of a document.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub title: Option<Vec<InlineContent>>,
    pub subtitle: Option<Vec<InlineContent>>,
    pub children: Vec<DocNode>,
    /// Document-scope annotations (i.e. annotations attached directly
    /// to the document, not nested inside any block).
    ///
    /// Phase 3a of #570 added this slot as a first-class home for them.
    /// Phase 3b (#614) flipped the source-of-truth atomically:
    ///
    /// - `from_lex_document` populates the slot from lex-core's
    ///   `doc.annotations`.
    /// - `to_lex_document` emits each entry back into
    ///   `lex_doc.annotations` via `to_lex_annotation_raw`, so a
    ///   `lex → IR → lex` roundtrip is structurally lossless.
    /// - `tree_to_events` does **not** flatten the slot into the event
    ///   stream as a synthetic `frontmatter` annotation — format-
    ///   specific serializers that need a packed YAML preamble
    ///   (currently just markdown) read this slot directly from the
    ///   IR.
    pub document_annotations: Vec<Annotation>,
}

/// Represents a heading with a specific level.
#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub level: usize,
    pub content: Vec<InlineContent>,
    pub children: Vec<DocNode>,
}

/// Represents a paragraph of text.
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    pub content: Vec<InlineContent>,
}

/// Decoration style for ordered lists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ListStyle {
    /// Unordered: `-`, `*`, `+`
    Bullet,
    /// Numeric: `1.`, `2.`, `3.`
    Numeric,
    /// Lowercase alphabetic: `a.`, `b.`, `c.`
    AlphaLower,
    /// Uppercase alphabetic: `A.`, `B.`, `C.`
    AlphaUpper,
    /// Lowercase roman: `i.`, `ii.`, `iii.`
    RomanLower,
    /// Uppercase roman: `I.`, `II.`, `III.`
    RomanUpper,
}

impl ListStyle {
    pub fn is_ordered(self) -> bool {
        !matches!(self, ListStyle::Bullet)
    }
}

/// Whether list markers use short or extended (hierarchical) form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ListForm {
    /// Short form: single level marker (e.g., `1.`, `a)`)
    Short,
    /// Extended form: multi-level nested index (e.g., `1.2.3`, `I.a.2`)
    Extended,
}

/// Represents a list of items.
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub items: Vec<ListItem>,
    pub ordered: bool,
    pub style: ListStyle,
    pub form: ListForm,
}

/// Represents an item in a list.
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    pub content: Vec<InlineContent>,
    pub children: Vec<DocNode>,
}

/// Represents a definition of a term.
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub term: Vec<InlineContent>,
    pub description: Vec<DocNode>,
}

/// Represents a block of verbatim text.
#[derive(Debug, Clone, PartialEq)]
pub struct Verbatim {
    pub subject: Option<String>,
    /// Destination href when a reference line anchors the verbatim subject
    /// (references-general.lex §2.3.2). The subject renders as a plain-text
    /// caption, so the resolved link travels alongside it: when `Some`, the
    /// HTML / Markdown serializers wrap the caption text in a link to this
    /// target. `None` for an unanchored subject and for IR built from non-lex
    /// sources. Not round-tripped — the reference line is reconstructed from
    /// `Document::reference_lines()`, not from the IR.
    pub subject_href: Option<String>,
    pub language: Option<String>,
    pub content: String,
    /// Closing-data parameters from the source, mirroring lex-core's
    /// `Data.parameters` on a verbatim block's closing marker (e.g.
    /// `caption=foo` in `:: image src=x caption=foo ::`).
    ///
    /// Populated by `from_lex_verbatim` from `closing_data.parameters`
    /// for verbatim blocks that fall back to `DocNode::Verbatim` (no
    /// `on_ir_build` handler hydrated them into a typed variant) and
    /// emitted back by `to_lex_verbatim` so the source round-trips.
    /// `render_dispatch::visit_verbatim` surfaces them to handlers via
    /// `LabelCtx.params` so third-party `verbatim_label: true` schemas
    /// with `on_render` only see the same params they would have under
    /// the pre-#616 AST walk.
    ///
    /// Empty for IR built from non-lex sources (markdown/rfc-xml
    /// parsers) and for verbatim blocks whose closing data carried no
    /// parameters.
    pub parameters: Vec<(String, String)>,
}

/// Represents an annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub label: String,
    pub parameters: Vec<(String, String)>,
    pub content: Vec<DocNode>,
    /// Which input form the user wrote, mirroring `Label::form` from
    /// lex-core. Carried across `from_lex` → IR → `to_lex` so the
    /// `lexd format` roundtrip preserves the source spelling. Set by
    /// `from_lex_annotation` from the AST and by the markdown parser
    /// from `classify_label` so `markdown → lex` conversions emit the
    /// blessed shortcut form (e.g. `:: title ::` rather than
    /// `:: lex.metadata.title ::`). Issue #593.
    pub form: LabelForm,
}

/// Represents a table.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub rows: Vec<TableRow>,
    pub header: Vec<TableRow>,
    pub caption: Option<Vec<InlineContent>>,
    pub footnotes: Vec<DocNode>,
    pub fullwidth: bool,
}

/// Represents a table row.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

/// Represents a table cell.
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    pub content: Vec<DocNode>,
    pub header: bool,
    pub align: TableCellAlignment,
    pub colspan: usize,
    pub rowspan: usize,
}

/// Alignment of a table cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TableCellAlignment {
    Left,
    Center,
    Right,
    None,
}

/// Represents inline content, such as text, bold, italics, etc.
///
/// All variants represent semantic content — structural concerns like list
/// markers or session numbering are expressed through container types (e.g.
/// `List.style`, `List.form`) and are never embedded in inline content.
#[derive(Debug, Clone, PartialEq)]
pub enum InlineContent {
    Text(String),
    Bold(Vec<InlineContent>),
    Italic(Vec<InlineContent>),
    Code(String),
    Math(String),
    /// A bracketed reference carrying both the raw literal (round-trips
    /// back to `[<raw>]` via `to_lex`) and the lex-core classification.
    ///
    /// `kind` is preserved from lex-core's `ReferenceInline` for IR built
    /// via `from_lex`. For IR built from non-lex sources (markdown
    /// links go straight to `Link`, rfc-xml `<xref>` etc.) the kind
    /// defaults to `ReferenceType::NotSure` since those importers don't
    /// classify against lex's reference grammar.
    ///
    /// Format adapters (HTML, markdown) dispatch on `kind`
    /// (Citation / FootnoteNumber / AnnotationReference → marker shapes)
    /// instead of re-parsing the raw string. Issue #614 follow-up.
    ///
    /// References that lex-core resolved an anchor for (an inline word
    /// anchor, or a reference-line whole-element / self-link anchor;
    /// references-general.lex §2.3) never reach this variant — they are
    /// rewritten to `Link` during IR construction by `ir/anchoring.rs`.
    /// Only marker-style references and link-like references with no
    /// adjacent word to anchor survive as `Reference`.
    Reference {
        raw: String,
        kind: ReferenceType,
    },
    /// A resolved link with explicit anchor text and href.
    /// Produced by resolving implicit anchors from Lex references,
    /// or by importing from formats that have explicit link anchors (Markdown, HTML).
    Link {
        text: String,
        href: String,
    },
    Image(Image),
}

/// Represents an image.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub src: String,
    pub alt: String,
    pub title: Option<String>,
}

/// Represents a video.
#[derive(Debug, Clone, PartialEq)]
pub struct Video {
    pub src: String,
    pub title: Option<String>,
    pub poster: Option<String>,
}

/// Represents an audio file.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    pub src: String,
    pub title: Option<String>,
}
