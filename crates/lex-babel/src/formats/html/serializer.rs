//! HTML serialization (Lex → HTML export)
//!
//! Converts Lex documents to semantic HTML5 with embedded CSS.
//! Pipeline: Lex AST → IR → Events → RcDom → HTML string
//!
//! This file is the façade — options, public entry points, and the IR
//! orchestration. The pipeline is split across siblings:
//! - [`dom_build`] — the IR-event → rcdom walk
//!   (`build_html_dom_with_splice`, `add_inline_to_node`).
//! - [`dom_helpers`] — rcdom node constructors, the DOM → string
//!   serializer, the full-document framing (`wrap_in_document`), and the
//!   string utilities (`normalize_language`, `html_escape`).

mod dom_build;
mod dom_helpers;

use crate::common::nested_to_flat::tree_to_events;
use crate::common::splice::SentinelBuffer;
use crate::error::FormatError;
use crate::formats::html::HtmlTheme;
use crate::ir::nodes::DocNode;
use lex_core::lex::ast::Document;

use dom_build::build_html_dom_with_splice;
use dom_helpers::{serialize_dom, wrap_in_document};

/// Options for HTML serialization
#[derive(Debug, Clone, Default)]
pub struct HtmlOptions {
    /// CSS theme to use
    pub theme: HtmlTheme,
    /// Optional custom CSS to append after the baseline and theme CSS
    pub custom_css: Option<String>,
}

impl HtmlOptions {
    pub fn new(theme: HtmlTheme) -> Self {
        Self {
            theme,
            custom_css: None,
        }
    }

    pub fn with_custom_css(mut self, css: String) -> Self {
        self.custom_css = Some(css);
        self
    }
}

/// Serialize a Lex document to HTML with the given theme
pub fn serialize_to_html(doc: &Document, theme: HtmlTheme) -> Result<String, FormatError> {
    serialize_to_html_with_options(doc, HtmlOptions::new(theme))
}

/// Serialize with an extension-system [`Registry`] in scope: every
/// labelled annotation / verbatim whose schema declares
/// `hooks.render: ["html"]` is dispatched to its handler. Handler
/// diagnostics (errors, format-shape mismatches, namespace disabled)
/// surface in the returned [`HtmlExportOutcome::diagnostics`].
///
/// For annotations whose handler returns `RenderOut::String`, the
/// handler's HTML is spliced into the output in place of the
/// annotation's default rendering (the `<!-- lex:label -->` ...
/// `<!-- /lex:label -->` comment pair plus any content events
/// between them). Handlers returning `RenderOut::WireAst` or
/// `Ok(None)` fall through to the default rendering — wire-AST →
/// HTML conversion is a follow-up.
///
/// Splice mechanism: post-#617 the splice state lives in
/// [`crate::common::splice::SpliceState`]; this entry point just
/// constructs one with the body slice of the plan and threads it
/// through the event walker. Markdown reuses the same helper.
///
/// Phase 3b of #614 made doc-scope annotations IR-only — they don't
/// flow through the event stream. To keep the body splice aligned,
/// this entry point slices `plan.doc_scope_count` entries off the
/// front before handing the plan to the splice walker. Doc-scope
/// handler outputs are therefore unspliced today; their diagnostics
/// still surface in the returned `HtmlExportOutcome`. Routing them
/// back into the rendered HTML is a separate follow-up.
pub fn serialize_to_html_with_registry(
    doc: &Document,
    options: HtmlOptions,
    registry: &lex_extension_host::Registry,
) -> Result<HtmlExportOutcome, FormatError> {
    // Build the IR through the caller's registry so third-party
    // `on_ir_build` hooks (verbatim-label hydration etc.) participate
    // in IR construction. Otherwise the IR `dispatch_render` walks
    // would diverge from what the same registry expects to see.
    let ir_doc = crate::to_ir_with_registry(doc, registry);
    let plan = crate::render_dispatch::dispatch_render(&ir_doc, registry, "html");
    // Phase 3b of #614: skip the doc-scope prefix of the plan when
    // feeding the event-indexed splice walker. The event stream
    // doesn't contain events for `document_annotations`, so doc-scope
    // plan entries would otherwise shift the index and route their
    // handler HTML into the wrong body-annotation slot.
    let body_plan = &plan.nodes[plan.doc_scope_count..];
    let html = serialize_to_html_with_splice_from_ir(ir_doc, options, Some(body_plan))?;
    let mut diagnostics = plan
        .nodes
        .iter()
        .filter_map(|n| n.diagnostic.clone())
        .collect::<Vec<_>>();
    diagnostics.extend(plan.root_diagnostics);
    Ok(HtmlExportOutcome { html, diagnostics })
}

/// Internal helper: serialize with an optional splice plan. When
/// `splice_plan` is `None` this is identical to
/// [`serialize_to_html_with_options`]; when `Some(&plan)` it threads
/// the plan through the DOM builder and the post-process replacement
/// step.
fn serialize_to_html_with_splice(
    doc: &Document,
    options: HtmlOptions,
    splice_plan: Option<&[crate::render_dispatch::RenderedNode]>,
) -> Result<String, FormatError> {
    let ir_doc = crate::to_ir(doc);
    serialize_to_html_with_splice_from_ir(ir_doc, options, splice_plan)
}

/// Splice-aware HTML serialization driven by an already-built IR
/// document. Used by [`serialize_to_html_with_registry`] so the IR
/// isn't built twice — once for dispatch and once for serialization.
fn serialize_to_html_with_splice_from_ir(
    ir_doc: crate::ir::nodes::Document,
    options: HtmlOptions,
    splice_plan: Option<&[crate::render_dispatch::RenderedNode]>,
) -> Result<String, FormatError> {
    let title_text = ir_doc
        .title
        .as_ref()
        .map(|t| crate::ir::to_wire::inlines_to_text(t));
    let subtitle_text = ir_doc
        .subtitle
        .as_ref()
        .map(|s| crate::ir::to_wire::inlines_to_text(s));

    let head_title = match (&title_text, &subtitle_text) {
        (Some(t), Some(s)) => format!("{t}: {s}"),
        (Some(t), None) => t.clone(),
        (None, _) => "Lex Document".to_string(),
    };

    let events = tree_to_events(&DocNode::Document(ir_doc));

    // rcdom has no raw-HTML node, so the event walker plants sentinel
    // comments and `SentinelBuffer::replace` substitutes the
    // handler's raw HTML after `serialize_dom`. Markdown's path
    // doesn't need this — Comrak's `NodeValue::HtmlBlock` carries
    // raw passthrough natively.
    let mut sentinels = SentinelBuffer::new();
    let (dom, has_math) = build_html_dom_with_splice(&events, splice_plan, &mut sentinels)?;

    let html_string = serialize_dom(&dom)?;
    let html_string = sentinels.replace(&html_string);

    wrap_in_document(
        &html_string,
        &head_title,
        title_text.as_deref(),
        subtitle_text.as_deref(),
        has_math,
        &options,
    )
}

/// Result of [`serialize_to_html_with_registry`]: the rendered HTML
/// plus any handler-emitted diagnostic messages (renderer errors,
/// format-shape mismatches, namespace disabled).
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlExportOutcome {
    pub html: String,
    pub diagnostics: Vec<String>,
}

/// Serialize a Lex document to HTML with full options
pub fn serialize_to_html_with_options(
    doc: &Document,
    options: HtmlOptions,
) -> Result<String, FormatError> {
    serialize_to_html_with_splice(doc, options, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::transforms::standard::STRING_TO_AST;

    #[test]
    fn test_simple_paragraph() {
        let lex_src = "This is a simple paragraph.\n";
        let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

        let html = serialize_to_html(&lex_doc, HtmlTheme::Modern).unwrap();

        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<p class=\"lex-paragraph\">"));
        assert!(html.contains("This is a simple paragraph."));
    }

    #[test]
    fn test_heading() {
        let lex_src = "1. Introduction\n\n    Content here.\n";
        let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

        let html = serialize_to_html(&lex_doc, HtmlTheme::Modern).unwrap();

        assert!(html.contains("<section class=\"lex-session lex-session-2\">"));
        assert!(html.contains("<h2>"));
        assert!(html.contains("Introduction"));
    }

    #[test]
    fn test_css_embedded() {
        let lex_src = "Test document.\n";
        let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

        let html = serialize_to_html(&lex_doc, HtmlTheme::Modern).unwrap();

        assert!(html.contains("<style>"));
        assert!(html.contains(".lex-document"));
        assert!(html.contains("Helvetica")); // Modern theme uses Helvetica font
    }

    #[test]
    fn test_fancy_serif_theme() {
        let lex_src = "Test document.\n";
        let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

        let html = serialize_to_html(&lex_doc, HtmlTheme::FancySerif).unwrap();

        assert!(html.contains("Cormorant")); // Fancy serif theme uses Cormorant font
    }

    #[test]
    fn test_custom_css_appended() {
        let lex_src = "Test document.\n";
        let lex_doc = STRING_TO_AST.run(lex_src.to_string()).unwrap();

        let custom_css = ".my-custom-class { color: red; }";
        let options = HtmlOptions::new(HtmlTheme::Modern).with_custom_css(custom_css.to_string());
        let html = serialize_to_html_with_options(&lex_doc, options).unwrap();

        // Custom CSS should be present
        assert!(html.contains(".my-custom-class { color: red; }"));
        // Baseline CSS should still be present
        assert!(html.contains(".lex-document"));
    }

    #[test]
    fn test_html_options_default() {
        let options = HtmlOptions::default();
        assert_eq!(options.theme, HtmlTheme::Modern);
        assert!(options.custom_css.is_none());
    }
}
