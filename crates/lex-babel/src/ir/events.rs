//! Defines the flat event stream representation of a document.

use crate::ir::nodes::{InlineContent, ListForm, ListStyle};

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
    },
    EndVerbatim,
    StartAnnotation {
        label: String,
        parameters: Vec<(String, String)>,
    },
    EndAnnotation {
        label: String,
    },
    StartTable,
    EndTable,
    StartTableRow {
        header: bool,
    },
    EndTableRow,
    StartTableCell {
        header: bool,
        align: crate::ir::nodes::TableCellAlignment,
    },
    EndTableCell,
    Image(crate::ir::nodes::Image),
    Video(crate::ir::nodes::Video),
    Audio(crate::ir::nodes::Audio),
    Inline(InlineContent),
}
