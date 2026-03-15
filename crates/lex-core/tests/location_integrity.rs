use lex_core::lex::ast::elements::{Annotation, Session};
use lex_core::lex::ast::{AstNode, ContentItem, Position, Range, TextContent};
use lex_core::lex::parsing::parse_document;
use lex_core::lex::testing::workspace_path;

fn assert_range_in_source(range: &Range, source: &str) {
    assert!(
        range.span.start <= range.span.end,
        "invalid span ordering: {:?}",
        range.span
    );
    assert!(
        range.span.end <= source.len(),
        "span {:?} exceeds source length {}",
        range.span,
        source.len()
    );
    assert!(
        range.span.start < range.span.end,
        "zero-length span for range {range:?}"
    );
}

fn validate_text_content(text: &TextContent, source: &str) {
    if let Some(location) = &text.location {
        assert_range_in_source(location, source);
        if let Some(slice) = source.get(location.span.clone()) {
            assert_eq!(slice, text.as_string(), "text content does not match slice");
        }
    }
}

fn validate_annotation(annotation: &Annotation, source: &str) {
    assert_range_in_source(&annotation.location, source);
    for child in annotation.children.iter() {
        validate_item(child, source);
    }
}

fn validate_session(session: &Session, source: &str) {
    assert_range_in_source(session.range(), source);
    if let Some(loc) = &session.title.location {
        assert_range_in_source(loc, source);
        if let Some(slice) = source.get(loc.span.clone()) {
            assert_eq!(slice, session.title.as_string());
        }
    }
    for annotation in session.annotations() {
        validate_annotation(annotation, source);
    }
    for child in session.children.iter() {
        validate_item(child, source);
    }
}

fn validate_item(item: &ContentItem, source: &str) {
    assert_range_in_source(item.range(), source);
    match item {
        ContentItem::Session(session) => validate_session(session, source),
        ContentItem::Paragraph(paragraph) => {
            for annotation in &paragraph.annotations {
                validate_annotation(annotation, source);
            }
            for line in &paragraph.lines {
                validate_item(line, source);
            }
        }
        ContentItem::List(list) => {
            for annotation in &list.annotations {
                validate_annotation(annotation, source);
            }
            for entry in list.items.iter() {
                validate_item(entry, source);
            }
        }
        ContentItem::ListItem(list_item) => {
            for text in &list_item.text {
                validate_text_content(text, source);
            }
            for annotation in &list_item.annotations {
                validate_annotation(annotation, source);
            }
            for child in list_item.children.iter() {
                validate_item(child, source);
            }
        }
        ContentItem::Definition(definition) => {
            validate_text_content(&definition.subject, source);
            for annotation in &definition.annotations {
                validate_annotation(annotation, source);
            }
            for child in definition.children.iter() {
                validate_item(child, source);
            }
        }
        ContentItem::Annotation(annotation) => validate_annotation(annotation, source),
        ContentItem::VerbatimBlock(block) => {
            validate_text_content(&block.subject, source);
            for annotation in &block.annotations {
                validate_annotation(annotation, source);
            }
            for child in block.children.iter() {
                validate_item(child, source);
            }
        }
        ContentItem::VerbatimLine(line) => {
            assert_range_in_source(&line.location, source);
            validate_text_content(&line.content, source);
        }
        ContentItem::TextLine(line) => {
            assert_range_in_source(&line.location, source);
            validate_text_content(&line.content, source);
        }
        ContentItem::Table(table) => {
            validate_text_content(&table.subject, source);
            for annotation in &table.annotations {
                validate_annotation(annotation, source);
            }
            // Validate cell children locations
            for row in table.all_rows() {
                for cell in &row.cells {
                    for child in cell.children.iter() {
                        validate_item(child, source);
                    }
                }
            }
        }
        ContentItem::BlankLineGroup(_) => {}
    }
}

#[test]
fn all_fixture_nodes_have_valid_locations() {
    let fixtures = [
        "comms/specs/trifecta/060-trifecta-nesting.lex",
        "comms/specs/elements/paragraph.docs/paragraph-03-flat-special-chars.lex",
    ];

    for relative in fixtures {
        let path = workspace_path(relative);
        let source = std::fs::read_to_string(&path).expect("failed to read fixture");
        let document = parse_document(&source).expect("failed to parse fixture");

        validate_session(document.root_session(), &source);
    }
}

fn verify_lookup(session: &Session) {
    for child in session.children.iter() {
        let pos = position_inside(child.range());
        let found = session
            .element_at(pos)
            .unwrap_or_else(|| panic!("expected element at {pos:?}"));
        assert!(found.range().contains(pos));

        if let ContentItem::Session(nested) = child {
            verify_lookup(nested);
        }
    }
}

fn position_inside(range: &Range) -> Position {
    if range.start.line == range.end.line {
        let mut column = range.start.column;
        if range.end.column > range.start.column {
            column += 1;
        }
        Position::new(range.start.line, column)
    } else {
        Position::new(range.start.line + 1, 0)
    }
}

#[test]
fn cursor_positions_find_nested_nodes() {
    let path = workspace_path("comms/specs/trifecta/070-trifecta-flat-simple.lex");
    let source = std::fs::read_to_string(&path).expect("failed to read fixture");
    let document = parse_document(&source).expect("failed to parse fixture");

    verify_lookup(document.root_session());
}
