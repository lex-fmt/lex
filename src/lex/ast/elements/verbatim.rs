//! Verbatim block element
//!
//!     A verbatim block embeds content that is not lex formatted. This can be any binary
//!     encoded data, such as images or videos or text in another formal language, most
//!     commonly programming language's code. Since the whole point of the element is to say:
//!     hands off, do not parse this, just preserve it, you'd think that it would be a simple
//!     element, but in reality this is by far the most complex element in Lex, and it warrants
//!     some explanation.
//!
//!     Note that a verbatim block can forgo content all together (i.e. binaries won't encode
//!     content).
//!
//! Structure
//!
//!     - subject: The lead item identifying what the verbatim block contains
//!     - children: VerbatimLine nodes containing the actual content (can be empty)
//!     - closing_data: The closing marker (format: `:: label params? ::`)
//!
//!     The subject introduces what the content is, and the closing data terminates the block.
//!     The data node carries the label/parameters describing the payload. As a convention
//!     though, if the content is to be interpreted by a tool, the label should be the name
//!     of the tool/language. While the lex software will not parse the content, it will
//!     preserve it exactly as it is, and can be used to format the content in editors and
//!     other tools.
//!
//! Syntax
//!
//!     <subject-line>
//!     <indent> <content> ... any number of content elements
//!     <dedent>  <data>
//!
//! Parsing Structure:
//!
//! | Element  | Prec. Blank | Head        | Blank    | Content  | Tail            |
//! |----------|-------------|-------------|----------|----------|-----------------|
//! | Verbatim | Optional    | SubjectLine | Optional | Optional | dedent+DataLine |
//!
//! Parsing Verbatim Blocks
//!
//!     The first point is that, since it can hold non Lex content, its content can't be
//!     parsed. It can be lexed without prejudice, but not parsed. Not only would it be
//!     gibberish, but worse, in case it would trigger indent and dedent events, it would
//!     throw off the parsing and break the document.
//!
//!     This has two consequences: that verbatim parsing must come first, lest its content
//!     create havoc on the structure and also that identifying its end marker has to be very
//!     easy. That's the reason why it ends in a data node, which is the only form that is
//!     not common on regular text.
//!
//!     The verbatim parsing is the only stateful parsing in the pipeline. It matches a
//!     subject line, then either an indented container (in-flow) or flat lines
//!     (full-width/groups), and requires the closing annotation at the same indentation as
//!     the subject.
//!
//!     Verbatim blocks are tried first in the grammar pattern matching order, before any
//!     other elements. This ensures that their non-lex content doesn't interfere with the
//!     parsing of the rest of the document.
//!
//! Content and the Indentation Wall
//!
//!     Verbatim content can be pretty much anything, and that includes any space characters,
//!     which we must not interpret as indentation, nor discard, as it's content. The way to
//!     think about this is through the indentation wall.
//!
//! In-Flow Mode
//!
//!     In this mode, called In-flow Mode, the verbatim content is indented just like any
//!     other children content in Lex, +1 from their parent.
//!
//!     Verbatim content starts at the wall (the subject's indentation + 1 level), until the
//!     end of line. Whitespace characters should be preserved as content. Content cannot,
//!     however, start before the wall, lest we had no way to determine the end of the block.
//!
//!     This logic allows for a neat trick: that verbatim blocks do not need to quote any
//!     content. Even if a line looks like a data node, the fact that it's not in the same
//!     level as the subject means it's not the block's end marker.
//!
//!     Example:
//!         I'm A verbatim Block Subject:
//!             |<- this is the indentation wall, that is the subject's + 1 level up
//!             I'm the first content line
//!             But content can be indented however I please
//!     error ->| as long as it's past the wall
//!             :: text ::
//!
//! Full-Width Mode
//!
//!     At times, verbatim content is very wide, as in tables. In these cases, the various
//!     indentation levels in the Lex document can consume valuable space which would throw
//!     off the content making it either hard to read or truncated by some tools.
//!
//!     For these cases, the full-width mode allows the content to take (almost) all columns.
//!     In this mode, the wall is at user-facing column 2 (zero-based column 1), so content
//!     can hug the left margin without looking like a closing annotation.
//!
//!     Example:
//!   Here is the content.
//!   |<- this is the wall
//!
//!             :: lex ::
//!
//!     The block's mode is determined by the position of the first non-whitespace character
//!     of the first content line. If it's at user-facing column 2, it's a full-width mode
//!     block; otherwise it's in-flow.
//!
//!     The reason for column 2: column 1 would be indistinguishable from the subject's
//!     indentation, while a full indent would lose horizontal space. Column 2 preserves
//!     visual separation without looking like an error.
//!
//! Verbatim Groups
//!
//!     Verbatim blocks support multiple subject/content pairs sharing a single closing
//!     annotation. Use the `group()` iterator to access all pairs. See the spec for syntax
//!     and examples.
//!
//!     This special casing rule allows multiple subject + content groups with only 1 closing
//!     annotation marker.
//!
//! Learn More:
//!
//!     - Verbatim blocks spec: specs/v1/elements/verbatim.lex
//!

use super::super::range::{Position, Range};
use super::super::text_content::TextContent;
use super::super::traits::{AstNode, Container, Visitor, VisualStructure};
use super::annotation::Annotation;
use super::container::VerbatimContainer;
use super::content_item::ContentItem;
use super::data::Data;
use super::typed_content::VerbatimContent;
use std::fmt;
use std::slice;

/// Represents the mode of a verbatim block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbatimBlockMode {
    /// The block's content is indented relative to the subject line.
    Inflow,
    /// The block's content starts at a fixed, absolute column.
    Fullwidth,
}

/// A verbatim block represents content from another format/system.
#[derive(Debug, Clone, PartialEq)]
pub struct Verbatim {
    /// Subject line of the first group (backwards-compatible direct access)
    pub subject: TextContent,
    /// Content lines of the first group (backwards-compatible direct access)
    pub children: VerbatimContainer,
    /// Closing data shared by all groups
    pub closing_data: Data,
    /// Annotations attached to this verbatim block
    pub annotations: Vec<Annotation>,
    /// Location spanning all groups and the closing data
    pub location: Range,
    /// The rendering mode of the verbatim block.
    pub mode: VerbatimBlockMode,
    /// Additional subject/content pairs beyond the first (for multi-group verbatims)
    additional_groups: Vec<VerbatimGroupItem>,
}

impl Verbatim {
    fn default_location() -> Range {
        Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
    }

    pub fn new(
        subject: TextContent,
        children: Vec<VerbatimContent>,
        closing_data: Data,
        mode: VerbatimBlockMode,
    ) -> Self {
        Self {
            subject,
            children: VerbatimContainer::from_typed(children),
            closing_data,
            annotations: Vec::new(),
            location: Self::default_location(),
            mode,
            additional_groups: Vec::new(),
        }
    }

    pub fn with_subject(subject: String, closing_data: Data) -> Self {
        Self {
            subject: TextContent::from_string(subject, None),
            children: VerbatimContainer::empty(),
            closing_data,
            annotations: Vec::new(),
            location: Self::default_location(),
            mode: VerbatimBlockMode::Inflow,
            additional_groups: Vec::new(),
        }
    }

    pub fn marker(subject: String, closing_data: Data) -> Self {
        Self {
            subject: TextContent::from_string(subject, None),
            children: VerbatimContainer::empty(),
            closing_data,
            annotations: Vec::new(),
            location: Self::default_location(),
            mode: VerbatimBlockMode::Inflow,
            additional_groups: Vec::new(),
        }
    }

    /// Preferred builder
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// Attach additional verbatim group entries beyond the first pair.
    pub fn with_additional_groups(mut self, groups: Vec<VerbatimGroupItem>) -> Self {
        self.additional_groups = groups;
        self
    }

    /// Mutable access to the additional verbatim groups beyond the first.
    pub fn additional_groups_mut(&mut self) -> std::slice::IterMut<'_, VerbatimGroupItem> {
        self.additional_groups.iter_mut()
    }

    /// Annotations attached to this verbatim block.
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Mutable access to verbatim annotations.
    pub fn annotations_mut(&mut self) -> &mut Vec<Annotation> {
        &mut self.annotations
    }

    /// Iterate annotation blocks attached to this verbatim block.
    pub fn iter_annotations(&self) -> std::slice::Iter<'_, Annotation> {
        self.annotations.iter()
    }

    /// Iterate all content items nested inside verbatim annotations.
    pub fn iter_annotation_contents(&self) -> impl Iterator<Item = &ContentItem> {
        self.annotations
            .iter()
            .flat_map(|annotation| annotation.children())
    }

    /// Returns an iterator over each subject/content pair in the group order.
    pub fn group(&self) -> VerbatimGroupIter<'_> {
        VerbatimGroupIter {
            first_yielded: false,
            verbatim: self,
            rest: self.additional_groups.iter(),
        }
    }

    /// Returns the number of subject/content pairs held by this verbatim block.
    pub fn group_len(&self) -> usize {
        1 + self.additional_groups.len()
    }
}

impl AstNode for Verbatim {
    fn node_type(&self) -> &'static str {
        "VerbatimBlock"
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
        visitor.visit_verbatim_block(self);
        // Visit all groups, not just the first
        for group in self.group() {
            visitor.visit_verbatim_group(&group);
            super::super::traits::visit_children(visitor, group.children);
            visitor.leave_verbatim_group(&group);
        }
        visitor.leave_verbatim_block(self);
    }
}

impl VisualStructure for Verbatim {
    fn is_source_line_node(&self) -> bool {
        true
    }

    fn has_visual_header(&self) -> bool {
        true
    }
}

impl Container for Verbatim {
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

impl fmt::Display for Verbatim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let group_count = self.group_len();
        let group_word = if group_count == 1 { "group" } else { "groups" };
        write!(
            f,
            "VerbatimBlock('{}', {} {}, closing: {})",
            self.subject.as_string(),
            group_count,
            group_word,
            self.closing_data.label.value
        )
    }
}

/// Stored representation of additional verbatim group entries
#[derive(Debug, Clone, PartialEq)]
pub struct VerbatimGroupItem {
    pub subject: TextContent,
    pub children: VerbatimContainer,
}

impl VerbatimGroupItem {
    pub fn new(subject: TextContent, children: Vec<VerbatimContent>) -> Self {
        Self {
            subject,
            children: VerbatimContainer::from_typed(children),
        }
    }
}

/// Immutable view over a verbatim group entry.
#[derive(Debug, Clone)]
pub struct VerbatimGroupItemRef<'a> {
    pub subject: &'a TextContent,
    pub children: &'a VerbatimContainer,
}

/// Iterator over all subject/content pairs inside a verbatim block.
pub struct VerbatimGroupIter<'a> {
    first_yielded: bool,
    verbatim: &'a Verbatim,
    rest: slice::Iter<'a, VerbatimGroupItem>,
}

impl<'a> Iterator for VerbatimGroupIter<'a> {
    type Item = VerbatimGroupItemRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.first_yielded {
            self.first_yielded = true;
            return Some(VerbatimGroupItemRef {
                subject: &self.verbatim.subject,
                children: &self.verbatim.children,
            });
        }

        self.rest.next().map(|item| VerbatimGroupItemRef {
            subject: &item.subject,
            children: &item.children,
        })
    }
}
