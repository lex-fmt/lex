//! HTML DOM construction from the IR event stream.
//!
//! Owns the body of the lex → HTML pipeline: the event walker that
//! materializes an rcdom tree (`build_html_dom_with_splice`) and the
//! inline-content emitter it leans on (`add_inline_to_node`). The leaf
//! node constructors, the DOM → string serializer, and the
//! full-document framing live in [`super::dom_helpers`]; this module is
//! purely the block/inline DOM build.

use super::dom_helpers::{
    create_comment, create_element, create_text, is_element_with_tag, normalize_language,
};
use crate::common::splice::{SentinelBuffer, SpliceState};
use crate::error::FormatError;
use crate::ir::events::Event;
use crate::ir::nodes::{InlineContent, TableCellAlignment};
use crate::render_dispatch::RenderedNode;
use markup5ever_rcdom::{Handle, RcDom};

/// Build an HTML DOM tree from IR events, optionally splicing
/// handler-rendered HTML in place of default annotation rendering.
///
/// When `splice_plan` is `Some(&plan)`, the splice state machine in
/// [`SpliceState`] tracks the alignment with the dispatch walker's
/// plan order; on entries with output, the walker registers a
/// sentinel comment in `sentinels` and the post-process step in
/// [`super::serialize_to_html_with_splice_from_ir`] substitutes it for
/// the handler's raw HTML.
///
/// When `splice_plan` is `None`, the builder behaves exactly as it
/// did before the splice landing.
///
/// Returns the constructed DOM and a `has_math` flag indicating whether any
/// `InlineContent::Math` was encountered during the walk. The caller uses
/// the flag to decide whether to inject the KaTeX renderer into the document
/// head — tracking it here is more reliable than scanning the serialized
/// HTML for the math class string, which can false-positive when a verbatim
/// code block happens to contain that text.
pub(super) fn build_html_dom_with_splice(
    events: &[Event],
    splice_plan: Option<&[RenderedNode]>,
    sentinels: &mut SentinelBuffer,
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

    // Splice state machine — counter + skip-depth — lives in the
    // shared helper. The HTML callback below converts the helper's
    // raw-passthrough string into a sentinel comment via `sentinels`,
    // and `serialize_to_html_with_splice_from_ir` substitutes after
    // DOM serialization.
    let mut splice = SpliceState::new(splice_plan);

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
        if splice.should_skip()
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
                // Create content wrapper (mirrors AST container structure for indentation).
                //
                // Inside a parent element that is ALREADY a semantic content
                // area, the wrapper is pure DOM bloat — two stacked containers
                // carrying the same "this is content" cue. Skip it for those
                // parents and push the parent onto the stack so EndContent's
                // pop is balanced and leaves current_parent pointing back at
                // the same element.
                //
                // - `<dd>`      — #604: the dd is already the content holder
                //                 for a definition body.
                // - `<section>` — #610 (option 1): after a heading the section
                //                 is the semantic content container; the
                //                 wrapper is redundant.
                current_heading = None;
                let parent_is_content_area = is_element_with_tag(&current_parent, "dd")
                    || is_element_with_tag(&current_parent, "section");
                if parent_is_content_area {
                    parent_stack.push(current_parent.clone());
                } else {
                    let content = create_element("div", vec![("class", "lex-content")]);
                    current_parent.children.borrow_mut().push(content.clone());
                    parent_stack.push(current_parent.clone());
                    current_parent = content;
                }
            }

            Event::EndContent => {
                // Close content wrapper
                current_parent = parent_stack.pop().ok_or_else(|| {
                    FormatError::SerializationError("Unbalanced content end".to_string())
                })?;
            }

            Event::StartParagraph => {
                // Paragraphs directly inside `<dd>` drop the redundant
                // `class="lex-paragraph"` — CSS can target `dd > p` instead (#604).
                current_heading = None;
                let para = if is_element_with_tag(&current_parent, "dd") {
                    create_element("p", vec![])
                } else {
                    create_element("p", vec![("class", "lex-paragraph")])
                };
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

            Event::StartVerbatim {
                language,
                subject,
                subject_href,
                parameters: _,
            } => {
                current_heading = None;
                in_verbatim = true;
                verbatim_language = language.clone();
                verbatim_content.clear();

                // Render subject as a caption before the code block. A reference
                // line can anchor the subject (references-general.lex §2.3.2):
                // when `subject_href` is set, the caption text is wrapped in a
                // link to that target.
                if let Some(subj) = subject {
                    let caption = create_element("div", vec![("class", "lex-verbatim-subject")]);
                    if let Some(href) = subject_href {
                        let anchor = create_element("a", vec![("href", href)]);
                        anchor.children.borrow_mut().push(create_text(subj));
                        caption.children.borrow_mut().push(anchor);
                    } else {
                        caption.children.borrow_mut().push(create_text(subj));
                    }
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
                if let Some(rendered_html) = splice.advance_at_start(label) {
                    // rcdom has no raw-HTML node; emit a sentinel
                    // comment that the post-process replacement
                    // expands. The skip-state is owned by `splice`.
                    let sentinel = sentinels.push(rendered_html.to_string());
                    let comment_node = create_comment(&sentinel);
                    current_parent.children.borrow_mut().push(comment_node);
                } else if !splice.should_skip() {
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
                if splice.should_skip() {
                    splice.advance_at_end();
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

        InlineContent::Reference {
            raw: ref_text,
            kind,
        } => {
            // Unresolved reference (non-linkable types like citations,
            // footnotes, etc.). Dispatch on the typed kind preserved
            // from lex-core; fall back to raw-string heuristic only for
            // `NotSure` (markdown / rfc-xml import paths that didn't
            // classify against lex's reference grammar).
            use crate::ir::nodes::ReferenceType;
            let href = match kind {
                ReferenceType::Citation(data) => {
                    // Anchor on the citation KEY only (`#ref-spec2025`); the raw
                    // literal includes the locator (`@spec2025, pp. 45-46`) which
                    // must not leak into the href. See the markdown serializer.
                    let key = data.keys.first().cloned().unwrap_or_else(|| {
                        ref_text.strip_prefix('@').unwrap_or(ref_text).to_string()
                    });
                    format!("#ref-{key}")
                }
                ReferenceType::NotSure => {
                    if let Some(citation) = ref_text.strip_prefix('@') {
                        format!("#ref-{citation}")
                    } else {
                        ref_text.clone()
                    }
                }
                _ => ref_text.clone(),
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
