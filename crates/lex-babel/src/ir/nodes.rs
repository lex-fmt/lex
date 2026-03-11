//! Core data structures for the Intermediate Representation (IR).

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
    pub children: Vec<DocNode>,
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
    pub language: Option<String>,
    pub content: String,
}

/// Represents an annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub label: String,
    pub parameters: Vec<(String, String)>,
    pub content: Vec<DocNode>,
}

/// Represents a table.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub rows: Vec<TableRow>,
    pub header: Vec<TableRow>,
    pub caption: Option<Vec<InlineContent>>,
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
#[derive(Debug, Clone, PartialEq)]
pub enum InlineContent {
    Text(String),
    Bold(Vec<InlineContent>),
    Italic(Vec<InlineContent>),
    Code(String),
    Math(String),
    Reference(String),
    /// A resolved link with explicit anchor text and href.
    /// Produced by resolving implicit anchors from Lex references,
    /// or by importing from formats that have explicit link anchors (Markdown, HTML).
    Link {
        text: String,
        href: String,
    },
    Marker(String),
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
