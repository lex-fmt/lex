//! Label element
//!
//!     A label is a short identifier used by annotations and other elements. Labels are
//!     components that carry a bit of information inside an element, only used in metadata.
//!
//!     Labels serve similar roles but have relevant differences from:
//!         - Tags: An annotation can only have one label, while tags are typically multiple.
//!         - IDs: labels are not unique, even in the same element
//!
//!     Labels support dot notation for namespaces:
//!         Namespaced: lex.internal, plugin.myapp.custom
//!         Namespaces are user defined, with the exception of the doc and lex namespaces
//!         which are reserved.
//!
//! Syntax
//!
//!     <letter> (<letter> | <digit> | "_" | "-" | ".")*
//!
//!     Labels are used in data nodes, which have the syntax:
//!         :: label params?
//!
//!     See [Data](super::data::Data) for how labels are used in data nodes.
//!
//!     Learn More:
//!         - Labels spec: specs/v1/elements/label.lex

use super::super::range::{Position, Range};
use std::fmt;

/// A label represents a named identifier in lex documents
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Label {
    pub value: String,
    pub location: Range,
}

impl Label {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }
    pub fn new(value: String) -> Self {
        Self {
            value,
            location: Self::default_location(),
        }
    }
    pub fn from_string(value: &str) -> Self {
        Self {
            value: value.to_string(),
            location: Self::default_location(),
        }
    }

    /// Preferred builder: `at(location)`
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label() {
        let location = super::super::super::range::Range::new(
            0..0,
            super::super::super::range::Position::new(1, 0),
            super::super::super::range::Position::new(1, 10),
        );
        let label = Label::new("test".to_string()).at(location.clone());
        assert_eq!(label.location, location);
    }
}
