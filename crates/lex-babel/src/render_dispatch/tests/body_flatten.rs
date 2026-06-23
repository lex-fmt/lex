//! `BodyKind::Text` body flattening — the
//! [`flatten_text_body`](super::super::flatten_text_body) helper that turns
//! an annotation's IR body into a single plain-text scalar.

use super::super::flatten_text_body;

/// Gemini review on PR #625: multi-paragraph bodies must join with
/// a single space (not `\n`) so the resulting YAML scalar stays
/// single-line. A literal newline orphans the trailing text from
/// its key when the handler embeds the value as `key: "<scalar>"`.
#[test]
fn flatten_text_body_joins_paragraphs_with_space_not_newline() {
    use crate::ir::nodes::{DocNode as IrNode, InlineContent, Paragraph};
    let body = vec![
        IrNode::Paragraph(Paragraph {
            content: vec![InlineContent::Text("First line.".into())],
        }),
        IrNode::Paragraph(Paragraph {
            content: vec![InlineContent::Text("Second line.".into())],
        }),
    ];
    let flat = flatten_text_body(&body);
    assert_eq!(flat, "First line. Second line.");
    assert!(
        !flat.contains('\n'),
        "flattened scalar must not contain raw newlines"
    );
}

/// Gemini review on PR #625: bare `DocNode::Inline` siblings (inlines
/// not wrapped in a Paragraph) are an unusual doc-scope shape, but
/// the flatten must include them rather than silently dropping content.
#[test]
fn flatten_text_body_processes_bare_inline_doc_nodes() {
    use crate::ir::nodes::{DocNode as IrNode, InlineContent};
    let body = vec![
        IrNode::Inline(InlineContent::Text("Hello, ".into())),
        IrNode::Inline(InlineContent::Text("world.".into())),
    ];
    assert_eq!(flatten_text_body(&body), "Hello, world.");
}

/// Copilot review on PR #625: text inside `Bold` and `Italic`
/// formatting containers must be preserved when flattening for a
/// `BodyKind::Text` schema. A metadata value like
/// `:: doc.title :: *Important* Title` would otherwise reach the
/// handler as `" Title"` — user-authored content silently dropped.
#[test]
fn flatten_text_body_recurses_through_bold_and_italic() {
    use crate::ir::nodes::{DocNode as IrNode, InlineContent, Paragraph};
    let body = vec![IrNode::Paragraph(Paragraph {
        content: vec![
            InlineContent::Italic(vec![InlineContent::Text("Important".into())]),
            InlineContent::Text(" ".into()),
            InlineContent::Bold(vec![
                InlineContent::Text("very ".into()),
                InlineContent::Italic(vec![InlineContent::Text("bold".into())]),
            ]),
            InlineContent::Text(" Title".into()),
        ],
    })];
    assert_eq!(flatten_text_body(&body), "Important very bold Title");
}
