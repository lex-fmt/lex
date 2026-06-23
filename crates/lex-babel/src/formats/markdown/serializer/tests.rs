use super::*;
use comrak::nodes::NodeValue;
use comrak::{parse_document, Arena, ComrakOptions};
use lex_core::lex::transforms::standard::STRING_TO_AST;

#[test]
fn test_simple_paragraph_ast() {
    let lex_src = "This is a simple paragraph.\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    // Convert to markdown
    let md = serialize_to_markdown(&lex_doc).unwrap();

    // Parse back to comrak AST to verify structure
    let arena = Arena::new();
    let options = ComrakOptions::default();
    let root = parse_document(&arena, &md, &options);

    // Verify we have a paragraph
    let mut found_paragraph = false;
    for child in root.children() {
        if matches!(child.data.borrow().value, NodeValue::Paragraph) {
            found_paragraph = true;

            // Check inline text content
            for _inline in child.children() {
                if let NodeValue::Text(ref text) = child.data.borrow().value {
                    assert!(text.contains("simple paragraph"));
                }
            }
        }
    }
    assert!(found_paragraph, "Should have a paragraph node");
}

#[test]
fn test_heading_ast() {
    let lex_src = "1. Introduction\n\n    Content here.\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

    let md = serialize_to_markdown(&lex_doc).unwrap();

    // Parse and verify AST structure
    let arena = Arena::new();
    let options = ComrakOptions::default();
    let root = parse_document(&arena, &md, &options);

    let mut found_heading = false;
    for child in root.children() {
        if let NodeValue::Heading(ref heading) = child.data.borrow().value {
            assert_eq!(heading.level, 2);
            found_heading = true;
        }
    }
    assert!(found_heading, "Should have a heading node");
}

/// Phase 3b (#614): a lex source with `:: lex.metadata.* ::`
/// document-scope annotations must surface in the markdown
/// output as a YAML frontmatter block. Pre-Phase-3b this worked
/// via the `frontmatter` event synthesis in `tree_to_events`;
/// after the flip the markdown serializer synthesizes YAML
/// directly from `document_annotations`.
#[test]
fn lex_metadata_annotations_emit_yaml_frontmatter() {
    let lex_src = ":: title :: My Doc\n\n:: author :: Alice\n\nBody.\n";
    let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();
    let md = serialize_to_markdown(&lex_doc).unwrap();

    assert!(
        md.starts_with("---\n"),
        "expected YAML frontmatter at start of markdown, got:\n{md}"
    );
    assert!(md.contains("title: My Doc"), "missing title key in:\n{md}");
    assert!(md.contains("author: Alice"), "missing author key in:\n{md}");
}

/// Phase 3b coverage retained from #596 / #597: the YAML body
/// flatten must read `Reference` and `Link` inlines (not only
/// `Text`/`Code`/`Math`) so a `:: author :: Alice [https://…]`
/// doesn't silently drop the link text in the YAML preamble.
#[test]
fn yaml_synthesis_includes_link_and_reference_inlines() {
    use crate::ir::nodes::{Annotation as IrAnn, DocNode, InlineContent, LabelForm, Paragraph};

    let yaml = render_document_annotations_as_yaml(
        &[IrAnn {
            label: "lex.metadata.author".to_string(),
            parameters: vec![],
            content: vec![DocNode::Paragraph(Paragraph {
                content: vec![
                    InlineContent::Text("Alice ".to_string()),
                    InlineContent::Link {
                        text: "https://alice.example".to_string(),
                        href: "https://alice.example".to_string(),
                    },
                ],
            })],
            form: LabelForm::Canonical,
        }],
        &[],
    )
    .expect("yaml block synthesized");
    assert!(
        yaml.contains("author: Alice https://alice.example"),
        "{yaml}"
    );

    let yaml = render_document_annotations_as_yaml(
        &[IrAnn {
            label: "lex.metadata.tags".to_string(),
            parameters: vec![],
            content: vec![DocNode::Paragraph(Paragraph {
                content: vec![
                    InlineContent::Text("rust ".to_string()),
                    InlineContent::Reference {
                        raw: "@manning".to_string(),
                        kind: crate::ir::nodes::ReferenceType::NotSure,
                    },
                ],
            })],
            form: LabelForm::Canonical,
        }],
        &[],
    )
    .expect("yaml block synthesized");
    assert!(yaml.contains("tags: rust @manning"), "{yaml}");
}

/// Gemini review on PR #621: a multi-line annotation body emits
/// `InlineContent::Text("\n")` separators between lines (see
/// `from_lex_paragraph`). A literal `\n` inside a YAML scalar
/// orphans the trailing lines — the YAML parser reads them as
/// keyless content. Collapse internal newlines to spaces so the
/// preamble stays valid.
#[test]
fn yaml_synthesis_collapses_internal_newlines_to_spaces() {
    use crate::ir::nodes::{Annotation as IrAnn, DocNode, InlineContent, LabelForm, Paragraph};

    let yaml = render_document_annotations_as_yaml(
        &[IrAnn {
            label: "lex.metadata.note".to_string(),
            parameters: vec![],
            content: vec![DocNode::Paragraph(Paragraph {
                content: vec![
                    InlineContent::Text("Line one.".to_string()),
                    InlineContent::Text("\n".to_string()),
                    InlineContent::Text("Line two.".to_string()),
                ],
            })],
            form: LabelForm::Canonical,
        }],
        &[],
    )
    .expect("yaml block synthesized");

    assert!(
        yaml.contains("note: Line one. Line two.\n"),
        "internal newlines must collapse to spaces; got:\n{yaml}"
    );
    // Defensive: the value line itself must not contain a raw
    // newline character that would break YAML scalar parsing.
    let value_line = yaml.lines().find(|l| l.starts_with("note:")).unwrap();
    assert!(
        !value_line.contains('\n'),
        "value line must be single-line in YAML"
    );
}

/// Gemini review on PR #597: the body flatten must also cover
/// `Code` and `Math` inline content so a metadata body like
/// `Doc with \`snippet\` and #E=mc^2#` doesn't silently drop the
/// code/math text in the YAML preamble.
#[test]
fn yaml_synthesis_includes_code_and_math_inlines() {
    use crate::ir::nodes::{Annotation as IrAnn, DocNode, InlineContent, LabelForm, Paragraph};

    let yaml = render_document_annotations_as_yaml(
        &[IrAnn {
            label: "lex.metadata.title".to_string(),
            parameters: vec![],
            content: vec![DocNode::Paragraph(Paragraph {
                content: vec![
                    InlineContent::Text("Doc with ".to_string()),
                    InlineContent::Code("snippet".to_string()),
                    InlineContent::Text(" and ".to_string()),
                    InlineContent::Math("E=mc^2".to_string()),
                ],
            })],
            form: LabelForm::Canonical,
        }],
        &[],
    )
    .expect("yaml block synthesized");
    assert!(
        yaml.contains("title: Doc with snippet and E=mc^2"),
        "{yaml}"
    );
}
