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
//! | Annotation | Optional    | AnnotationStartLine | Yes   | Yes     | AnnotationEnd |
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
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub data: Data,
    pub children: GeneralContainer,
    pub location: Range,
}

impl Annotation {
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
}
