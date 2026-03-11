//! Parameter element
//!
//!     A parameter is a pair of key and value. Parameters are components that carry a bit
//!     of information inside an element, only used in metadata. They can be used in
//!     annotations and verbatim blocks to convey structured metadata.
//!
//!     Together with labels, parameters allow for structured metadata. They are used in
//!     data nodes, which have the syntax:
//!         :: label params?
//!
//! Syntax
//!
//!     <key> "=" <value>
//!
//!     Examples:
//!         priority=high
//!         severity=high
//!
//!     Parameters are optional in data nodes. Multiple parameters can be specified
//!     separated by whitespace.
//!
//!     See [Data](super::data::Data) for how parameters are used in data nodes.
//!
//!     Learn More:
//!         - Parameters spec: specs/v1/elements/parameter.lex

use super::super::range::{Position, Range};
use crate::lex::escape::unescape_quoted;
use std::fmt;

/// A parameter represents a key-value pair
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub key: String,
    pub value: String,
    pub location: Range,
}

impl Parameter {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }

    pub fn new(key: String, value: String) -> Self {
        Self {
            key,
            value,
            location: Self::default_location(),
        }
    }

    /// Preferred builder
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// Returns the semantic value with outer quotes stripped and escapes resolved.
    ///
    /// For quoted values like `"Hello World"`, returns `Hello World`.
    /// For values with escapes like `"say \"hello\""`, returns `say "hello"`.
    /// For unquoted values like `simple`, returns `simple` unchanged.
    pub fn unquoted_value(&self) -> String {
        unescape_quoted(&self.value)
    }
}

impl fmt::Display for Parameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.key, self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter() {
        let location = super::super::super::range::Range::new(
            0..0,
            super::super::super::range::Position::new(1, 0),
            super::super::super::range::Position::new(1, 10),
        );
        let param = Parameter::new("key".to_string(), "value".to_string()).at(location.clone());
        assert_eq!(param.location, location);
    }
}
