//! Markdown serialization (Lex → Markdown export)
//!
//! Converts Lex documents to CommonMark Markdown.
//! Pipeline: Lex AST → IR → Events → Comrak AST → Markdown string
//!
//! This file is the façade. The pipeline is split across siblings:
//! - [`comrak_build`] — the IR-event → Comrak-AST walk (`build_comrak_ast`,
//!   `add_inline_to_node`) and the Comrak option set.
//! - [`frontmatter`] — YAML frontmatter synthesis from
//!   `document_annotations`, title-as-H1 prepending, and the text
//!   flattening / escape helpers those steps share.

mod comrak_build;
mod frontmatter;

#[cfg(test)]
mod tests;

use crate::common::nested_to_flat::tree_to_events;
use crate::error::FormatError;
use crate::ir::nodes::DocNode;
use crate::render_dispatch::dispatch_render;
use comrak::{format_commonmark, Arena};
use lex_core::lex::ast::Document;
use lex_extension_host::Registry;

use comrak_build::{build_comrak_ast, default_comrak_options};
use frontmatter::{
    inlines_to_text, prepend_title_as_h1, render_document_annotations_as_yaml,
    strip_heading_digit_dot_escape,
};

/// Result of [`serialize_to_markdown_with_registry`]: the rendered
/// Markdown plus any handler-emitted diagnostic messages (renderer
/// errors, format-shape mismatches, namespace disabled).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownExportOutcome {
    pub markdown: String,
    pub diagnostics: Vec<String>,
}

/// Serialize a Lex document to Markdown, dispatching `on_render`
/// handlers through [`crate::default_registry()`]. Diagnostics
/// produced by handlers are discarded; for diagnostic-aware
/// serialization use [`serialize_to_markdown_with_registry`].
pub fn serialize_to_markdown(doc: &Document) -> Result<String, FormatError> {
    let outcome = serialize_to_markdown_with_registry(doc, crate::default_registry())?;
    Ok(outcome.markdown)
}

/// Serialize with an extension-system [`Registry`] in scope: every
/// labelled annotation whose schema declares `hooks.render: ["markdown"]`
/// is dispatched to its handler. Handlers returning `RenderOut::String`
/// are spliced into the output in place of the annotation's default
/// rendering. For doc-scope annotations (the `document_annotations` IR
/// slot), handler output is consumed as a YAML frontmatter line; the
/// `lex.metadata.*` family without registered handlers falls back to
/// in-place synthesis.
pub fn serialize_to_markdown_with_registry(
    doc: &Document,
    registry: &Registry,
) -> Result<MarkdownExportOutcome, FormatError> {
    let ir_doc = crate::to_ir_with_registry(doc, registry);
    let plan = dispatch_render(&ir_doc, registry, "markdown");

    // Extract title from IR
    let document_title = ir_doc.title.as_ref().map(|title_inlines| {
        let title_text = inlines_to_text(title_inlines);
        let subtitle_text = ir_doc.subtitle.as_ref().map(|sub| inlines_to_text(sub));
        (title_text, subtitle_text)
    });

    // Phase 3b (#614): YAML frontmatter is synthesized directly from
    // `document_annotations` rather than via a `frontmatter` event
    // that `tree_to_events` used to inject. The IR slot is the single
    // source of truth on the lex → markdown path. For doc-scope
    // annotations with a registered render handler (e.g. `doc.*`), the
    // handler's YAML line replaces the default synthesis; everything
    // else (`lex.metadata.*` shortcuts) falls back to the legacy
    // synthesis path.
    let doc_scope_plan = &plan.nodes[..plan.doc_scope_count];
    let body_plan = &plan.nodes[plan.doc_scope_count..];
    let frontmatter_yaml =
        render_document_annotations_as_yaml(&ir_doc.document_annotations, doc_scope_plan);

    // Step 2: IR → Events
    let events = tree_to_events(&DocNode::Document(ir_doc));

    // Step 3: Events → Comrak AST. SpliceState consumes the body plan
    // to replace handler-rendered annotations with their raw markdown
    // output (emitted as a `NodeValue::HtmlBlock` literal so Comrak
    // passes it through unchanged).
    let arena = Arena::new();
    let root = build_comrak_ast(&arena, &events, body_plan)?;

    // Step 4: Comrak AST → Markdown string (using comrak's serializer)
    let mut output = Vec::new();
    let options = default_comrak_options();
    format_commonmark(root, &options, &mut output).map_err(|e| {
        FormatError::SerializationError(format!("Comrak serialization failed: {e}"))
    })?;

    let markdown = String::from_utf8(output)
        .map_err(|e| FormatError::SerializationError(format!("UTF-8 conversion failed: {e}")))?;

    // Remove Comrak's "end list" HTML comments which appear between consecutive lists
    let cleaned = markdown.replace("<!-- end list -->\n\n", "");

    // #606: Strip Comrak's backslash-escape on a leading digit-dot inside an
    // ATX heading. Comrak escapes `1.` at heading start to disambiguate from
    // an ordered-list marker, but a `#`-headed line can't open a list — the
    // escape is visually noisy. Only applies at the start of a heading line.
    let cleaned = strip_heading_digit_dot_escape(&cleaned);

    // Prepend document title as H1 heading if present
    let with_title = prepend_title_as_h1(&cleaned, document_title);

    // Prepend YAML frontmatter from document_annotations, if any.
    // Markdown imports already place a `frontmatter` annotation in
    // `children[0]` (handled inside `build_comrak_ast`), so this
    // branch fires only when the IR was built from a lex source —
    // the two paths don't double-write.
    let with_frontmatter = match frontmatter_yaml {
        Some(yaml) => format!("{yaml}{with_title}"),
        None => with_title,
    };

    let mut diagnostics: Vec<String> = plan
        .nodes
        .iter()
        .filter_map(|n| n.diagnostic.clone())
        .collect();
    diagnostics.extend(plan.root_diagnostics);

    Ok(MarkdownExportOutcome {
        markdown: with_frontmatter,
        diagnostics,
    })
}
