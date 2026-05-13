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

/// How the user spelled a label site, relative to the resolved canonical.
///
/// Forward-looking infrastructure for the label namespace model
/// described in `comms/specs/general.lex` §4. The eventual contract:
/// Lex accepts up to three spellings of any `lex.*` label and one
/// spelling of any community label, and round-trips the user's choice
/// so `lexd format` does not silently rewrite the source.
///
/// **Status in this PR (#584 PR 1/5):** the enum and the `form` field
/// on [`Label`] exist; the parse-time `NormalizeLabels` stage tags
/// labels that match its legacy-rewrite table, defaulting to
/// `Canonical` for every other site. No formatter consults `form` yet
/// — `lexd format` still emits `label.value` verbatim. PR 2 expands
/// `NormalizeLabels` into the full resolution rules (universal
/// prefix-strip, `Community` classification, hard-error for `doc.*`
/// and unrecognized bare); PR 3 wires `form` through the formatter
/// so the roundtrip promise lands end-to-end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LabelForm {
    /// User wrote the canonical form verbatim (`lex.metadata.author`).
    /// This is also the default when a Label is constructed
    /// programmatically without a specific source form (e.g. from the
    /// wire codec, where the wire always carries the canonical).
    Canonical,
    /// User wrote the prefix-stripped form (`metadata.author`).
    /// Resolves to canonical by prepending `lex.`.
    Stripped,
    /// User wrote the one-segment shortcut (`author`). Resolves to
    /// canonical via the normative shortcut table.
    Shortcut,
    /// User wrote a community label (`acme.task`). Carries a single
    /// accepted spelling and round-trips unchanged. Registry validation
    /// is deferred to the analysis stage.
    Community,
}

impl Default for LabelForm {
    fn default() -> Self {
        Self::Canonical
    }
}

/// A label represents a named identifier in lex documents
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Label {
    /// The resolved canonical spelling. For `lex.*` labels this is the
    /// full `lex.X.Y.Z` form. For community labels it is the spelling
    /// the user wrote (community labels carry only one accepted form).
    pub value: String,
    pub location: Range,
    /// Which input form the user wrote. Defaults to
    /// [`LabelForm::Canonical`] when a Label is built programmatically;
    /// the parse-time `NormalizeLabels` stage tags this for the labels
    /// it rewrites. See [`LabelForm`]'s docs for the PR-by-PR status —
    /// in this PR the field is recorded but no formatter consults it.
    pub form: LabelForm,
}

impl Label {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }
    pub fn new(value: String) -> Self {
        Self {
            value,
            location: Self::default_location(),
            form: LabelForm::Canonical,
        }
    }
    pub fn from_string(value: &str) -> Self {
        Self {
            value: value.to_string(),
            location: Self::default_location(),
            form: LabelForm::Canonical,
        }
    }

    /// Preferred builder: `at(location)`
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// Builder: tag the input form the user wrote.
    pub fn with_form(mut self, form: LabelForm) -> Self {
        self.form = form;
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

    #[test]
    fn label_defaults_form_to_canonical() {
        assert_eq!(Label::new("x".into()).form, LabelForm::Canonical);
        assert_eq!(Label::from_string("y").form, LabelForm::Canonical);
    }

    #[test]
    fn with_form_tags_label() {
        let l = Label::from_string("author").with_form(LabelForm::Shortcut);
        assert_eq!(l.form, LabelForm::Shortcut);
        assert_eq!(l.value, "author");
    }

    #[test]
    fn label_form_default_is_canonical() {
        assert_eq!(LabelForm::default(), LabelForm::Canonical);
    }
}
