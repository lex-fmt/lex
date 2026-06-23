//! Walk/visit ordering behaviour: labelled-verbatim dispatch via the
//! `language` field, and table header-before-body traversal order.

use super::*;
use crate::render_dispatch::dispatch_render;
use lex_extension::wire::Format;
use lex_extension::{HandlerError, LabelCtx, LexHandler, RenderOut};
use lex_extension_host::Registry;

/// Regression for Copilot's review on PR #623: a labelled verbatim
/// block without an `on_ir_build` handler still needs render
/// dispatch — `from_lex_verbatim` falls back to `DocNode::Verbatim`
/// with the closing label preserved in `language`, and
/// `visit_verbatim` resurrects dispatch via that field.
#[test]
fn verbatim_label_with_on_render_only_dispatches_via_language_field() {
    let registry = Registry::new();
    // `verbatim_label: true`, html render hook, no `on_ir_build` —
    // the schema declares the label as a verbatim closer but
    // doesn't hydrate it into a typed IR node, so it stays a
    // generic `DocNode::Verbatim`.
    let mut s = schema("acme.snippet", &["html"]);
    s.verbatim_label = true;
    registry
        .register_namespace("acme", vec![s], Box::new(EchoRender))
        .unwrap();
    // Note the closing `::` line — that's the verbatim form's
    // closer carrying the label.
    let doc = parse("Code:\n    let x = 1;\n:: acme.snippet ::\n");
    let plan = dispatch_render(&doc, &registry, "html");
    assert_eq!(
        plan.nodes.len(),
        1,
        "labelled verbatim with on_render hook should produce a plan entry"
    );
    assert_eq!(plan.nodes[0].label, "acme.snippet");
    assert!(plan.nodes[0]
        .output
        .as_deref()
        .is_some_and(|s| s.contains("acme.snippet")));
}

/// Regression for Copilot's review on PR #623: the dispatch walk
/// must visit table header rows before body rows so the plan
/// indexing matches `tree_to_events`'s emit order (header-first).
/// Otherwise an annotation inside a header cell would get its
/// handler output spliced into the wrong cell.
#[test]
fn table_header_cells_walked_before_body_cells() {
    use std::sync::Mutex;
    struct OrderedCapture {
        seen: std::sync::Arc<Mutex<Vec<String>>>,
    }
    impl LexHandler for OrderedCapture {
        fn on_render(&self, ctx: &LabelCtx, _: Format) -> Result<Option<RenderOut>, HandlerError> {
            self.seen.lock().unwrap().push(ctx.label.clone());
            Ok(None)
        }
    }
    let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![
                schema("acme.in_header", &["html"]),
                schema("acme.in_body", &["html"]),
            ],
            Box::new(OrderedCapture { seen: seen.clone() }),
        )
        .unwrap();
    // Synthesise an IR Table with a labelled annotation in a
    // header cell and another in a body cell. Test the walker
    // directly rather than parsing — keeps the regression
    // hermetic to the dispatch order.
    use crate::ir::nodes::{
        Annotation as IrAnn, DocNode as IrNode, Document as IrDoc, LabelForm, Paragraph as IrPara,
        Table as IrTable, TableCell as IrCell, TableCellAlignment, TableRow as IrRow,
    };
    fn cell_with_annotation(label: &str) -> IrCell {
        IrCell {
            content: vec![
                IrNode::Annotation(IrAnn {
                    label: label.into(),
                    parameters: Vec::new(),
                    content: Vec::new(),
                    form: LabelForm::Canonical,
                }),
                IrNode::Paragraph(IrPara {
                    content: vec![crate::ir::nodes::InlineContent::Text("cell".into())],
                }),
            ],
            header: false,
            align: TableCellAlignment::None,
            colspan: 1,
            rowspan: 1,
        }
    }
    let doc = IrDoc {
        title: None,
        subtitle: None,
        children: vec![IrNode::Table(IrTable {
            rows: vec![IrRow {
                cells: vec![cell_with_annotation("acme.in_body")],
            }],
            header: vec![IrRow {
                cells: vec![cell_with_annotation("acme.in_header")],
            }],
            caption: None,
            footnotes: Vec::new(),
            fullwidth: false,
        })],
        document_annotations: Vec::new(),
    };
    let _ = dispatch_render(&doc, &registry, "html");
    let kinds = seen.lock().unwrap().clone();
    assert_eq!(
        kinds.as_slice(),
        &["acme.in_header", "acme.in_body"],
        "header cells must be walked before body cells"
    );
}
