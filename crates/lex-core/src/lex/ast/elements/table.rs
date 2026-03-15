//! Table element
//!
//!     Tables are a native element for structured, tabular data. They share the outer
//!     structure of verbatim blocks (subject line, indented content, closing annotation)
//!     but with inline-parsed pipe-delimited content instead of raw text.
//!
//! Structure
//!
//!     - subject: The table caption (inline-parsed)
//!     - header_rows: Header rows (default: first row)
//!     - body_rows: Body/data rows
//!     - footnotes: Optional scoped footnote list
//!     - closing_data: The closing annotation (:: table params? ::)
//!     - mode: Inflow or Fullwidth (inherited from verbatim wall logic)
//!
//! Syntax
//!
//!     <subject-line>
//!         | cell | cell | cell |
//!         | cell | cell | cell |
//!     :: table params? ::
//!
//! Cell Merging
//!
//!     Merge markers (`>>` for colspan, `^^` for rowspan) are resolved during AST
//!     assembly. The content cell gets its colspan/rowspan incremented, and absorbed
//!     cells are removed. The final AST contains only content cells with span counts.
//!
//! Multi-line Mode
//!
//!     When blank lines separate pipe groups, consecutive pipe lines within a group
//!     form a single row. Auto-detected; no flags needed.
//!
//! Learn More:
//!
//!     - Table element spec: specs/elements/table.lex
//!     - Table proposal: specs/proposals/table.lex

use super::super::range::Range;
use super::super::text_content::TextContent;
use super::super::traits::{AstNode, Container, Visitor, VisualStructure};
use super::annotation::Annotation;
use super::container::GeneralContainer;
use super::content_item::ContentItem;
use super::data::Data;
use super::list::List;
use super::typed_content::ContentElement;
use super::verbatim::VerbatimBlockMode;
use std::fmt;

/// Alignment hint for a table cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableCellAlignment {
    /// Left-aligned (default)
    Left,
    /// Center-aligned
    Center,
    /// Right-aligned
    Right,
    /// No explicit alignment
    None,
}

/// A single cell in a table row.
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    /// The cell's inline content (trimmed text, inline-parsed)
    pub content: TextContent,
    /// Block-level children (lists, definitions, etc.) when cell has block content
    pub children: GeneralContainer,
    /// Number of columns this cell spans (1 = no merge)
    pub colspan: usize,
    /// Number of rows this cell spans (1 = no merge)
    pub rowspan: usize,
    /// Column alignment for this cell
    pub align: TableCellAlignment,
    /// Whether this cell is in a header row
    pub header: bool,
    /// Byte range location
    pub location: Range,
}

impl TableCell {
    pub fn new(content: TextContent) -> Self {
        Self {
            content,
            children: GeneralContainer::empty(),
            colspan: 1,
            rowspan: 1,
            align: TableCellAlignment::None,
            header: false,
            location: Range::default(),
        }
    }

    pub fn with_children(mut self, children: Vec<ContentElement>) -> Self {
        self.children = GeneralContainer::from_typed(children);
        self
    }

    /// Whether this cell has block-level content (lists, definitions, etc.)
    pub fn has_block_content(&self) -> bool {
        !self.children.is_empty()
    }

    pub fn with_span(mut self, colspan: usize, rowspan: usize) -> Self {
        self.colspan = colspan;
        self.rowspan = rowspan;
        self
    }

    pub fn with_align(mut self, align: TableCellAlignment) -> Self {
        self.align = align;
        self
    }

    pub fn with_header(mut self, header: bool) -> Self {
        self.header = header;
        self
    }

    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// The text content of this cell
    pub fn text(&self) -> &str {
        self.content.as_string()
    }

    /// Whether this cell is empty (whitespace-only or no content)
    pub fn is_empty(&self) -> bool {
        self.content.as_string().trim().is_empty()
    }
}

/// A row in a table.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    /// The cells in this row
    pub cells: Vec<TableCell>,
    /// Byte range location
    pub location: Range,
}

impl TableRow {
    pub fn new(cells: Vec<TableCell>) -> Self {
        Self {
            cells,
            location: Range::default(),
        }
    }

    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// Number of cells in this row
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}

/// A table element with structured, pipe-delimited content.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Caption/subject line (inline-parsed)
    pub subject: TextContent,
    /// Header rows (typically first row; controlled by header=N parameter)
    pub header_rows: Vec<TableRow>,
    /// Body/data rows
    pub body_rows: Vec<TableRow>,
    /// Optional scoped footnote definitions
    pub footnotes: Option<Box<List>>,
    /// Closing annotation (:: table params? ::)
    pub closing_data: Data,
    /// Annotations attached to this table
    pub annotations: Vec<Annotation>,
    /// Location spanning the entire table element
    pub location: Range,
    /// Rendering mode (Inflow or Fullwidth, same as verbatim blocks)
    pub mode: VerbatimBlockMode,
}

impl Table {
    pub fn new(
        subject: TextContent,
        header_rows: Vec<TableRow>,
        body_rows: Vec<TableRow>,
        closing_data: Data,
        mode: VerbatimBlockMode,
    ) -> Self {
        Self {
            subject,
            header_rows,
            body_rows,
            footnotes: None,
            closing_data,
            annotations: Vec::new(),
            location: Range::default(),
            mode,
        }
    }

    pub fn with_footnotes(mut self, footnotes: List) -> Self {
        self.footnotes = Some(Box::new(footnotes));
        self
    }

    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// All rows (header + body) in document order
    pub fn all_rows(&self) -> impl Iterator<Item = &TableRow> {
        self.header_rows.iter().chain(self.body_rows.iter())
    }

    /// Total number of rows (header + body)
    pub fn row_count(&self) -> usize {
        self.header_rows.len() + self.body_rows.len()
    }

    /// Maximum column count across all rows
    pub fn column_count(&self) -> usize {
        self.all_rows()
            .map(|row| row.cells.len())
            .max()
            .unwrap_or(0)
    }

    /// Annotations attached to this table.
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Mutable access to table annotations.
    pub fn annotations_mut(&mut self) -> &mut Vec<Annotation> {
        &mut self.annotations
    }
}

impl AstNode for Table {
    fn node_type(&self) -> &'static str {
        "Table"
    }

    fn display_label(&self) -> String {
        let subject_text = self.subject.as_string();
        if subject_text.chars().count() > 50 {
            format!("{}…", subject_text.chars().take(50).collect::<String>())
        } else {
            subject_text.to_string()
        }
    }

    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        visitor.visit_table(self);
        visitor.leave_table(self);
    }
}

impl VisualStructure for Table {
    fn is_source_line_node(&self) -> bool {
        true
    }

    fn has_visual_header(&self) -> bool {
        true
    }
}

impl Container for Table {
    fn label(&self) -> &str {
        self.subject.as_string()
    }

    fn children(&self) -> &[ContentItem] {
        // Tables don't use the generic ContentItem children pattern;
        // their structure is rows/cells. Return empty slice.
        &[]
    }

    fn children_mut(&mut self) -> &mut Vec<ContentItem> {
        // Tables don't use generic children. This is a design tension with
        // the Container trait but is consistent with how they work.
        // For now, panic - callers should use the typed row/cell API.
        panic!("Tables use structured rows/cells, not generic children")
    }
}

impl fmt::Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Table('{}', {} header + {} body rows, {} cols)",
            self.subject.as_string(),
            self.header_rows.len(),
            self.body_rows.len(),
            self.column_count()
        )
    }
}
