//! Schemas for the `doc.*` family of document-level metadata labels.
//!
//! `doc.*` is the reserved namespace for document-level metadata.
//! [Issue #615](https://github.com/lex-fmt/lex/issues/615) registers
//! six built-in canonicals here as the registry-shaped replacement for
//! the hardcoded markdown frontmatter whitelist
//! (`crates/lex-babel/src/formats/markdown/serializer.rs`):
//!
//! - `doc.title`, `doc.author`, `doc.date`
//! - `doc.tags`, `doc.category`, `doc.template`
//!
//! Each schema declares per-format `render` hooks so the unified
//! dispatch surface can emit format-specific output:
//!
//! - **Markdown** — YAML frontmatter line (`<key>: <value>\n`).
//! - **HTML** — `<title>` / `<meta name="…" content="…">` element.
//!
//! The third-party `doc.*` namespace remains forbidden — only the
//! canonicals listed in [`DOC_BUILTIN_LABELS`] survive
//! `NormalizeLabels`'s strict-mode rejection. The bare metadata
//! shortcuts (`:: title ::`, `:: author ::`, …) continue to map onto
//! `lex.metadata.*` for now; rerouting them onto the `doc.*` canonicals
//! is the work of Sub D (#617), which retires the markdown HACK and
//! flips the shortcut table.
//!
//! The `note` label is intentionally absent — it's content-level (not
//! document-level) and stays in its natural namespace per the issue's
//! "naming guidance for the metadata replacement" section.

use lex_extension::handler::HandlerError;
use lex_extension::schema::{
    BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, RenderHook, Schema,
};
use lex_extension::wire::{AnnotationBody, Format, LabelCtx, RenderOut};
use std::collections::BTreeMap;

/// Fully-qualified labels for every built-in `doc.*` canonical, in the
/// order they appear in the issue's allocation list. `NormalizeLabels`
/// uses this slice to allow these (and only these) `doc.*` inputs
/// through the otherwise-forbidden namespace.
pub const DOC_BUILTIN_LABELS: &[&str] = &[
    "doc.title",
    "doc.author",
    "doc.date",
    "doc.tags",
    "doc.category",
    "doc.template",
];

/// `true` if `label` names one of the built-in `doc.*` canonicals.
/// Used by `NormalizeLabels` to carve the built-ins out of the strict
/// `doc.*` rejection.
pub fn is_doc_builtin(label: &str) -> bool {
    DOC_BUILTIN_LABELS.contains(&label)
}

/// Common shape: every `doc.*` metadata label attaches to the document,
/// carries its value as the annotation body (single-line text), and
/// declares the `markdown` + `html` render hooks that
/// `LexBuiltinsHandler::on_render` services.
fn doc_schema(label: &'static str, description: &'static str) -> Schema {
    Schema {
        schema_version: 1,
        label: label.into(),
        description: Some(description.into()),
        params: BTreeMap::new(),
        attaches_to: vec!["document".into()],
        body: BodyShape {
            kind: BodyKind::Text,
            presence: BodyPresence::Optional,
            description: Some(
                "Annotation body (single-line text) carries the metadata value.".into(),
            ),
        },
        verbatim_label: false,
        capabilities: Capabilities::default(),
        hooks: HookSet {
            render: vec![RenderHook::new("markdown"), RenderHook::new("html")],
            ..HookSet::default()
        },
        handler: None,
    }
}

pub fn doc_title_schema() -> Schema {
    doc_schema(
        "doc.title",
        "Document title. Renders into HTML `<title>` and YAML \
         frontmatter `title:` keys.",
    )
}

pub fn doc_author_schema() -> Schema {
    doc_schema(
        "doc.author",
        "Document author. Renders into HTML `<meta name=\"author\">` \
         and YAML frontmatter `author:` keys.",
    )
}

pub fn doc_date_schema() -> Schema {
    doc_schema(
        "doc.date",
        "Document date. Renders into HTML `<meta name=\"date\">` and \
         YAML frontmatter `date:` keys.",
    )
}

pub fn doc_tags_schema() -> Schema {
    doc_schema(
        "doc.tags",
        "Document tags (comma-separated). Renders into HTML \
         `<meta name=\"keywords\">` and YAML frontmatter `tags:` keys.",
    )
}

pub fn doc_category_schema() -> Schema {
    doc_schema(
        "doc.category",
        "Document category. Renders into HTML \
         `<meta name=\"category\">` and YAML frontmatter `category:` keys.",
    )
}

pub fn doc_template_schema() -> Schema {
    doc_schema(
        "doc.template",
        "Template hint for renderers that select a layout per document. \
         Surfaces in YAML frontmatter (`template:`) and as an HTML \
         `<meta name=\"template\">`.",
    )
}

/// Every `doc.*` schema, in declaration order matching
/// [`DOC_BUILTIN_LABELS`].
pub fn all_schemas() -> Vec<Schema> {
    vec![
        doc_title_schema(),
        doc_author_schema(),
        doc_date_schema(),
        doc_tags_schema(),
        doc_category_schema(),
        doc_template_schema(),
    ]
}

/// Render one `doc.*` annotation into format-specific text, on behalf
/// of `LexBuiltinsHandler::on_render`.
///
/// Returns `Ok(Some(RenderOut::String { string }))` for a recognised
/// `(label, format)` pair; `Ok(None)` lets the host fall back to its
/// default rendering for unhandled combinations (an unrecognised
/// label, an unsupported format, or an empty body — the host has
/// nothing useful to splice when the value is missing).
pub fn render_doc_annotation(
    ctx: &LabelCtx,
    format: &Format,
) -> Result<Option<RenderOut>, HandlerError> {
    let key = match ctx.label.strip_prefix("doc.") {
        Some(k) if is_doc_builtin(&ctx.label) => k,
        _ => return Ok(None),
    };
    let value = match &ctx.body {
        AnnotationBody::Text(s) => s.trim().to_string(),
        AnnotationBody::Lex { .. } | AnnotationBody::None => return Ok(None),
    };
    if value.is_empty() {
        return Ok(None);
    }

    let rendered = match format {
        Format::Markdown => render_markdown_yaml_line(key, &value),
        Format::Html => render_html_meta(key, &value),
        // Future: Format::Latex emits `\title{}`, `\author{}`, ...
        _ => return Ok(None),
    };
    Ok(Some(RenderOut::String { string: rendered }))
}

/// One YAML frontmatter line for the markdown serializer's preamble
/// builder (`<key>: <value>\n`). Internal `\n` collapse to ` ` so the
/// output stays a valid single YAML scalar.
fn render_markdown_yaml_line(key: &str, value: &str) -> String {
    let value = value.replace('\n', " ");
    format!("{key}: {value}\n")
}

/// One HTML `<head>` element for the html serializer. `doc.title`
/// becomes the `<title>`; everything else becomes a `<meta>` whose
/// `name=` attribute mirrors the lex key (with the `doc.tags`
/// → `keywords` rewrite the standard HTML metadata vocabulary
/// expects).
fn render_html_meta(key: &str, value: &str) -> String {
    if key == "title" {
        return format!("<title>{}</title>", html_escape_text(value));
    }
    let html_name = match key {
        "tags" => "keywords",
        other => other,
    };
    format!(
        "<meta name=\"{}\" content=\"{}\">",
        html_escape_text(html_name),
        html_escape_text(value)
    )
}

/// Minimal HTML-attribute escape. Restricts to the four characters
/// the consumers (markdown YAML preamble, HTML meta) need to emit
/// safely; doesn't pull in `html_escape` since this module wants no
/// new dep.
fn html_escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_extension::wire::{NodeRef, Position, Range};

    fn ctx(label: &str, body: AnnotationBody) -> LabelCtx {
        LabelCtx {
            label: label.into(),
            params: serde_json::Value::Null,
            body,
            node: NodeRef {
                kind: "document".into(),
                range: Range {
                    start: Position(0, 0),
                    end: Position(0, 0),
                },
                origin: None,
            },
        }
    }

    #[test]
    fn all_doc_schemas_attach_to_document() {
        for schema in all_schemas() {
            assert_eq!(
                schema.attaches_to,
                vec!["document".to_string()],
                "{} must attach to document",
                schema.label
            );
            assert!(
                !schema.verbatim_label,
                "{} is annotation, not verbatim",
                schema.label
            );
        }
    }

    #[test]
    fn all_doc_schemas_declare_markdown_and_html_render_hooks() {
        for schema in all_schemas() {
            let formats: Vec<&str> = schema.hooks.render.iter().map(|h| h.0.as_str()).collect();
            assert!(
                formats.contains(&"markdown") && formats.contains(&"html"),
                "{} must declare markdown + html render hooks; got {formats:?}",
                schema.label,
            );
            // `ir_build` and `resolve` are off — these are render-only
            // (the metadata value flows through document_annotations,
            // not through verbatim hydration).
            assert!(!schema.hooks.ir_build);
            assert!(!schema.hooks.resolve);
        }
    }

    #[test]
    fn doc_builtin_labels_match_all_schemas() {
        let labels: Vec<String> = all_schemas().into_iter().map(|s| s.label).collect();
        let expected: Vec<String> = DOC_BUILTIN_LABELS
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(
            labels, expected,
            "DOC_BUILTIN_LABELS must mirror all_schemas() so NormalizeLabels and \
             register_into see the same set"
        );
    }

    #[test]
    fn is_doc_builtin_recognises_every_canonical() {
        for label in DOC_BUILTIN_LABELS {
            assert!(is_doc_builtin(label), "{label} must be a doc built-in");
        }
        assert!(!is_doc_builtin("doc.unknown"));
        assert!(!is_doc_builtin("doc."));
        assert!(!is_doc_builtin("title"));
        assert!(!is_doc_builtin("lex.metadata.title"));
    }

    #[test]
    fn render_markdown_emits_yaml_line() {
        let c = ctx("doc.title", AnnotationBody::Text("My Doc".into()));
        let out = render_doc_annotation(&c, &Format::Markdown)
            .expect("ok")
            .expect("Some");
        match out {
            RenderOut::String { string } => assert_eq!(string, "title: My Doc\n"),
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[test]
    fn render_html_title_emits_title_element() {
        let c = ctx("doc.title", AnnotationBody::Text("My Doc".into()));
        let out = render_doc_annotation(&c, &Format::Html)
            .expect("ok")
            .expect("Some");
        match out {
            RenderOut::String { string } => assert_eq!(string, "<title>My Doc</title>"),
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[test]
    fn render_html_non_title_emits_meta_element() {
        let c = ctx("doc.author", AnnotationBody::Text("Alice".into()));
        let out = render_doc_annotation(&c, &Format::Html)
            .expect("ok")
            .expect("Some");
        match out {
            RenderOut::String { string } => {
                assert_eq!(string, "<meta name=\"author\" content=\"Alice\">")
            }
            other => panic!("expected String, got {other:?}"),
        }
    }

    /// `doc.tags` is the one label whose HTML name diverges from the
    /// lex key — it maps to the HTML standard `keywords` slot.
    #[test]
    fn render_html_tags_maps_to_keywords_meta() {
        let c = ctx("doc.tags", AnnotationBody::Text("rust, lex".into()));
        let out = render_doc_annotation(&c, &Format::Html)
            .expect("ok")
            .expect("Some");
        match out {
            RenderOut::String { string } => {
                assert_eq!(string, "<meta name=\"keywords\" content=\"rust, lex\">")
            }
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[test]
    fn render_html_escapes_special_characters() {
        let c = ctx("doc.title", AnnotationBody::Text("Lex & <Friends>".into()));
        let out = render_doc_annotation(&c, &Format::Html)
            .expect("ok")
            .expect("Some");
        match out {
            RenderOut::String { string } => {
                assert_eq!(string, "<title>Lex &amp; &lt;Friends&gt;</title>")
            }
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[test]
    fn render_returns_none_for_empty_body() {
        let c = ctx("doc.title", AnnotationBody::Text("   ".into()));
        let out = render_doc_annotation(&c, &Format::Markdown).expect("ok");
        assert!(
            out.is_none(),
            "empty body must fall back to host default rendering"
        );
    }

    #[test]
    fn render_returns_none_for_unsupported_format() {
        let c = ctx("doc.title", AnnotationBody::Text("My Doc".into()));
        let out = render_doc_annotation(&c, &Format::Custom("rfc-xml".into())).expect("ok");
        assert!(out.is_none());
    }

    #[test]
    fn render_returns_none_for_non_doc_label() {
        let c = ctx("acme.title", AnnotationBody::Text("My Doc".into()));
        let out = render_doc_annotation(&c, &Format::Markdown).expect("ok");
        assert!(out.is_none(), "unrelated labels must be skipped");
    }

    #[test]
    fn render_returns_none_for_unknown_doc_label() {
        // `doc.unknown` isn't on the built-in list — the handler must
        // not synthesize anything for it (the rejection test below
        // confirms NormalizeLabels also blocks it before we get here,
        // but the handler is defence-in-depth).
        let c = ctx("doc.unknown", AnnotationBody::Text("v".into()));
        let out = render_doc_annotation(&c, &Format::Markdown).expect("ok");
        assert!(out.is_none());
    }

    #[test]
    fn doc_schemas_round_trip_through_json() {
        for schema in all_schemas() {
            let json = serde_json::to_string(&schema).expect("serialize");
            let back: Schema = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, schema, "round trip for {}", schema.label);
        }
    }
}
