//! Tests for spec/overview documents in comms/specs/elements/

use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;
use lex_core::lex::testing::workspace_path;

// ============================================================================
// Element spec documents
// ============================================================================

#[test]
fn spec_annotation() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/annotation.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(4).item(0, |item| {
        item.assert_session().label("Introduction");
    });
}

#[test]
fn spec_data() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/data.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(6).item(0, |item| {
        item.assert_session().label("Introduction");
    });
}

#[test]
fn spec_definition() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/definition.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(6).item(0, |item| {
        item.assert_session().label("Introduction");
    });
}

#[test]
fn spec_document() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/document.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(1).item(0, |item| {
        item.assert_session().label("Document Title");
    });
}

#[test]
fn spec_escaping() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/escaping.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(5).item(0, |item| {
        item.assert_session().label("Feature: Escaping");
    });
}

#[test]
fn spec_footnotes() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/footnotes.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(5).item(0, |item| {
        item.assert_session().label("Introduction");
    });
}

#[test]
fn spec_inlines() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/inlines.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(5).item(0, |item| {
        item.assert_session().label("Feature: Inlines");
    });
}

#[test]
fn spec_label() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/label.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(4).item(0, |item| {
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
fn spec_list() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/list.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(10).item(0, |item| {
        item.assert_session().label("Introduction");
    });
}

#[test]
fn spec_paragraph() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/paragraph.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(9).item(0, |item| {
        item.assert_session().label("Introduction");
    });
}

#[test]
fn spec_parameter() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/parameter.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(6).item(0, |item| {
        item.assert_session().label("Introduction");
    });
}

#[test]
fn spec_session() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/session.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(8).item(0, |item| {
        item.assert_session().label("Introduction");
    });
}

#[test]
fn spec_table() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/table.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc).item_count(14).item(0, |item| {
        item.assert_session().label("Introduction");
    });
}

#[test]
fn spec_verbatim() {
    let doc = Lexplore::from_path(workspace_path("comms/specs/elements/verbatim.lex"))
        .parse()
        .unwrap();
    assert_ast(&doc)
        .item_count(10)
        .item(0, |item| {
            item.assert_session().label("Introduction");
        })
        .item(1, |item| {
            item.assert_session().label("Syntax");
        });
}

// ============================================================================
// Template documents
// ============================================================================

#[test]
fn spec_template_document_simple() {
    let doc = Lexplore::from_path(workspace_path(
        "comms/specs/elements/XXX-document-simple.lex",
    ))
    .parse()
    .unwrap();

    assert_ast(&doc).item_count(10).item(1, |item| {
        item.assert_session()
            .label("1. Session with Paragraph Content {{session-title}}")
            .child(2, |child| {
                child.assert_paragraph().text("<insert element here>");
            });
    });
}

#[test]
fn spec_template_document_tricky() {
    let doc = Lexplore::from_path(workspace_path(
        "comms/specs/elements/XXX-document-tricky.lex",
    ))
    .parse()
    .unwrap();

    assert_ast(&doc).item_count(4).item(1, |item| {
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

// ============================================================================
// Discovery: ensure all element spec files are tested
// ============================================================================

#[test]
fn all_element_spec_files_covered() {
    let dir = workspace_path("comms/specs/elements");
    let tested = [
        "annotation.lex",
        "data.lex",
        "definition.lex",
        "document.lex",
        "escaping.lex",
        "footnotes.lex",
        "inlines.lex",
        "label.lex",
        "list.lex",
        "paragraph.lex",
        "parameter.lex",
        "session.lex",
        "table.lex",
        "verbatim.lex",
        "XXX-document-simple.lex",
        "XXX-document-tricky.lex",
    ];

    let entries: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".lex") {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    for name in &entries {
        assert!(
            tested.contains(&name.as_str()),
            "Element spec file {name} exists but has no test — add one above"
        );
    }
}
