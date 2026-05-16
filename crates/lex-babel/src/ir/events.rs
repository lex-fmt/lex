//! Defines the flat event stream representation of a document.

use crate::ir::nodes::{InlineContent, LabelForm, ListForm, ListStyle};

/// Represents a single event in the document stream.
///
/// This enum is used to represent a document as a flat sequence of events,
/// which is useful for stream-based processing and conversion between formats.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    StartDocument,
    EndDocument,
    StartHeading(usize),
    EndHeading(usize),
    /// Marks the start of a container's children (mirrors AST container structure)
    StartContent,
    /// Marks the end of a container's children
    EndContent,
    StartParagraph,
    EndParagraph,
    StartList {
        ordered: bool,
        style: ListStyle,
        form: ListForm,
    },
    EndList,
    StartListItem,
    EndListItem,
    StartDefinition,
    EndDefinition,
    StartDefinitionTerm,
    EndDefinitionTerm,
    StartDefinitionDescription,
    EndDefinitionDescription,
    StartVerbatim {
        language: Option<String>,
        subject: Option<String>,
        /// Closing-data parameters preserved end-to-end through the
        /// event stream so `IR → events → IR` round-trips losslessly
        /// (mirrors `StartAnnotation.parameters`).
        parameters: Vec<(String, String)>,
    },
    EndVerbatim,
    StartAnnotation {
        label: String,
        parameters: Vec<(String, String)>,
        /// Source form of the label (canonical, stripped, shortcut, or
        /// community). Round-trips the user's input spelling through
        /// the IR → AST flow so `lexd format` doesn't silently rewrite.
        /// Issue #593.
        form: LabelForm,
    },
    EndAnnotation {
        label: String,
    },
    StartTable {
        caption: Option<Vec<crate::ir::nodes::InlineContent>>,
        fullwidth: bool,
    },
    EndTable,
    StartTableRow {
        header: bool,
    },
    EndTableRow,
    StartTableCell {
        header: bool,
        align: crate::ir::nodes::TableCellAlignment,
        colspan: usize,
        rowspan: usize,
    },
    EndTableCell,
    /// Table-scoped footnotes, emitted after all rows and before EndTable
    StartTableFootnotes,
    EndTableFootnotes,
    Image(crate::ir::nodes::Image),
    Video(crate::ir::nodes::Video),
    Audio(crate::ir::nodes::Audio),
    Inline(InlineContent),
}
