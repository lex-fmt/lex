//! Target-format identifier used by the render hook.
//!
//! Built-in formats (`html`, `latex`, `markdown`, `pdf`) are exhaustively
//! enumerated for ergonomic match arms. Namespace-defined formats slip
//! through as [`Format::Custom`].

use std::convert::Infallible;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A render-target format. Wire form is the lowercase string name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Format {
    Html,
    Latex,
    Markdown,
    Pdf,
    /// Any namespace-defined format string (e.g., `"plasma-spec-v4"`,
    /// `"docx"`). Compared case-sensitively.
    Custom(String),
}

impl Format {
    /// Wire string for this format.
    pub fn as_str(&self) -> &str {
        match self {
            Format::Html => "html",
            Format::Latex => "latex",
            Format::Markdown => "markdown",
            Format::Pdf => "pdf",
            Format::Custom(s) => s.as_str(),
        }
    }
}

impl FromStr for Format {
    type Err = Infallible;

    /// Unknown strings become [`Format::Custom`]; parsing is infallible.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "html" => Format::Html,
            "latex" => Format::Latex,
            "markdown" => Format::Markdown,
            "pdf" => Format::Pdf,
            other => Format::Custom(other.to_string()),
        })
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Format {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Format {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(s.parse().expect("Format::from_str is infallible"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_round_trips() {
        let f = Format::Html;
        let s = serde_json::to_string(&f).unwrap();
        assert_eq!(s, r#""html""#);
        let back: Format = serde_json::from_str(&s).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn unknown_becomes_custom() {
        let s = r#""plasma-spec-v4""#;
        let f: Format = serde_json::from_str(s).unwrap();
        assert_eq!(f, Format::Custom("plasma-spec-v4".to_string()));
        assert_eq!(serde_json::to_string(&f).unwrap(), s);
    }

    #[test]
    fn from_str_via_trait() {
        let f: Format = "html".parse().unwrap();
        assert_eq!(f, Format::Html);
        let f: Format = "docx".parse().unwrap();
        assert_eq!(f, Format::Custom("docx".to_string()));
    }

    #[test]
    fn display_uses_wire_string() {
        assert_eq!(Format::Html.to_string(), "html");
        assert_eq!(Format::Custom("docx".to_string()).to_string(), "docx");
    }
}
