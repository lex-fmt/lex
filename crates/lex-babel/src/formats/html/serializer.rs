//! HTML serialization (Lex → HTML export)
//!
//! Converts Lex documents to semantic HTML5 with embedded CSS.
//! Pipeline: Lex AST → IR → Events → RcDom → HTML string

use crate::common::nested_to_flat::tree_to_events;
use crate::error::FormatError;
use crate::formats::html::HtmlTheme;
use crate::ir::events::Event;
use crate::ir::nodes::{DocNode, InlineContent, TableCellAlignment};
use html5ever::{
    ns, serialize, serialize::SerializeOpts, serialize::TraversalScope, Attribute, LocalName,
    QualName,
};
use lex_core::lex::ast::Document;
use markup5ever_rcdom::{Handle, Node, NodeData, RcDom, SerializableHandle};
use std::cell::{Cell, RefCell};
use std::default::Default;
use std::rc::Rc;

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
/// Splice mechanism: the AST walker (`dispatch_render`) and the
/// event walker (`tree_to_events`) visit annotations in matching
/// document order, so the HTML builder maintains a counter and
/// indexes into the plan as it sees `Event::StartAnnotation`. When
/// the plan entry has output, the builder emits a sentinel comment
/// (`<!--LEX-RENDER-SPLICE:N-->`) and skips events until the
/// matching `EndAnnotation`; after DOM serialization, the sentinel
/// is string-replaced with the handler's raw HTML. The skip-state
/// nests on depth so handlers consume their full subtree (including
/// any inner labelled annotations — those handlers fired during the
/// dispatch walk; their results aren't separately spliced because
/// the outer handler owns the body's rendering).
pub fn serialize_to_html_with_registry(
    doc: &Document,
    options: HtmlOptions,
    registry: &lex_extension_host::Registry,
) -> Result<HtmlExportOutcome, FormatError> {
    let plan = crate::render_dispatch::dispatch_render(doc, registry, "html");
    let html = serialize_to_html_with_splice(doc, options, Some(&plan.nodes))?;
    let mut diagnostics = plan
        .nodes
        .iter()
        .filter_map(|n| n.diagnostic.clone())
        .collect::<Vec<_>>();
    diagnostics.extend(plan.root_diagnostics);
    Ok(HtmlExportOutcome { html, diagnostics })
}

/// Sentinel comment marker emitted by [`build_html_dom`] when a
/// render-plan entry has output. After DOM serialization, every
/// occurrence of this comment (with a numeric ID appended) is
/// replaced with the raw HTML at the corresponding index in the
/// splice-output buffer.
///
/// Comments are used as sentinels because the html5ever DOM doesn't
/// have a "raw HTML" text-node concept — every text node is HTML-
/// escaped at serialization. Comments serialize byte-for-byte
/// (modulo whitespace inside them), so a unique sentinel is reliably
/// round-trip-able through the DOM → string pass.
const SPLICE_SENTINEL_PREFIX: &str = "LEX-RENDER-SPLICE:";

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

    let title_text = ir_doc.title.as_ref().map(|t| ir_inline_to_text(t));
    let subtitle_text = ir_doc.subtitle.as_ref().map(|s| ir_inline_to_text(s));

    let head_title = match (&title_text, &subtitle_text) {
        (Some(t), Some(s)) => format!("{t}: {s}"),
        (Some(t), None) => t.clone(),
        (None, _) => "Lex Document".to_string(),
    };

    let events = tree_to_events(&DocNode::Document(ir_doc));

    // Splice outputs are collected during the DOM build so the
    // post-process step has them keyed by the sentinel index.
    let mut splice_outputs: Vec<String> = Vec::new();
    let (dom, has_math) = build_html_dom_with_splice(&events, splice_plan, &mut splice_outputs)?;

    let html_string = serialize_dom(&dom)?;
    // Replace each sentinel comment with the handler's raw HTML.
    // We do the substitution on the inner-body string before the
    // wrap_in_document call so the wrap doesn't have to know about
    // splicing.
    let html_string = replace_splice_sentinels(&html_string, &splice_outputs);

    wrap_in_document(
        &html_string,
        &head_title,
        title_text.as_deref(),
        subtitle_text.as_deref(),
        has_math,
        &options,
    )
}

/// Replace every `<!--LEX-RENDER-SPLICE:N-->` sentinel in `html`
/// with the raw HTML at `outputs[N]`. Tolerates trailing whitespace
/// in the comment (html5ever doesn't trim, but markup5ever_rcdom
/// can normalize on serialize). Non-numeric or out-of-range indices
/// are left in place — that's a programming bug that would surface
/// as a visible sentinel in the output rather than silent corruption.
fn replace_splice_sentinels(html: &str, outputs: &[String]) -> String {
    if outputs.is_empty() {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len());
    let mut remaining = html;
    let pattern_open = format!("<!--{SPLICE_SENTINEL_PREFIX}");
    while let Some(start) = remaining.find(&pattern_open) {
        out.push_str(&remaining[..start]);
        let after_prefix = &remaining[start + pattern_open.len()..];
        // The ID is decimal digits until `-->`.
        let Some(end_marker) = after_prefix.find("-->") else {
            // Malformed sentinel — copy through and continue.
            out.push_str(&remaining[start..]);
            remaining = "";
            break;
        };
        let id_str = after_prefix[..end_marker].trim();
        match id_str.parse::<usize>() {
            Ok(idx) if idx < outputs.len() => {
                out.push_str(&outputs[idx]);
            }
            _ => {
                // Out-of-range or non-numeric — leave the sentinel
                // visible. Surfaces as a noticeable bug in the
                // rendered output, easier to spot than silent drop.
                out.push_str(&remaining[start..start + pattern_open.len() + end_marker + 3]);
            }
        }
        remaining = &after_prefix[end_marker + 3..];
    }
    out.push_str(remaining);
    out
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

/// Build an HTML DOM tree from IR events, optionally splicing
/// handler-rendered HTML in place of default annotation rendering.
///
/// When `splice_plan` is `Some(&plan)`, the builder maintains an
/// annotation counter that advances on every `Event::StartAnnotation`
/// (mirroring the dispatch walker's order). On entries with output,
/// it appends a sentinel comment to the DOM, pushes the output into
/// `splice_outputs`, and enters a skip-state for content events
/// until the matching `EndAnnotation`. The skip-state nests on
/// depth so a handled outer annotation's body — including any
/// inner labelled annotations — is consumed entirely. The
/// post-process step in [`serialize_to_html_with_splice`] replaces
/// each sentinel with the corresponding raw HTML.
///
/// When `splice_plan` is `None`, the builder behaves exactly as
/// the original `build_html_dom` did before the splice landing.
/// Build the HTML DOM from a flat event stream.
///
/// Returns the constructed DOM and a `has_math` flag indicating whether any
/// `InlineContent::Math` was encountered during the walk. The caller uses
/// the flag to decide whether to inject the KaTeX renderer into the document
/// head — tracking it here is more reliable than scanning the serialized
/// HTML for the math class string, which can false-positive when a verbatim
/// code block happens to contain that text.
fn build_html_dom_with_splice(
    events: &[Event],
    splice_plan: Option<&[crate::render_dispatch::RenderedNode]>,
    splice_outputs: &mut Vec<String>,
) -> Result<(RcDom, bool), FormatError> {
    let dom = RcDom::default();

    // Create document container
    let doc_container = create_element("div", vec![("class", "lex-document")]);

    let mut current_parent: Handle = doc_container.clone();
    let mut parent_stack: Vec<Handle> = vec![];

    // State for collecting verbatim content
    let mut in_verbatim = false;
    let mut verbatim_language: Option<String> = None;
    let mut verbatim_content = String::new();

    // State for heading context
    let mut current_heading: Option<Handle> = None;

    // Splice state. `annotation_idx` advances on every
    // StartAnnotation event regardless of skip-state, so it stays
    // aligned with the dispatch walker's plan order (both walks
    // emit annotations in matching document order — see
    // `serialize_to_html_with_registry`'s docs for the contract).
    // `splice_skip_depth` is non-zero while we're inside a
    // handler-consumed annotation; events arriving in that window
    // are suppressed so the handler's output replaces them entirely.
    let mut annotation_idx: usize = 0;
    let mut splice_skip_depth: usize = 0;

    // Consecutive Definition siblings share one `<dl>`. After `EndDefinition`
    // we leave `current_parent` pointing at the open `<dl>` and set this
    // flag; the next event either reuses the dl (another Definition) or
    // closes it (anything else).
    let mut defer_close_dl: bool = false;

    // Set by `add_inline_to_node` whenever it processes an `InlineContent::Math`.
    // The outer serializer consults this to decide whether to inject KaTeX into
    // the document head; tracking it during construction avoids the fragility
    // of a substring scan over the serialized HTML (which can false-positive on
    // verbatim code blocks that happen to contain `class="lex-math"`).
    let mut has_math: bool = false;

    for event in events {
        // Inside a splice skip region, only Start/EndAnnotation
        // events are inspected — for nesting depth and counter
        // bookkeeping. All other events are suppressed so the
        // handler's output stands alone.
        if splice_skip_depth > 0
            && !matches!(
                event,
                Event::StartAnnotation { .. } | Event::EndAnnotation { .. }
            )
        {
            continue;
        }
        // If we have a `<dl>` waiting to be closed and the next event
        // isn't another Definition, close it now before handling the event.
        if defer_close_dl && !matches!(event, Event::StartDefinition) {
            current_parent = parent_stack.pop().ok_or_else(|| {
                FormatError::SerializationError(
                    "Failed to close deferred definition list".to_string(),
                )
            })?;
            defer_close_dl = false;
        }
        match event {
            Event::StartDocument => {
                // Already created doc_container
            }

            Event::EndDocument => {
                // Done
            }

            Event::StartHeading(level) => {
                // Create section wrapper for this session
                let class = format!("lex-session lex-session-{level}");
                let section = create_element("section", vec![("class", &class)]);
                current_parent.children.borrow_mut().push(section.clone());
                parent_stack.push(current_parent.clone());
                current_parent = section;

                // Create heading element (h1-h6, max at h6)
                // For levels > 6, add class attribute to preserve true depth
                let clamped = (*level as u8).min(6);
                let heading_tag = format!("h{clamped}");
                let heading = if *level > 6 {
                    let class = format!("lex-level-{level}");
                    create_element(&heading_tag, vec![("class", &class)])
                } else {
                    create_element(&heading_tag, vec![])
                };
                current_parent.children.borrow_mut().push(heading.clone());
                current_heading = Some(heading);
            }

            Event::EndHeading(_) => {
                current_heading = None;
                // Close section
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced heading end".to_string())
                })?;
            }

            Event::StartContent => {
                // Create content wrapper (mirrors AST container structure for indentation)
                current_heading = None;
                let content = create_element("div", vec![("class", "lex-content")]);
                current_parent.children.borrow_mut().push(content.clone());
                parent_stack.push(current_parent.clone());
                current_parent = content;
            }

            Event::EndContent => {
                // Close content wrapper
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced content end".to_string())
                })?;
            }

            Event::StartParagraph => {
                current_heading = None;
                let para = create_element("p", vec![("class", "lex-paragraph")]);
                current_parent.children.borrow_mut().push(para.clone());
                parent_stack.push(current_parent.clone());
                current_parent = para;
            }

            Event::EndParagraph => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced paragraph end".to_string())
                })?;
            }

            Event::StartList { ordered, style, .. } => {
                current_heading = None;
                let tag = if *ordered { "ol" } else { "ul" };
                // For ordered lists, set the HTML type attribute to preserve decoration style
                let list = match style {
                    crate::ir::nodes::ListStyle::AlphaLower => {
                        create_element(tag, vec![("class", "lex-list"), ("type", "a")])
                    }
                    crate::ir::nodes::ListStyle::AlphaUpper => {
                        create_element(tag, vec![("class", "lex-list"), ("type", "A")])
                    }
                    crate::ir::nodes::ListStyle::RomanLower => {
                        create_element(tag, vec![("class", "lex-list"), ("type", "i")])
                    }
                    crate::ir::nodes::ListStyle::RomanUpper => {
                        create_element(tag, vec![("class", "lex-list"), ("type", "I")])
                    }
                    _ => create_element(tag, vec![("class", "lex-list")]),
                };
                current_parent.children.borrow_mut().push(list.clone());
                parent_stack.push(current_parent.clone());
                current_parent = list;
            }

            Event::EndList => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced list end".to_string())
                })?;
            }

            Event::StartListItem => {
                current_heading = None;
                let item = create_element("li", vec![("class", "lex-list-item")]);
                current_parent.children.borrow_mut().push(item.clone());
                parent_stack.push(current_parent.clone());
                current_parent = item;
            }

            Event::EndListItem => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced list item end".to_string())
                })?;
            }

            Event::StartVerbatim { language, subject } => {
                current_heading = None;
                in_verbatim = true;
                verbatim_language = language.clone();
                verbatim_content.clear();

                // Render subject as a caption before the code block
                if let Some(subj) = subject {
                    let caption = create_element("div", vec![("class", "lex-verbatim-subject")]);
                    let text = create_text(subj);
                    caption.children.borrow_mut().push(text);
                    current_parent.children.borrow_mut().push(caption);
                }
            }

            Event::EndVerbatim => {
                // Check for special metadata comment format
                if let Some(ref lang) = verbatim_language {
                    if let Some(label) = lang.strip_prefix("lex-metadata:") {
                        // Render as comment
                        let comment_text = format!(" lex:{label}{verbatim_content}");
                        let comment_node = create_comment(&comment_text);
                        current_parent.children.borrow_mut().push(comment_node);

                        in_verbatim = false;
                        verbatim_language = None;
                        verbatim_content.clear();
                        continue; // Skip normal verbatim handling
                    }
                }

                // Create pre + code block with highlight.js-compatible classes
                let normalized_lang;
                let mut pre_attrs = vec![("class", "lex-verbatim")];
                let lang_string;
                if let Some(ref lang) = verbatim_language {
                    lang_string = lang.clone();
                    pre_attrs.push(("data-language", &lang_string));
                    normalized_lang = Some(format!("language-{}", normalize_language(lang)));
                } else {
                    normalized_lang = None;
                }

                let pre = create_element("pre", pre_attrs);
                let code_attrs = match normalized_lang {
                    Some(ref class) => vec![("class", class.as_str())],
                    None => vec![],
                };
                let code = create_element("code", code_attrs);
                let text = create_text(&verbatim_content);
                code.children.borrow_mut().push(text);
                pre.children.borrow_mut().push(code);
                current_parent.children.borrow_mut().push(pre);

                in_verbatim = false;
                verbatim_language = None;
                verbatim_content.clear();
            }

            Event::StartDefinition => {
                current_heading = None;
                if defer_close_dl {
                    // Previous Definition just ended at the same level; keep
                    // using its `<dl>` so sibling defs share one container.
                    defer_close_dl = false;
                } else {
                    let dl = create_element("dl", vec![("class", "lex-definition")]);
                    current_parent.children.borrow_mut().push(dl.clone());
                    parent_stack.push(current_parent.clone());
                    current_parent = dl;
                }
            }

            Event::EndDefinition => {
                // Defer the `<dl>` close so that an immediately-following
                // sibling Definition can reuse this container.
                defer_close_dl = true;
            }

            Event::StartDefinitionTerm => {
                let dt = create_element("dt", vec![]);
                current_parent.children.borrow_mut().push(dt.clone());
                parent_stack.push(current_parent.clone());
                current_parent = dt;
            }

            Event::EndDefinitionTerm => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced definition term end".to_string())
                })?;
            }

            Event::StartDefinitionDescription => {
                let dd = create_element("dd", vec![]);
                current_parent.children.borrow_mut().push(dd.clone());
                parent_stack.push(current_parent.clone());
                current_parent = dd;
            }

            Event::EndDefinitionDescription => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError(
                        "Unbalanced definition description end".to_string(),
                    )
                })?;
            }

            Event::StartTable { caption, fullwidth } => {
                current_heading = None;
                let mut table_attrs = vec![("class", "lex-table")];
                let fullwidth_class;
                if *fullwidth {
                    fullwidth_class = "lex-table lex-table-fullwidth".to_string();
                    table_attrs = vec![("class", &fullwidth_class)];
                }
                let table = create_element("table", table_attrs);

                // Render caption if present
                if let Some(caption_inlines) = caption {
                    let caption_el = create_element("caption", vec![]);
                    for inline in caption_inlines {
                        add_inline_to_node(&caption_el, inline, &mut has_math)?;
                    }
                    table.children.borrow_mut().push(caption_el);
                }

                current_parent.children.borrow_mut().push(table.clone());
                parent_stack.push(current_parent.clone());
                current_parent = table;
            }

            Event::EndTable => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced table end".to_string())
                })?;
            }

            Event::StartTableFootnotes => {
                let footer = create_element("tfoot", vec![("class", "lex-table-footnotes")]);
                current_parent.children.borrow_mut().push(footer.clone());
                parent_stack.push(current_parent.clone());
                current_parent = footer;
            }

            Event::EndTableFootnotes => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced table footnotes end".to_string())
                })?;
            }

            Event::StartTableRow { header: _ } => {
                let tr = create_element("tr", vec![]);
                current_parent.children.borrow_mut().push(tr.clone());
                parent_stack.push(current_parent.clone());
                current_parent = tr;
            }

            Event::EndTableRow => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced table row end".to_string())
                })?;
            }

            Event::StartTableCell {
                header,
                align,
                colspan,
                rowspan,
            } => {
                let tag = if *header { "th" } else { "td" };
                let mut attrs: Vec<(&str, String)> = vec![];
                match align {
                    TableCellAlignment::Left => {
                        attrs.push(("style", "text-align: left".to_string()))
                    }
                    TableCellAlignment::Right => {
                        attrs.push(("style", "text-align: right".to_string()))
                    }
                    TableCellAlignment::Center => {
                        attrs.push(("style", "text-align: center".to_string()))
                    }
                    TableCellAlignment::None => {}
                }
                if *colspan > 1 {
                    attrs.push(("colspan", colspan.to_string()));
                }
                if *rowspan > 1 {
                    attrs.push(("rowspan", rowspan.to_string()));
                }

                let str_attrs: Vec<(&str, &str)> =
                    attrs.iter().map(|(k, v)| (*k, v.as_str())).collect();
                let cell = create_element(tag, str_attrs);
                current_parent.children.borrow_mut().push(cell.clone());
                parent_stack.push(current_parent.clone());
                current_parent = cell;
            }

            Event::EndTableCell => {
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced table cell end".to_string())
                })?;
            }

            Event::Inline(inline_content) => {
                if in_verbatim {
                    // Accumulate verbatim content
                    if let InlineContent::Text(text) = inline_content {
                        verbatim_content.push_str(text);
                    }
                } else if let Some(ref heading) = current_heading {
                    // Add to heading
                    add_inline_to_node(heading, inline_content, &mut has_math)?;
                } else {
                    // Add to current parent
                    add_inline_to_node(&current_parent, inline_content, &mut has_math)?;
                }
            }

            Event::StartAnnotation {
                label, parameters, ..
            } => {
                current_heading = None;
                // The annotation counter advances on every Start
                // regardless of skip-state. Nested annotations
                // inside a handler-consumed outer still advance the
                // counter (so the next non-skipped annotation
                // indexes correctly into the plan); they also bump
                // skip-depth so we find the matching outer End.
                let this_idx = annotation_idx;
                annotation_idx += 1;

                if splice_skip_depth > 0 {
                    splice_skip_depth += 1;
                    continue;
                }

                // Check the plan: does this annotation have a
                // handler-rendered output ready to splice? We match
                // on label too as a sanity check — if the plan's
                // entry doesn't agree on the label, the walks have
                // diverged and we fall through to default rendering
                // rather than splice the wrong output.
                let splice_target = splice_plan
                    .and_then(|plan| plan.get(this_idx))
                    .filter(|entry| entry.label == *label)
                    .and_then(|entry| entry.output.as_ref());

                if let Some(rendered_html) = splice_target {
                    let sentinel_idx = splice_outputs.len();
                    splice_outputs.push(rendered_html.clone());
                    let sentinel = format!("{SPLICE_SENTINEL_PREFIX}{sentinel_idx}");
                    let comment_node = create_comment(&sentinel);
                    current_parent.children.borrow_mut().push(comment_node);
                    splice_skip_depth = 1;
                } else {
                    // No splice — emit the default lex:label start
                    // comment as before.
                    let mut comment = format!(" lex:{label}");
                    for (key, value) in parameters {
                        comment.push_str(&format!(" {key}={value}"));
                    }
                    comment.push(' ');
                    let comment_node = create_comment(&comment);
                    current_parent.children.borrow_mut().push(comment_node);
                }
            }

            Event::EndAnnotation { label } => {
                if splice_skip_depth > 0 {
                    splice_skip_depth -= 1;
                    continue;
                }
                // Not in splice — emit the default lex:label end
                // comment as before.
                let comment = format!(" /lex:{label} ");
                let comment_node = create_comment(&comment);
                current_parent.children.borrow_mut().push(comment_node);
            }

            Event::Image(image) => {
                let figure = create_element("figure", vec![("class", "lex-image")]);
                current_parent.children.borrow_mut().push(figure.clone());

                let mut attrs = vec![("src", image.src.as_str()), ("alt", image.alt.as_str())];
                if let Some(title) = &image.title {
                    attrs.push(("title", title.as_str()));
                }
                let img = create_element("img", attrs);
                figure.children.borrow_mut().push(img);

                if !image.alt.is_empty() {
                    let caption = create_element("figcaption", vec![]);
                    let text = create_text(&image.alt);
                    caption.children.borrow_mut().push(text);
                    figure.children.borrow_mut().push(caption);
                }
            }

            Event::Video(video) => {
                let figure = create_element("figure", vec![("class", "lex-video")]);
                current_parent.children.borrow_mut().push(figure.clone());

                let mut attrs = vec![("src", video.src.as_str()), ("controls", "")];
                if let Some(poster) = &video.poster {
                    attrs.push(("poster", poster.as_str()));
                }
                if let Some(title) = &video.title {
                    attrs.push(("title", title.as_str()));
                }
                let vid = create_element("video", attrs);
                figure.children.borrow_mut().push(vid);
            }

            Event::Audio(audio) => {
                let figure = create_element("figure", vec![("class", "lex-audio")]);
                current_parent.children.borrow_mut().push(figure.clone());

                let mut attrs = vec![("src", audio.src.as_str()), ("controls", "")];
                if let Some(title) = &audio.title {
                    attrs.push(("title", title.as_str()));
                }
                let aud = create_element("audio", attrs);
                figure.children.borrow_mut().push(aud);
            }
        }
    }

    // Close any `<dl>` that was deferred at end-of-events.
    if defer_close_dl {
        current_parent = parent_stack.pop().ok_or_else(|| {
            FormatError::SerializationError(
                "Failed to close deferred definition list at end of events".to_string(),
            )
        })?;
    }
    let _ = current_parent;

    // Set the document container as the root
    dom.document.children.borrow_mut().push(doc_container);

    Ok((dom, has_math))
}

/// Add inline content to an HTML node, handling references → anchors conversion.
///
/// `has_math` is set to true if any `InlineContent::Math` is encountered
/// (transitively, through nested Bold/Italic/Link children). The caller
/// uses this to decide whether to inject KaTeX into the document head.
fn add_inline_to_node(
    parent: &Handle,
    inline: &InlineContent,
    has_math: &mut bool,
) -> Result<(), FormatError> {
    match inline {
        InlineContent::Text(text) => {
            let text_node = create_text(text);
            parent.children.borrow_mut().push(text_node);
        }

        InlineContent::Bold(children) => {
            let strong = create_element("strong", vec![]);
            parent.children.borrow_mut().push(strong.clone());
            for child in children {
                add_inline_to_node(&strong, child, has_math)?;
            }
        }

        InlineContent::Italic(children) => {
            let em = create_element("em", vec![]);
            parent.children.borrow_mut().push(em.clone());
            for child in children {
                add_inline_to_node(&em, child, has_math)?;
            }
        }

        InlineContent::Code(code_text) => {
            let code = create_element("code", vec![]);
            let text = create_text(code_text);
            code.children.borrow_mut().push(text);
            parent.children.borrow_mut().push(code);
        }

        InlineContent::Math(math_text) => {
            *has_math = true;
            let math_span = create_element("span", vec![("class", "lex-math")]);
            let dollar_open = create_text("$");
            let math_content = create_text(math_text);
            let dollar_close = create_text("$");
            math_span.children.borrow_mut().push(dollar_open);
            math_span.children.borrow_mut().push(math_content);
            math_span.children.borrow_mut().push(dollar_close);
            parent.children.borrow_mut().push(math_span);
        }

        InlineContent::Reference(ref_text) => {
            // Unresolved reference (non-linkable types like citations, footnotes, etc.)
            // Handle citations (@...) by targeting a reference ID
            let href = if let Some(citation) = ref_text.strip_prefix('@') {
                format!("#ref-{citation}")
            } else {
                ref_text.to_string()
            };

            let anchor = create_element("a", vec![("href", &href)]);
            let anchor_text = create_text(ref_text);
            anchor.children.borrow_mut().push(anchor_text);
            parent.children.borrow_mut().push(anchor);
        }

        InlineContent::Link { text, href } => {
            let anchor = create_element("a", vec![("href", href)]);
            let anchor_text = create_text(text);
            anchor.children.borrow_mut().push(anchor_text);
            parent.children.borrow_mut().push(anchor);
        }

        InlineContent::Image(image) => {
            let mut attrs = vec![("src", image.src.as_str()), ("alt", image.alt.as_str())];
            if let Some(title) = &image.title {
                attrs.push(("title", title.as_str()));
            }
            let img = create_element("img", attrs);
            parent.children.borrow_mut().push(img);
        }
    }

    Ok(())
}

/// Create an HTML element with attributes
fn create_element(tag: &str, attrs: Vec<(&str, &str)>) -> Handle {
    let qual_name = QualName::new(None, ns!(html), LocalName::from(tag));
    let attributes = attrs
        .into_iter()
        .map(|(name, value)| Attribute {
            name: QualName::new(None, ns!(), LocalName::from(name)),
            value: value.to_string().into(),
        })
        .collect();

    Rc::new(Node {
        parent: Cell::new(None),
        children: RefCell::new(Vec::new()),
        data: NodeData::Element {
            name: qual_name,
            attrs: RefCell::new(attributes),
            template_contents: Default::default(),
            mathml_annotation_xml_integration_point: false,
        },
    })
}

/// Create a text node
fn create_text(text: &str) -> Handle {
    Rc::new(Node {
        parent: Cell::new(None),
        children: RefCell::new(Vec::new()),
        data: NodeData::Text {
            contents: RefCell::new(text.to_string().into()),
        },
    })
}

/// Create a comment node
fn create_comment(text: &str) -> Handle {
    Rc::new(Node {
        parent: Cell::new(None),
        children: RefCell::new(Vec::new()),
        data: NodeData::Comment {
            contents: text.to_string().into(),
        },
    })
}

/// Serialize the DOM to an HTML string (just the inner content)
fn serialize_dom(dom: &RcDom) -> Result<String, FormatError> {
    let mut output = Vec::new();

    // Get the document container (first child of document root)
    let doc_container = dom
        .document
        .children
        .borrow()
        .first()
        .ok_or_else(|| FormatError::SerializationError("Empty document".to_string()))?
        .clone();

    // Serialize each child of the doc_container
    // Use TraversalScope::IncludeNode to serialize the element AND its children
    let opts = SerializeOpts {
        traversal_scope: TraversalScope::IncludeNode,
        ..Default::default()
    };

    for child in doc_container.children.borrow().iter() {
        let serializable = SerializableHandle::from(child.clone());
        serialize(&mut output, &serializable, opts.clone()).map_err(|e| {
            FormatError::SerializationError(format!("HTML serialization failed: {e}"))
        })?;
    }

    String::from_utf8(output)
        .map_err(|e| FormatError::SerializationError(format!("UTF-8 conversion failed: {e}")))
}

/// Wrap the content in a complete HTML document with embedded CSS
fn wrap_in_document(
    body_html: &str,
    head_title: &str,
    body_title: Option<&str>,
    body_subtitle: Option<&str>,
    has_math: bool,
    options: &HtmlOptions,
) -> Result<String, FormatError> {
    let baseline_css = include_str!("../../../css/baseline.css");
    let theme_css = match options.theme {
        HtmlTheme::FancySerif => include_str!("../../../css/themes/theme-fancy-serif.css"),
        HtmlTheme::Modern => include_str!("../../../css/themes/theme-modern.css"),
    };

    // Custom CSS is appended after baseline and theme
    let custom_css = options.custom_css.as_deref().unwrap_or("");

    let escaped_head_title = html_escape(head_title);

    let header_html = match body_title {
        Some(t) => {
            let escaped_t = html_escape(t);
            match body_subtitle {
                Some(s) => format!(
                    "<header class=\"lex-doc-header\"><h1 class=\"lex-doc-title\">{escaped_t}</h1><p class=\"lex-doc-subtitle\">{}</p></header>\n",
                    html_escape(s)
                ),
                None => format!(
                    "<header class=\"lex-doc-header\"><h1 class=\"lex-doc-title\">{escaped_t}</h1></header>\n"
                ),
            }
        }
        None => String::new(),
    };

    // KaTeX is only included when the document contains math spans — saves
    // ~290 KB on the wire for math-free documents.
    let katex_html = if has_math {
        r#"  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css" crossorigin="anonymous">
  <script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js" crossorigin="anonymous"></script>
  <script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js" crossorigin="anonymous" onload="renderMathInElement(document.body, {delimiters: [{left: '$', right: '$', display: false}, {left: '$$', right: '$$', display: true}], throwOnError: false});"></script>
"#
    } else {
        ""
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta name="generator" content="lex-babel">
  <title>{escaped_head_title}</title>
  <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/styles/github.min.css">
{katex_html}  <style>
{baseline_css}
{theme_css}
{custom_css}
  </style>
  <script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/highlight.min.js"></script>
  <script>hljs.highlightAll();</script>
</head>
<body>
<div class="lex-document">
{header_html}{body_html}
</div>
</body>
</html>"#
    );

    Ok(html)
}

/// Map common language aliases to highlight.js class names
fn normalize_language(lang: &str) -> &str {
    match lang {
        "js" => "javascript",
        "ts" => "typescript",
        "py" => "python",
        "sh" => "bash",
        "c++" | "cpp" => "cpp",
        "c#" | "csharp" => "csharp",
        "yml" => "yaml",
        "rb" => "ruby",
        "rs" => "rust",
        "kt" => "kotlin",
        "md" => "markdown",
        "objc" | "obj-c" => "objectivec",
        other => other,
    }
}

/// Convert IR inline content to plain text for title rendering
fn ir_inline_to_text(content: &[InlineContent]) -> String {
    content
        .iter()
        .map(|inline| match inline {
            InlineContent::Text(t) => t.clone(),
            InlineContent::Bold(c) | InlineContent::Italic(c) => ir_inline_to_text(c),
            InlineContent::Code(c) | InlineContent::Math(c) => c.clone(),
            InlineContent::Reference(r) => r.clone(),
            InlineContent::Link { text, .. } => text.clone(),
            InlineContent::Image(img) => img.alt.clone(),
        })
        .collect()
}

/// Escape HTML special characters in text
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
