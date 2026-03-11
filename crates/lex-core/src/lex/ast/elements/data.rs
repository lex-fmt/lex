//! Data Node
//!
//!     Data nodes encapsulate the reusable :: label parameters? header shared by
//!     annotations and future elements. They carry the label plus optional parameters
//!     but no closing :: marker or content.
//!
//!     In keeping with Lex's ethos of putting content first there is only one formal
//!     syntax element: the lex-marker, a double colon (::). Accordingly, it's only used
//!     in metadata, there is in Data nodes. Data nodes group a label (an identifier) and
//!     optional parameters.
//!
//! Syntax
//!
//!     <data> = <lex-marker> <whitespace> <label> (<whitespace> <parameters>)?
//!
//!     Examples:
//!         :: note
//!         :: note severity=high
//!         :: syntax
//!
//!     Data nodes always appear at the start of a line (after whitespace), so they are
//!     very easy to identify.
//!
//!     The lex-marker (::) is the only formal syntax element introduced by Lex. All other
//!     markers are naturally occurring in ordinary text, and with the meaning they already
//!     convey.
//!
//!     See [Label](super::label::Label) and [Parameter](super::parameter::Parameter) for
//!     the component elements that make up data nodes.

use super::super::range::{Position, Range};
use super::label::Label;
use super::parameter::Parameter;
use std::fmt;

/// Structured data payload extracted from `:: label params?` headers.
#[derive(Debug, Clone, PartialEq)]
pub struct Data {
    pub label: Label,
    pub parameters: Vec<Parameter>,
    pub location: Range,
}

impl Data {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }

    pub fn new(label: Label, parameters: Vec<Parameter>) -> Self {
        Self {
            label,
            parameters,
            location: Self::default_location(),
        }
    }

    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }
}

impl fmt::Display for Data {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Data('{}', {} params)",
            self.label.value,
            self.parameters.len()
        )
    }
}
