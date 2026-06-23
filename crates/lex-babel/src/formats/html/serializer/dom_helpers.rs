//! Low-level DOM construction, serialization, and document framing for
//! HTML export.
//!
//! These are the leaf helpers the event walker in [`super::dom_build`]
//! leans on — rcdom node constructors (`create_element`, `create_text`,
//! `create_comment`), the small element predicate (`is_element_with_tag`),
//! the DOM → string serializer (`serialize_dom`), the full-document
//! wrapper that embeds CSS / KaTeX / highlight.js (`wrap_in_document`),
//! and the two pure string utilities (`normalize_language`,
//! `html_escape`). None of them touch the IR event stream; they're the
//! reusable substrate beneath the walk.

use crate::error::FormatError;
use crate::formats::html::{HtmlOptions, HtmlTheme};
use html5ever::{
    ns, serialize, serialize::SerializeOpts, serialize::TraversalScope, Attribute, LocalName,
    QualName,
};
use markup5ever_rcdom::{Handle, Node, NodeData, RcDom, SerializableHandle};
use std::cell::{Cell, RefCell};
use std::default::Default;
use std::rc::Rc;

/// Create an HTML element with attributes
pub(super) fn create_element(tag: &str, attrs: Vec<(&str, &str)>) -> Handle {
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

/// Return true if `handle` is an element with the given tag name.
pub(super) fn is_element_with_tag(handle: &Handle, tag: &str) -> bool {
    if let NodeData::Element { name, .. } = &handle.data {
        name.local.as_ref() == tag
    } else {
        false
    }
}

/// Create a text node
pub(super) fn create_text(text: &str) -> Handle {
    Rc::new(Node {
        parent: Cell::new(None),
        children: RefCell::new(Vec::new()),
        data: NodeData::Text {
            contents: RefCell::new(text.to_string().into()),
        },
    })
}

/// Create a comment node
pub(super) fn create_comment(text: &str) -> Handle {
    Rc::new(Node {
        parent: Cell::new(None),
        children: RefCell::new(Vec::new()),
        data: NodeData::Comment {
            contents: text.to_string().into(),
        },
    })
}

/// Serialize the DOM to an HTML string (just the inner content)
pub(super) fn serialize_dom(dom: &RcDom) -> Result<String, FormatError> {
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
pub(super) fn wrap_in_document(
    body_html: &str,
    head_title: &str,
    body_title: Option<&str>,
    body_subtitle: Option<&str>,
    has_math: bool,
    options: &HtmlOptions,
) -> Result<String, FormatError> {
    let baseline_css = include_str!("../../../../css/baseline.css");
    let theme_css = match options.theme {
        HtmlTheme::FancySerif => include_str!("../../../../css/themes/theme-fancy-serif.css"),
        HtmlTheme::Modern => include_str!("../../../../css/themes/theme-modern.css"),
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
    // ~290 KB on the wire for math-free documents. SRI hashes are pinned to
    // KaTeX 0.16.11; bumping the version requires re-computing them from the
    // release tarball (github.com/KaTeX/KaTeX/releases) with
    // `openssl dgst -sha384 -binary <file> | openssl base64 -A`.
    let katex_html = if has_math {
        r#"  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css" integrity="sha384-nB0miv6/jRmo5UMMR1wu3Gz6NLsoTkbqJghGIsx//Rlm+ZU03BU6SQNC66uf4l5+" crossorigin="anonymous">
  <script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js" integrity="sha384-7zkQWkzuo3B5mTepMUcHkMB5jZaolc2xDwL6VFqjFALcbeS9Ggm/Yr2r3Dy4lfFg" crossorigin="anonymous"></script>
  <script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js" integrity="sha384-43gviWU0YVjaDtb/GhzOouOXtZMP/7XUzwPTstBeZFe/+rCMvRwr4yROQP43s0Xk" crossorigin="anonymous" onload="renderMathInElement(document.body, {delimiters: [{left: '$', right: '$', display: false}, {left: '$$', right: '$$', display: true}], throwOnError: false});"></script>
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
  <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/styles/github.min.css" integrity="sha384-eFTL69TLRZTkNfYZOLM+G04821K1qZao/4QLJbet1pP4tcF+fdXq/9CdqAbWRl/L" crossorigin="anonymous">
{katex_html}  <style>
{baseline_css}
{theme_css}
{custom_css}
  </style>
  <script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/highlight.min.js" integrity="sha384-RH2xi4eIQ/gjtbs9fUXM68sLSi99C7ZWBRX1vDrVv6GQXRibxXLbwO2NGZB74MbU" crossorigin="anonymous"></script>
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
pub(super) fn normalize_language(lang: &str) -> &str {
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

/// Escape HTML special characters in text
pub(super) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
