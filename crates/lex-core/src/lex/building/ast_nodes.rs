//! AST Node Creation from Extracted Data
//!
//! This module creates AST nodes from primitive data structures returned by
//! the data_extraction layer. It handles the conversion from byte ranges to
//! AST Range (line/column positions) and constructs the final AST structures.
//!
//! # Architecture
//!
//! ```text
//! Data Structs (primitives) → AST Creation → AST Nodes
//! { text: String,               ↓
//!   byte_range: Range<usize> }  - Convert byte ranges → ast::Range
//!                                - Create TextContent, TextLine, etc.
//!                                - Build complete AST nodes
//!                                ↓
//!                                ContentItem (with ast::Range)
//! ```
//!
//! # Responsibilities
//!
//! - Convert `Range<usize>` (byte offsets) → `ast::Range` (line/column)
//! - Create AST structures (TextContent, TextLine, Paragraph, etc.)
//! - Pure AST construction - no token processing
//! - Aggregate locations from children where needed
//!
//! # Key Design Principle
//!
//! This layer receives primitives and produces AST types. The byte→line/column
//! conversion happens here using `byte_range_to_ast_range()`.

use super::extraction::{
    DataExtraction, DefinitionData, ListItemData, ParagraphData, SessionData, VerbatimBlockData,
    VerbatimGroupData,
};
use super::location::{
    aggregate_locations, byte_range_to_ast_range, compute_location_from_locations, default_location,
};
use crate::lex::ast::elements::blank_line_group::BlankLineGroup;
use crate::lex::ast::elements::typed_content::{
    ContentElement, ListContent, SessionContent, VerbatimContent,
};
use crate::lex::ast::elements::verbatim::VerbatimGroupItem;
use crate::lex::ast::elements::SequenceMarker;
use crate::lex::ast::range::SourceLocation;
use crate::lex::ast::traits::AstNode;
use crate::lex::ast::{
    Annotation, Data, Definition, Label, List, ListItem, Paragraph, Range, Session, TextContent,
    TextLine, Verbatim,
};
use crate::lex::parsing::ContentItem;
use crate::lex::token::Token;
use std::ops::Range as ByteRange;

// ============================================================================
// TYPE SAFETY STATUS
// ============================================================================
//
// This module has been partially refactored for type safety (Steps 1-4 of #228):
//
// ✓ Step 1-4 Complete: Type conversions validate nesting rules
//
// ✓ Step 5 Complete: Element constructors now require typed content
//   - Definition::new(subject, Vec<ContentElement>)
//   - Session::new(title, Vec<SessionContent>)
//   - Annotation::new(label, params, Vec<ContentElement>)
// ✓ Step 6 (optional) Complete: Parser/AST builder now supplies typed content so
//   container policies are enforced before AST construction.
// ✓ Step 7 Complete: Deprecated container shims removed; no runtime conversion
//   paths remain in the builder pipeline.
//
// The builder now relies exclusively on typed content supplied by upstream
// stages. Conversion helpers only exist in tests to assert enum behavior.
//
// ============================================================================

// ============================================================================
// PARAGRAPH CREATION
// ============================================================================

/// Create a Paragraph AST node from extracted paragraph data.
///
/// Converts byte ranges to AST Ranges and builds the Paragraph structure
/// with TextLines.
///
/// # Arguments
///
/// * `data` - Extracted paragraph data with text and byte ranges
/// * `source` - Original source string (needed for byte→line/column conversion)
///
/// # Returns
///
/// A Paragraph ContentItem with proper ast::Range locations
pub(super) fn paragraph_node(data: ParagraphData, source_location: &SourceLocation) -> ContentItem {
    // Convert byte ranges to AST ranges and build TextLines
    let lines: Vec<ContentItem> = data
        .text_lines
        .into_iter()
        .map(|(text, byte_range)| {
            let location = byte_range_to_ast_range(byte_range, source_location);
            let text_content = TextContent::from_string(text, Some(location.clone()));
            let text_line = TextLine::new(text_content).at(location);
            ContentItem::TextLine(text_line)
        })
        .collect();

    // Convert overall byte range to AST range
    let overall_location = byte_range_to_ast_range(data.overall_byte_range, source_location);

    ContentItem::Paragraph(Paragraph {
        lines,
        annotations: Vec::new(),
        location: overall_location,
    })
}

// ============================================================================
// SESSION CREATION
// ============================================================================

/// Create a Session AST node from extracted session data.
///
/// Converts byte range to AST Range, creates TextContent for title,
/// and aggregates location from title and children.
///
/// # Arguments
///
/// * `data` - Extracted session data with title text and byte range
/// * `content` - Child content items
/// * `source` - Original source string
///
/// # Returns
///
/// A Session ContentItem
pub(in crate::lex::building) fn session_node(
    session_data: SessionData,
    content: Vec<SessionContent>,
    source_location: &SourceLocation,
) -> ContentItem {
    let title_location = source_location.byte_range_to_ast_range(&session_data.title_byte_range);
    let title_text = session_data.title_text;

    // Construct SequenceMarker if present in session_data
    let marker = if let Some(marker_data) = session_data.marker {
        let marker_location = source_location.byte_range_to_ast_range(&marker_data.byte_range);

        Some(SequenceMarker::new(
            marker_data.style,
            marker_data.separator,
            marker_data.form,
            TextContent::from_string(marker_data.text, Some(marker_location.clone())),
            marker_location,
        ))
    } else {
        None
    };

    // Validate that session markers are valid (no Plain style)
    if let Some(ref m) = marker {
        debug_assert!(
            m.is_valid_for_session(),
            "Invalid session marker: {m:?}. Sessions don't support Plain (-) markers."
        );
    }

    let child_items: Vec<ContentItem> = content.iter().cloned().map(ContentItem::from).collect();
    let location = aggregate_locations(title_location.clone(), &child_items);

    let title = TextContent::from_string(title_text, Some(title_location));
    let mut session = Session::new(title, content).at(location);
    session.marker = marker;
    ContentItem::Session(session)
}

// ============================================================================
// DEFINITION CREATION
// ============================================================================

/// Create a Definition AST node from extracted definition data.
///
/// Converts byte range to AST Range, creates TextContent for subject,
/// and aggregates location from subject and children.
///
/// # Arguments
///
/// * `data` - Extracted definition data with subject text and byte range
/// * `content` - Child content items
/// * `source` - Original source string
///
/// # Returns
///
/// A Definition ContentItem
pub(super) fn definition_node(
    data: DefinitionData,
    content: Vec<ContentElement>,
    source_location: &SourceLocation,
) -> ContentItem {
    let subject_location = byte_range_to_ast_range(data.subject_byte_range, source_location);
    let subject = TextContent::from_string(data.subject_text, Some(subject_location.clone()));
    let child_items: Vec<ContentItem> = content.iter().cloned().map(ContentItem::from).collect();
    let location = aggregate_locations(subject_location, &child_items);

    let definition = Definition::new(subject, content).at(location);
    ContentItem::Definition(definition)
}

// ============================================================================
// LIST CREATION
// ============================================================================

/// Create a List AST node from list items.
///
/// Aggregates location from all list items.
///
/// # Arguments
///
/// * `items` - Vector of ListItem nodes
///
/// # Returns
///
/// A List ContentItem
pub(super) fn list_node(items: Vec<ListItem>) -> ContentItem {
    let item_locations: Vec<Range> = items.iter().map(|item| item.location.clone()).collect();

    // Extract marker from first item if available
    let marker = items.first().and_then(|first_item| {
        use crate::lex::ast::elements::SequenceMarker;
        let marker_text = first_item.marker.as_string();
        let marker_location = first_item.marker.location.clone();
        SequenceMarker::parse(marker_text, marker_location)
    });

    let typed_items: Vec<ListContent> = items.into_iter().map(ListContent::ListItem).collect();

    let location = if item_locations.is_empty() {
        Range::default()
    } else {
        compute_location_from_locations(&item_locations)
    };

    ContentItem::List(List {
        items: crate::lex::ast::elements::container::ListContainer::from_typed(typed_items),
        marker,
        annotations: Vec::new(),
        location,
    })
}

// ============================================================================
// LIST ITEM CREATION
// ============================================================================

/// Create a ListItem AST node from extracted list item data.
///
/// Converts byte range to AST Range, creates TextContent for marker,
/// and aggregates location from marker and children.
///
/// # Arguments
///
/// * `data` - Extracted list item data with marker text and byte range
/// * `content` - Child content items
/// * `source` - Original source string
///
/// # Returns
///
/// A ListItem node (not wrapped in ContentItem)
pub(super) fn list_item_node(
    data: ListItemData,
    content: Vec<ContentElement>,
    source_location: &SourceLocation,
) -> ListItem {
    let marker_location = byte_range_to_ast_range(data.marker_byte_range, source_location);
    let marker = TextContent::from_string(data.marker_text, Some(marker_location.clone()));

    let body_location = byte_range_to_ast_range(data.body_byte_range, source_location);
    let body = TextContent::from_string(data.body_text, Some(body_location.clone()));

    let child_items: Vec<ContentItem> = content.iter().cloned().map(ContentItem::from).collect();
    let mut location_sources = vec![marker_location, body_location];
    location_sources.extend(child_items.iter().map(|item| item.range().clone()));
    let location = compute_location_from_locations(&location_sources);

    ListItem::with_text_content(marker, body, content).at(location)
}

// ============================================================================
// ANNOTATION CREATION
// ============================================================================

/// Build a Data AST node from extracted information.
pub(super) fn data_node(data: DataExtraction, source_location: &SourceLocation) -> Data {
    use crate::lex::ast::Parameter;

    let label_location = byte_range_to_ast_range(data.label_byte_range, source_location);
    let label = Label::new(data.label_text).at(label_location.clone());

    // Convert ParameterData to Parameter AST nodes
    let mut parameter_ranges = vec![label_location.clone()];
    let parameters: Vec<Parameter> = data
        .parameters
        .into_iter()
        .map(|param_data| {
            let location = byte_range_to_ast_range(param_data.overall_byte_range, source_location);
            parameter_ranges.push(location.clone());
            Parameter {
                key: param_data.key_text,
                value: param_data.value_text.unwrap_or_default(),
                location,
            }
        })
        .collect();

    let location = compute_location_from_locations(&parameter_ranges);

    Data::new(label, parameters).at(location)
}

/// Create an Annotation AST node from a Data node and child content.
pub(super) fn annotation_node(data: Data, content: Vec<ContentElement>) -> ContentItem {
    let child_items: Vec<ContentItem> = content.iter().cloned().map(ContentItem::from).collect();
    let location = aggregate_locations(data.location.clone(), &child_items);

    let annotation = Annotation::from_data(data, content).at(location);

    ContentItem::Annotation(annotation)
}

// ============================================================================
// VERBATIM BLOCK CREATION
// ============================================================================

/// Create a VerbatimBlock AST node from extracted verbatim block data.
///
/// Converts byte ranges to AST Ranges, creates TextContent for subject and content,
/// and aggregates location from all components.
///
/// # Arguments
///
/// * `data` - Extracted verbatim block data (with indentation wall already stripped)
/// * `closing_data` - The closing data node
/// * `source` - Original source string
///
/// # Returns
///
/// A VerbatimBlock ContentItem
pub(super) fn verbatim_block_node(
    data: VerbatimBlockData,
    closing_data: Data,
    source_location: &SourceLocation,
) -> ContentItem {
    if data.groups.is_empty() {
        panic!("Verbatim blocks must contain at least one subject/content pair");
    }
    let mode = data.mode;
    let mut data_groups = data.groups.into_iter();
    let (first_subject, first_children, mut location_sources) =
        build_verbatim_group(data_groups.next().unwrap(), source_location);
    let mut additional_groups: Vec<VerbatimGroupItem> = Vec::new();
    for group_data in data_groups {
        let (subject, children, mut group_locations) =
            build_verbatim_group(group_data, source_location);
        location_sources.append(&mut group_locations);
        additional_groups.push(VerbatimGroupItem::new(subject, children));
    }
    location_sources.push(closing_data.location.clone());
    let location = compute_location_from_locations(&location_sources);
    let verbatim_block = Verbatim::new(first_subject, first_children, closing_data, mode)
        .with_additional_groups(additional_groups)
        .at(location);
    ContentItem::VerbatimBlock(Box::new(verbatim_block))
}

fn build_verbatim_group(
    group_data: VerbatimGroupData,
    source_location: &SourceLocation,
) -> (TextContent, Vec<VerbatimContent>, Vec<Range>) {
    use crate::lex::ast::elements::VerbatimLine;

    let subject_location = byte_range_to_ast_range(group_data.subject_byte_range, source_location);
    let subject = TextContent::from_string(group_data.subject_text, Some(subject_location.clone()));

    let mut children: Vec<VerbatimContent> = Vec::new();
    let mut locations: Vec<Range> = vec![subject_location];

    for (line_text, line_byte_range) in group_data.content_lines {
        let line_location = byte_range_to_ast_range(line_byte_range, source_location);
        locations.push(line_location.clone());

        let line_content = TextContent::from_string(line_text, Some(line_location.clone()));
        let verbatim_line = VerbatimLine::from_text_content(line_content).at(line_location);
        children.push(VerbatimContent::VerbatimLine(verbatim_line));
    }

    // Children are all VerbatimLines by construction - no validation needed
    (subject, children, locations)
}

// ============================================================================
// BLANK LINE GROUP CREATION
// ============================================================================

/// Create a BlankLineGroup AST node from normalized blank line tokens.
pub(super) fn blank_line_group_node(
    tokens: Vec<(Token, ByteRange<usize>)>,
    source_location: &SourceLocation,
) -> ContentItem {
    if tokens.is_empty() {
        return ContentItem::BlankLineGroup(BlankLineGroup::new(0, vec![]).at(default_location()));
    }

    let count = tokens
        .iter()
        .filter(|(token, _)| matches!(token, Token::BlankLine(_)))
        .count()
        .max(1);

    let ast_locations: Vec<Range> = tokens
        .iter()
        .map(|(_, span)| byte_range_to_ast_range(span.clone(), source_location))
        .collect();
    let location = compute_location_from_locations(&ast_locations);
    let source_tokens = tokens.into_iter().map(|(token, _)| token).collect();

    ContentItem::BlankLineGroup(BlankLineGroup::new(count, source_tokens).at(location))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::elements::typed_content::{ContentElement, SessionContent};
    use crate::lex::ast::elements::verbatim::VerbatimBlockMode;
    use crate::lex::ast::range::SourceLocation;
    use crate::lex::ast::traits::AstNode;
    use crate::lex::ast::Position;
    use crate::lex::building::extraction;

    #[test]
    fn test_paragraph_node() {
        let source = "hello";
        let source_location = SourceLocation::new(source);
        let data = ParagraphData {
            text_lines: vec![("hello".to_string(), 0..5)],
            overall_byte_range: 0..5,
        };

        let result = paragraph_node(data, &source_location);

        match result {
            ContentItem::Paragraph(para) => {
                assert_eq!(para.lines.len(), 1);
                assert_eq!(para.location.start, Position::new(0, 0));
                assert_eq!(para.location.end, Position::new(0, 5));
            }
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_session_node() {
        let source = "Session";
        let source_location = SourceLocation::new(source);
        let data = SessionData {
            title_text: "Session".to_string(),
            title_byte_range: 0..7,
            marker: None,
        };

        let result = session_node(data, Vec::<SessionContent>::new(), &source_location);

        match result {
            ContentItem::Session(session) => {
                assert_eq!(session.title.as_string(), "Session");
                assert_eq!(session.location.start, Position::new(0, 0));
                assert_eq!(session.location.end, Position::new(0, 7));
            }
            _ => panic!("Expected Session"),
        }
    }

    #[test]
    fn test_data_node_assigns_parameter_locations() {
        let source = "note severity=high";
        let source_location = SourceLocation::new(source);
        let extraction = extraction::extract_data(
            vec![
                (Token::Text("note".to_string()), 0..4),
                (Token::Whitespace(1), 4..5),
                (Token::Text("severity".to_string()), 5..13),
                (Token::Equals, 13..14),
                (Token::Text("high".to_string()), 14..18),
            ],
            source,
        );

        let data = data_node(extraction, &source_location);

        assert_eq!(data.label.value, "note");
        assert_eq!(data.label.location.span, 0..5);
        assert_eq!(data.parameters.len(), 1);
        assert_eq!(data.parameters[0].location.span, 5..18);
        assert_eq!(data.location.span, 0..18);
    }

    #[test]
    fn test_verbatim_block_node_aggregates_groups() {
        let source = "Example:\n    code line\nOther:\n    more\n:: shell ::\n";
        let source_location = SourceLocation::new(source);

        fn span(haystack: &str, needle: &str) -> std::ops::Range<usize> {
            let start = haystack.find(needle).expect("needle not found");
            start..start + needle.len()
        }

        let data = VerbatimBlockData {
            groups: vec![
                VerbatimGroupData {
                    subject_text: "Example:".to_string(),
                    subject_byte_range: span(source, "Example:"),
                    content_lines: vec![("code line".to_string(), span(source, "code line"))],
                },
                VerbatimGroupData {
                    subject_text: "Other:".to_string(),
                    subject_byte_range: span(source, "Other:"),
                    content_lines: vec![("more".to_string(), span(source, "more"))],
                },
            ],
            mode: VerbatimBlockMode::Inflow,
        };

        let closing_span = span(source, ":: shell ::");
        let closing_label_span = span(source, "shell");
        let closing_label = Label::new("shell".to_string()).at(byte_range_to_ast_range(
            closing_label_span,
            &source_location,
        ));
        let closing_data = Data::new(closing_label, Vec::new()).at(byte_range_to_ast_range(
            closing_span.clone(),
            &source_location,
        ));

        let block = match verbatim_block_node(data, closing_data, &source_location) {
            ContentItem::VerbatimBlock(block) => block,
            other => panic!("Expected verbatim block, got {:?}", other.node_type()),
        };

        assert_eq!(block.location.span, 0..closing_span.end);
        assert_eq!(
            block.subject.location.as_ref().unwrap().span,
            span(source, "Example:")
        );
        assert_eq!(block.group_len(), 2);

        let mut groups = block.group();
        let first = groups.next().expect("first group missing");
        assert_eq!(
            first.subject.location.as_ref().unwrap().span,
            span(source, "Example:")
        );
        if let Some(ContentItem::VerbatimLine(line)) = first.children.iter().next() {
            assert_eq!(line.location.span, span(source, "code line"));
        } else {
            panic!("expected verbatim line in first group");
        }

        let second = groups.next().expect("second group missing");
        assert_eq!(
            second.subject.location.as_ref().unwrap().span,
            span(source, "Other:")
        );
        if let Some(ContentItem::VerbatimLine(line)) = second.children.iter().next() {
            assert_eq!(line.location.span, span(source, "more"));
        } else {
            panic!("expected verbatim line in second group");
        }

        assert_eq!(block.closing_data.location.span, closing_span);
    }

    // ============================================================================
    // VALIDATION TESTS
    // ============================================================================

    #[test]
    fn test_session_allows_session_child() {
        use crate::lex::ast::elements::Session;

        let source = "Parent Session\n    Nested Session\n";
        let source_location = SourceLocation::new(source);
        let nested_session = Session::with_title("Nested Session".to_string());
        let content = vec![SessionContent::Session(nested_session)];

        let data = SessionData {
            title_text: "Parent Session".to_string(),
            title_byte_range: 0..14,
            marker: None,
        };

        // This should succeed - Sessions can contain Sessions
        let result = session_node(data, content, &source_location);

        match result {
            ContentItem::Session(session) => {
                assert_eq!(session.children.len(), 1);
                assert_eq!(session.title.as_string(), "Parent Session");
            }
            _ => panic!("Expected Session"),
        }
    }

    #[test]
    fn test_definition_allows_non_session_children() {
        use crate::lex::ast::elements::Paragraph;

        let source = "Test Subject:\n    Some content\n";
        let source_location = SourceLocation::new(source);
        let para = Paragraph::from_line("Some content".to_string());
        let content = vec![ContentElement::Paragraph(para)];

        let data = DefinitionData {
            subject_text: "Test Subject".to_string(),
            subject_byte_range: 0..12,
        };

        // This should succeed - Definitions can contain Paragraphs
        let result = definition_node(data, content, &source_location);

        match result {
            ContentItem::Definition(def) => {
                assert_eq!(def.children.len(), 1);
                assert_eq!(def.subject.as_string(), "Test Subject");
            }
            _ => panic!("Expected Definition"),
        }
    }

    #[test]
    fn test_annotation_allows_non_session_children() {
        use crate::lex::ast::elements::Paragraph;

        let source = ":: note ::\n    Some content\n";
        let source_location = SourceLocation::new(source);
        let para = Paragraph::from_line("Some content".to_string());
        let content = vec![ContentElement::Paragraph(para)];

        let data = DataExtraction {
            label_text: "note".to_string(),
            label_byte_range: 0..4,
            parameters: vec![],
        };

        // This should succeed - Annotations can contain Paragraphs
        let data_node = data_node(data, &source_location);
        let result = annotation_node(data_node, content);

        match result {
            ContentItem::Annotation(ann) => {
                assert_eq!(ann.children.len(), 1);
                assert_eq!(ann.data.label.value, "note");
            }
            _ => panic!("Expected Annotation"),
        }
    }

    #[test]
    fn test_list_item_allows_non_session_children() {
        use crate::lex::ast::elements::Paragraph;

        let source = "- Item\n    Some content\n";
        let source_location = SourceLocation::new(source);
        let para = Paragraph::from_line("Item content".to_string());
        let content = vec![ContentElement::Paragraph(para)];

        let data = ListItemData {
            marker_text: "-".to_string(),
            marker_byte_range: 0..1,
            body_text: "Item".to_string(),
            body_byte_range: 2..6,
        };

        // This should succeed - ListItems can contain Paragraphs
        let result = list_item_node(data, content, &source_location);
        assert_eq!(result.children.len(), 1);
        assert_eq!(result.marker(), "-");
        assert_eq!(result.text(), "Item");
    }
}
