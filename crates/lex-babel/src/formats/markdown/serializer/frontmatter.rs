//! YAML frontmatter synthesis and title rendering for Markdown export.
//!
//! Splits out the document-level concerns of the markdown serializer:
//! synthesizing a `---\n…\n---` YAML preamble from the IR's
//! `document_annotations`, prepending the document title as an H1, and
//! the small text-flattening helpers those two steps share. The body of
//! the document (the Comrak AST build) lives in
//! [`super::comrak_build`]; this module owns everything that frames it.

use crate::ir::nodes::{Annotation as IrAnnotation, DocNode, InlineContent};
use crate::render_dispatch::RenderedNode;

/// Build the YAML frontmatter block (`---\n…\n---\n\n`) from an IR
/// document's `document_annotations`, or `None` if the slot would
/// produce an empty block.
///
/// `doc_scope_plan` is the doc-scope prefix of the registry's render
/// plan (the first `plan.doc_scope_count` entries). Annotations whose
/// handler returned a `RenderOut::String` consume the next plan entry
/// and emit the handler's text as the YAML line — this is how the
/// `doc.*` namespace (registered via Sub B) routes through the
/// markdown pipeline. Annotations without a registered handler — most
/// notably the `lex.metadata.*` shortcut family — fall back to the
/// legacy in-place synthesis that mirrors the key-flattening logic the
/// retired `emit_frontmatter_event` used: `lex.metadata.<key>` →
/// `<key>`; annotations with a paragraph body produce `<key>: <body>`;
/// annotations with structured params produce `<key>.<sub>: <value>`;
/// marker-form annotations contribute nothing.
pub(crate) fn render_document_annotations_as_yaml(
    annotations: &[IrAnnotation],
    doc_scope_plan: &[RenderedNode],
) -> Option<String> {
    if annotations.is_empty() {
        return None;
    }
    let mut lines = String::new();
    let mut plan_iter = doc_scope_plan.iter().peekable();
    for ann in annotations {
        // Lockstep with the plan: `dispatch_render` visits
        // `document_annotations` in order and emits one plan entry
        // per registered annotation, so the next plan entry whose
        // label matches this annotation belongs to it.
        let plan_entry = match plan_iter.peek() {
            Some(entry) if entry.label == ann.label => plan_iter.next(),
            _ => None,
        };
        if let Some(entry) = plan_entry {
            if let Some(out) = &entry.output {
                lines.push_str(out);
                continue;
            }
            // Registered but no output (handler returned None or
            // errored) — fall through to legacy synthesis.
        }
        synthesize_yaml_line_from_annotation(&mut lines, ann);
    }
    if lines.is_empty() {
        return None;
    }
    Some(format!("---\n{lines}---\n\n"))
}

/// Legacy in-place synthesis: append one or more YAML lines for `ann`
/// to `lines`. Used for annotations whose label isn't registered with a
/// markdown render handler — primarily the `lex.metadata.*` shortcut
/// family preserved while the shortcut table still maps `:: title ::`
/// onto `lex.metadata.title` rather than `doc.title`.
fn synthesize_yaml_line_from_annotation(lines: &mut String, ann: &IrAnnotation) {
    let key = ann
        .label
        .strip_prefix("lex.metadata.")
        .unwrap_or(ann.label.as_str())
        .to_string();
    // Trim whitespace lex picks up after the closing `::` separator
    // (e.g. `:: title :: My Doc` produces a paragraph whose first
    // inline is `" My Doc"`). Collapse internal newlines to spaces —
    // multi-line annotation bodies emit `InlineContent::Text("\n")`
    // between lines (see `from_lex_paragraph`), and a literal `\n`
    // inside a YAML scalar would orphan the trailing lines from the
    // key.
    let body_text = flatten_annotation_body_text(&ann.content)
        .replace('\n', " ")
        .trim()
        .to_string();
    if !body_text.is_empty() {
        lines.push_str(&format!("{key}: {body_text}\n"));
    } else if !ann.parameters.is_empty() {
        for (k, v) in &ann.parameters {
            lines.push_str(&format!("{key}.{k}: {v}\n"));
        }
    }
}

/// Flatten the text content of an annotation's paragraph children
/// into a single string. Used for YAML frontmatter synthesis. Covers
/// every inline shape that can legitimately appear inside metadata
/// bodies — `Text`, `Code`, `Math`, `Reference`, `Link` (regression
/// coverage from #596 / #597). Bold / Italic / Image are skipped on
/// purpose: YAML values are leaf strings, not rich content.
fn flatten_annotation_body_text(content: &[DocNode]) -> String {
    let mut text = String::new();
    for child in content {
        if let DocNode::Paragraph(p) = child {
            for inline in &p.content {
                match inline {
                    InlineContent::Text(t) | InlineContent::Code(t) | InlineContent::Math(t) => {
                        text.push_str(t)
                    }
                    InlineContent::Reference { raw, .. } => text.push_str(raw),
                    InlineContent::Link { text: t, .. } => text.push_str(t),
                    _ => {}
                }
            }
        }
    }
    text
}

/// Prepend document title as an H1 heading, optionally followed by subtitle as H2
///
/// If the document has a title, prepend `# Title` at the beginning.
/// If it also has a subtitle, append `## Subtitle` below.
pub(crate) fn prepend_title_as_h1(
    markdown: &str,
    title: Option<(String, Option<String>)>,
) -> String {
    match title {
        Some((t, Some(sub))) => format!("# {t}\n\n## {sub}\n\n{markdown}"),
        Some((t, None)) => format!("# {t}\n\n{markdown}"),
        None => markdown.to_string(),
    }
}

/// Convert IR inline content to plain text for title rendering
pub(crate) fn inlines_to_text(content: &[InlineContent]) -> String {
    content
        .iter()
        .map(|inline| match inline {
            InlineContent::Text(t) => t.clone(),
            InlineContent::Bold(c) => inlines_to_text(c),
            InlineContent::Italic(c) => inlines_to_text(c),
            InlineContent::Code(c) => c.clone(),
            InlineContent::Math(m) => m.clone(),
            InlineContent::Reference { raw, .. } => raw.clone(),
            InlineContent::Link { text, .. } => text.clone(),
            InlineContent::Image(img) => img.alt.clone(),
        })
        .collect()
}

/// Strip Comrak's `\.` escape after a leading digit run inside an ATX
/// heading. Comrak escapes `^(#+ \d+)\.` → `^(#+ \d+)\\.` to keep
/// pure CommonMark renderers from misreading the line as an ordered list,
/// but a `#`-prefixed line can't open a list, so the escape is just noise.
/// Applies per line, headings only — paragraph-leading `1\.` is left
/// alone since Comrak's protection there is meaningful (#606).
pub(crate) fn strip_heading_digit_dot_escape(markdown: &str) -> String {
    static HEADING_DIGIT_DOT: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| {
            regex::Regex::new(r"(?m)^(#+ \d+(?:\.\d+)*)\\\.").expect("compile heading digit-dot")
        });
    HEADING_DIGIT_DOT.replace_all(markdown, "$1.").into_owned()
}
