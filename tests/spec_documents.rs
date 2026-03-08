//! Tests for spec/overview documents that don't map to numbered element loaders

use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;
use lex_core::lex::testing::workspace_path;

#[test]
fn test_labels_spec_document() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/label.lex"))
        .parse()
        .unwrap();

    assert_ast(&doc).item(0, |item| {
        item.assert_session()
            .label_contains("Introduction")
            .child(0, |child| {
                child
                    .assert_paragraph()
                    .text_contains("identifiers for annotations");
            });
    });
}

#[test]
fn test_parameters_spec_document() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/parameter.lex"))
        .parse()
        .unwrap();

    assert_ast(&doc).item(0, |item| {
        item.assert_session().label("Introduction");
    });
}

#[test]
fn test_verbatim_spec_document() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/verbatim.lex"))
        .parse()
        .unwrap();

    assert_ast(&doc)
        .item(0, |item| {
            item.assert_session().label("Introduction");
        })
        .item(1, |item| {
            item.assert_session().label("Syntax");
        });
}

#[test]
fn test_template_document_simple() {
    let doc = Lexplore::from_path(workspace_path(
        "comms/specs/elements/XXX-document-simple.lex",
    ))
    .parse()
    .unwrap();

    assert_ast(&doc).item(1, |item| {
        item.assert_session()
            .label("1. Session with Paragraph Content {{session-title}}")
            .child(2, |child| {
                child.assert_paragraph().text("<insert element here>");
            });
    });
}

#[test]
fn test_template_document_tricky() {
    let doc = Lexplore::from_path(workspace_path(
        "comms/specs/elements/XXX-document-tricky.lex",
    ))
    .parse()
    .unwrap();

    assert_ast(&doc).item(1, |item| {
        item.assert_session()
            .label("1. Root Session {{session-title}}")
            .child(1, |child| {
                child
                    .assert_session()
                    .label("1.1. Sub-session with Paragraph {{session-title}}")
                    .child(1, |list_child| {
                        list_child.assert_list().item_count(2);
                    });
            });
    });
}
