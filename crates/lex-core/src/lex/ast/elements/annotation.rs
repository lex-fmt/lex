//! Annotation
//!
//! Annotations are a core element in lex, but not the document's content , rather it's metadata one.
//! They provide not only a way for authors and collaborators to register non content related
//! information, but the right hooks for tooling to build on top of lex (e.g., comments, review
//! metadata, publishing hints).
//!
//! As such they provide : -
//! - labels: a way to identify the annotation
//! - parameters (optional): a way to provide structured data
//! - Optional content, like all other elements:
//!     - Nestable containter that can host any element but sessions
//!     - Shorthand for for single or no content annotations.
//!
//!
//! Syntax:
//!   Short Hand Form:
//!     <lex-marker> <label> <parameters>? <lex-marker>
//!   Long Hand Form:
//!     <lex-marker> <label> <parameters>? <lex-marker>
//!     <indent> <content> ... any number of content elements
//!     <dedent> <lex-marker>
//!
//! Parsing Structure:
//!
//! | Element    | Prec. Blank | Head                | Blank | Content | Tail          |
//! |------------|-------------|---------------------|-------|---------|---------------|
//! | Annotation | Optional    | DataMarkerLine | Yes   | Yes     | dedent        |
//!
//! Special Case: Short form annotations are one-liners without content or dedent.
//!
//!  Examples:
//!      Label only:
//!         :: image ::  
//!      Label and parameters:
//!         :: note severity=high :: Check this carefully
//!      Marker form (no content):
//!         :: debug ::
//!      Parameters augmenting the label:
//!         :: meta type=python :: (parameters need an accompanying label)
//!      Long Form:
//!         :: label ::
//!             John has reviewed this paragraph. Hence we're only lacking:
//!             - Janest's approval
//!             - OK from legal
//! Learn More:
//! - The annotation spec: specs/v1/elements/annotation.lex
//! - The annotation sample: specs/v1/samples/element-based/annotations/annotations.simple.lex
//! - Labels: specs/v1/elements/label.lex
//! - Parameters: specs/v1/elements/parameter.lex

use super::super::range::{Position, Range};
use super::super::traits::{AstNode, Container, Visitor, VisualStructure};
use super::container::GeneralContainer;
use super::content_item::ContentItem;
use super::data::Data;
use super::label::Label;
use super::parameter::Parameter;
use super::typed_content::ContentElement;
use std::fmt;

/// An annotation represents some metadata about an AST element.
///
/// # Reserved label namespace
///
/// Labels starting with `lex.` (the `lex.*` namespace, [`Annotation::RESERVED_NAMESPACE_PREFIX`])
/// are reserved for core-defined semantics. Third-party tooling must not author labels in
/// this namespace; the core may add new `lex.*` labels without a coordinating versioning
/// concern. Non-reserved labels remain freely available for extensions
/// (`mycompany.include`, `docs.embed`, etc.).
///
/// The current set of reserved labels:
/// - [`Annotation::INCLUDE_LABEL`] (`"lex.include"`) — see `comms/specs/proposals/includes.lex`.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub data: Data,
    pub children: GeneralContainer,
    pub location: Range,
}

impl Annotation {
    /// Reserved label prefix for core-defined annotation semantics.
    ///
    /// Any annotation whose label starts with this prefix is owned by the Lex
    /// core and may carry behavior in the resolver / analysis layers. External
    /// authors should pick a different namespace.
    pub const RESERVED_NAMESPACE_PREFIX: &'static str = "lex.";

    /// Reserved label for the include directive.
    ///
    /// An annotation with this label and a `src=` parameter is interpreted by
    /// `lex_core::includes` as a request to splice another Lex file's content
    /// into the parent container at the annotation's position. See
    /// `comms/specs/proposals/includes.lex` for the full design.
    pub const INCLUDE_LABEL: &'static str = "lex.include";

    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }
    pub fn new(label: Label, parameters: Vec<Parameter>, children: Vec<ContentElement>) -> Self {
        let data = Data::new(label, parameters);
        Self::from_data(data, children)
    }
    pub fn marker(label: Label) -> Self {
        Self::from_data(Data::new(label, Vec::new()), Vec::new())
    }
    pub fn with_parameters(label: Label, parameters: Vec<Parameter>) -> Self {
        Self::from_data(Data::new(label, parameters), Vec::new())
    }
    pub fn from_data(data: Data, children: Vec<ContentElement>) -> Self {
        Self {
            data,
            children: GeneralContainer::from_typed(children),
            location: Self::default_location(),
        }
    }

    /// Preferred builder
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// Range covering only the annotation header (label + parameters).
    pub fn header_location(&self) -> &Range {
        &self.data.location
    }

    /// Bounding range covering only the annotation's children.
    pub fn body_location(&self) -> Option<Range> {
        Range::bounding_box(self.children.iter().map(|item| item.range()))
    }

    /// Whether this annotation's label is in the reserved `lex.*` namespace.
    pub fn is_reserved(&self) -> bool {
        self.data
            .label
            .value
            .starts_with(Self::RESERVED_NAMESPACE_PREFIX)
    }

    /// Whether this annotation is the include directive (label `lex.include`).
    ///
    /// Hides the string-match on the reserved label so callers don't sprinkle
    /// `annotation.label == "lex.include"` throughout the codebase. Also serves
    /// as the migration boundary if a future version models includes as a
    /// distinct AST node type.
    pub fn is_include(&self) -> bool {
        self.data.label.value == Self::INCLUDE_LABEL
    }

    /// The `src=` parameter value, if present.
    ///
    /// Useful on its own for any annotation that uses a `src` parameter
    /// (verbatim-via-annotation, future `lex.*` directives, etc.). For
    /// the include-specific case, callers typically pair this with
    /// [`Annotation::is_include`].
    pub fn include_src(&self) -> Option<&str> {
        self.data
            .parameters
            .iter()
            .find(|p| p.key == "src")
            .map(|p| p.value.as_str())
    }
}

impl AstNode for Annotation {
    fn node_type(&self) -> &'static str {
        "Annotation"
    }
    fn display_label(&self) -> String {
        if self.data.parameters.is_empty() {
            self.data.label.value.clone()
        } else {
            format!(
                "{} ({} params)",
                self.data.label.value,
                self.data.parameters.len()
            )
        }
    }
    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        visitor.visit_annotation(self);
        super::super::traits::visit_children(visitor, &self.children);
        visitor.leave_annotation(self);
    }
}

impl VisualStructure for Annotation {
    fn is_source_line_node(&self) -> bool {
        true
    }

    fn has_visual_header(&self) -> bool {
        true
    }
}

impl Container for Annotation {
    fn label(&self) -> &str {
        &self.data.label.value
    }
    fn children(&self) -> &[ContentItem] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<ContentItem> {
        self.children.as_mut_vec()
    }
}

impl fmt::Display for Annotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Annotation('{}', {} params, {} items)",
            self.data.label.value,
            self.data.parameters.len(),
            self.children.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::elements::paragraph::Paragraph;
    use crate::lex::ast::elements::typed_content::ContentElement;

    #[test]
    fn test_annotation_header_and_body_locations() {
        let header_range = Range::new(0..4, Position::new(0, 0), Position::new(0, 4));
        let child_range = Range::new(10..20, Position::new(1, 0), Position::new(2, 0));
        let label = Label::new("note".to_string()).at(header_range.clone());
        let data = Data::new(label, Vec::new()).at(header_range.clone());
        let child = ContentElement::Paragraph(
            Paragraph::from_line("body".to_string()).at(child_range.clone()),
        );

        let annotation = Annotation::from_data(data, vec![child]).at(Range::new(
            0..25,
            Position::new(0, 0),
            Position::new(2, 0),
        ));

        assert_eq!(annotation.header_location().span, header_range.span);
        assert_eq!(annotation.body_location().unwrap().span, child_range.span);
    }

    fn ann(label: &str, params: Vec<(&str, &str)>) -> Annotation {
        let parameters = params
            .into_iter()
            .map(|(k, v)| Parameter::new(k.to_string(), v.to_string()))
            .collect();
        Annotation::with_parameters(Label::new(label.to_string()), parameters)
    }

    #[test]
    fn test_is_reserved() {
        assert!(ann("lex.include", vec![]).is_reserved());
        assert!(ann("lex.foo.bar", vec![]).is_reserved());
        // Boundary: a label that starts with "lex" but not "lex." is NOT reserved.
        assert!(!ann("lexicon", vec![]).is_reserved());
        assert!(!ann("review", vec![]).is_reserved());
        assert!(!ann("mycompany.include", vec![]).is_reserved());
    }

    #[test]
    fn test_is_include() {
        assert!(ann("lex.include", vec![("src", "x.lex")]).is_include());
        // Other lex.* labels are reserved but not includes.
        assert!(!ann("lex.something_else", vec![]).is_include());
        // Same trailing label without the lex. prefix is not an include.
        assert!(!ann("include", vec![("src", "x.lex")]).is_include());
    }

    #[test]
    fn test_include_src() {
        let with_src = ann("lex.include", vec![("src", "chapters/01.lex")]);
        assert_eq!(with_src.include_src(), Some("chapters/01.lex"));

        // The accessor is independent of the label — works for any annotation
        // that happens to carry a `src` parameter (verbatim-via-annotation, etc.)
        let other_with_src = ann("image", vec![("src", "diagram.png")]);
        assert_eq!(other_with_src.include_src(), Some("diagram.png"));

        // No src parameter → None.
        assert_eq!(ann("lex.include", vec![]).include_src(), None);
        assert_eq!(
            ann("lex.include", vec![("title", "Chapter 1")]).include_src(),
            None
        );
    }

    #[test]
    fn test_constants_match_documented_values() {
        assert_eq!(Annotation::RESERVED_NAMESPACE_PREFIX, "lex.");
        assert_eq!(Annotation::INCLUDE_LABEL, "lex.include");
        // Sanity: the include label is itself reserved.
        assert!(Annotation::INCLUDE_LABEL.starts_with(Annotation::RESERVED_NAMESPACE_PREFIX));
    }
}
