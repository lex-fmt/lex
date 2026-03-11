//! Definition element
//!
//!  Definitions are a core element for explaining terms and concepts.
//!  They pair a subject (the term being defined) with its content, the definition body.
//!
//! Syntax:
//!     <text-span>+ <colon> <line-break>
//!     <indent> <content> ... any number of content elements
//!     <dedent>
//!
//! Parsing Structure:
//!
//! | Element    | Prec. Blank | Head        | Blank | Content | Tail   |
//! |------------|-------------|-------------|-------|---------|--------|
//! | Definition | Optional    | SubjectLine | No    | Yes     | dedent |
//!
//! Examples:
//!     Cache:
//!         Temporary storage for frequently accessed data.
//!
//!     Microservice:
//!         An architectural style that structures applications as loosely coupled services.
//!
//!         Each service is independently deployable and scalable.
//!
//! Learn More:
//! - The definition spec: specs/v1/elements/definition.lex
//! - The definition sample: specs/v1/samples/element-based/definitions/definitions.simple.lex

use super::super::range::{Position, Range};
use super::super::text_content::TextContent;
use super::super::traits::{AstNode, Container, Visitor, VisualStructure};
use super::annotation::Annotation;
use super::container::GeneralContainer;
use super::content_item::ContentItem;
use super::typed_content::ContentElement;
use std::fmt;

/// A definition provides a subject and associated content
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub subject: TextContent,
    pub children: GeneralContainer,
    pub annotations: Vec<Annotation>,
    pub location: Range,
}

impl Definition {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }
    pub fn new(subject: TextContent, children: Vec<ContentElement>) -> Self {
        Self {
            subject,
            children: GeneralContainer::from_typed(children),
            annotations: Vec::new(),
            location: Self::default_location(),
        }
    }
    pub fn with_subject(subject: String) -> Self {
        Self {
            subject: TextContent::from_string(subject, None),
            children: GeneralContainer::empty(),
            annotations: Vec::new(),
            location: Self::default_location(),
        }
    }
    /// Preferred builder
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// Annotations attached to this definition.
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Mutable access to definition annotations.
    pub fn annotations_mut(&mut self) -> &mut Vec<Annotation> {
        &mut self.annotations
    }

    /// Iterate over annotation blocks attached to this definition.
    pub fn iter_annotations(&self) -> std::slice::Iter<'_, Annotation> {
        self.annotations.iter()
    }

    /// Iterate over all content items nested inside attached annotations.
    pub fn iter_annotation_contents(&self) -> impl Iterator<Item = &ContentItem> {
        self.annotations
            .iter()
            .flat_map(|annotation| annotation.children())
    }

    /// Range covering only the subject line.
    pub fn header_location(&self) -> Option<&Range> {
        self.subject.location.as_ref()
    }

    /// Bounding range covering only the definition's children.
    pub fn body_location(&self) -> Option<Range> {
        Range::bounding_box(self.children.iter().map(|item| item.range()))
    }
}

impl AstNode for Definition {
    fn node_type(&self) -> &'static str {
        "Definition"
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
        visitor.visit_definition(self);
        super::super::traits::visit_children(visitor, &self.children);
        visitor.leave_definition(self);
    }
}

impl VisualStructure for Definition {
    fn is_source_line_node(&self) -> bool {
        true
    }

    fn has_visual_header(&self) -> bool {
        true
    }
}

impl Container for Definition {
    fn label(&self) -> &str {
        self.subject.as_string()
    }
    fn children(&self) -> &[ContentItem] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<ContentItem> {
        self.children.as_mut_vec()
    }
}

impl fmt::Display for Definition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Definition('{}', {} items)",
            self.subject.as_string(),
            self.children.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::elements::paragraph::Paragraph;

    #[test]
    fn test_definition() {
        let location = super::super::super::range::Range::new(
            0..0,
            super::super::super::range::Position::new(1, 0),
            super::super::super::range::Position::new(1, 10),
        );
        let definition = Definition::with_subject("Subject".to_string()).at(location.clone());
        assert_eq!(definition.location, location);
    }

    #[test]
    fn test_definition_header_and_body_locations() {
        let subject_range = Range::new(0..7, Position::new(0, 0), Position::new(0, 7));
        let child_range = Range::new(10..15, Position::new(1, 0), Position::new(1, 5));
        let subject = TextContent::from_string("Subject".to_string(), Some(subject_range.clone()));
        let child = ContentElement::Paragraph(
            Paragraph::from_line("Body".to_string()).at(child_range.clone()),
        );

        let definition = Definition::new(subject, vec![child]).at(Range::new(
            0..20,
            Position::new(0, 0),
            Position::new(1, 5),
        ));

        assert_eq!(definition.header_location(), Some(&subject_range));
        assert_eq!(definition.body_location().unwrap().span, child_range.span);
    }
}
