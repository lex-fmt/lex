use lex_babel::format::Format;
use lex_babel::formats::lex::LexFormat;
use lex_babel::formats::markdown::serializer::serialize_to_markdown_with_registry;
use lex_babel::formats::markdown::MarkdownFormat;
use lex_core::lex::loader::DocumentLoader;
use lex_extension::schema::{
    BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, RenderHook, Schema,
};
use lex_extension::wire::{Format as WireFormat, LabelCtx, RenderOut};
use lex_extension::{HandlerError, LexHandler};
use lex_extension_host::Registry;
use std::collections::BTreeMap;

#[test]
fn test_annotation_round_trip() {
    let md = r#"
<!-- lex:note type=warning -->
This is a warning.
<!-- /lex:note -->
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");
    let output = MarkdownFormat
        .serialize(&doc)
        .expect("Failed to serialize markdown");

    println!("Output:\n{output}");

    assert!(output.contains("<!-- lex:note type=warning"));
    assert!(output.contains("This is a warning."));
    assert!(output.contains("-->"));
}

/// Issue #593: when markdown imports an annotation with a blessed
/// shortcut spelling (`<!-- lex:title ...`), the `markdown → lex`
/// conversion must emit the same shortcut spelling (`:: title :: ...`)
/// rather than the verbose canonical (`:: lex.metadata.title ::`).
/// The IR-level `LabelForm` propagation is what makes this work.
#[test]
fn markdown_to_lex_preserves_blessed_shortcut_form() {
    let md = "<!-- lex:title -->\nMy Doc\n<!-- /lex:title -->\n";

    let doc = MarkdownFormat.parse(md).expect("parse markdown");
    let lex_output = LexFormat::default().serialize(&doc).expect("serialize lex");

    // The serializer may emit either inline (`:: title :: My Doc`) or
    // block (`:: title\n    My Doc`) shape — both are acceptable; the
    // critical check is that the label is the blessed shortcut spelling
    // rather than the verbose canonical.
    assert!(
        lex_output.contains(":: title"),
        "expected blessed shortcut spelling, got:\n{lex_output}"
    );
    assert!(
        lex_output.contains("My Doc"),
        "annotation body lost in output:\n{lex_output}"
    );
    assert!(
        !lex_output.contains("lex.metadata.title"),
        "canonical form leaked into output:\n{lex_output}"
    );
}

#[test]
fn test_nested_annotations() {
    let md = r#"
<!-- lex:outer -->
  <!-- lex:inner -->
  Nested content
  <!-- /lex:inner -->
<!-- /lex:outer -->
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");
    let output = MarkdownFormat
        .serialize(&doc)
        .expect("Failed to serialize markdown");

    assert!(output.contains("<!-- lex:outer -->"));
    assert!(output.contains("<!-- lex:inner -->"));
    assert!(output.contains("Nested content"));
    assert!(output.contains("<!-- /lex:inner -->"));
    assert!(output.contains("<!-- /lex:outer -->"));
}

// =================================================================
// #617 acceptance: SpliceState retires the markdown HACK
// =================================================================

fn schema(label: &str, formats: &[&str]) -> Schema {
    Schema {
        schema_version: 1,
        label: label.into(),
        description: None,
        params: BTreeMap::new(),
        attaches_to: vec![
            "annotation".into(),
            "document".into(),
            "session".into(),
            "paragraph".into(),
        ],
        body: BodyShape {
            kind: BodyKind::None,
            presence: BodyPresence::Optional,
            description: None,
        },
        verbatim_label: false,
        capabilities: Capabilities::default(),
        hooks: HookSet {
            render: formats.iter().map(|s| RenderHook::new(*s)).collect(),
            ..HookSet::default()
        },
        handler: None,
        diagnostics: Vec::new(),
    }
}

/// #617 acceptance: a content-level metadata annotation with rich
/// body content (paragraphs, nested lists, nested annotations) survives
/// lex → markdown → lex without dropping body shape. The pre-#617
/// markdown HACK only round-tripped flat single-paragraph bodies; the
/// SpliceState migration treats every annotation generically and the
/// default rendering preserves the body via Comrak.
#[test]
fn note_with_rich_body_round_trips_lex_markdown_lex() {
    // Place the `:: note ::` inside a session body so it stays a
    // content-level annotation (top-level annotations are absorbed
    // into the doc-scope IR slot, which renders as YAML frontmatter).
    let lex_src = "1. Heading\n\n    \
                   :: note ::\n        \
                   First paragraph of the note.\n\n        \
                   Second paragraph references rust.\n\n        \
                   - bullet one\n        \
                   - bullet two\n\n        \
                   :: warning ::\n            \
                   Nested annotation body.\n";

    let lex_doc = DocumentLoader::from_string(lex_src)
        .parse()
        .expect("parse original lex");
    let md = MarkdownFormat.serialize(&lex_doc).expect("lex → markdown");

    // Default rendering should bracket the note with the comment pair
    // and pass the body through as markdown blocks. No metadata-
    // whitelist HACK is rewriting it into a `<!-- lex:note ...\nbody\n-->`
    // block any more.
    assert!(
        md.contains("<!-- lex:note -->"),
        "outer note open comment missing in:\n{md}"
    );
    assert!(
        md.contains("<!-- /lex:note -->"),
        "outer note close comment missing in:\n{md}"
    );
    assert!(
        md.contains("<!-- lex:warning -->"),
        "nested warning open comment missing in:\n{md}"
    );
    assert!(
        md.contains("<!-- /lex:warning -->"),
        "nested warning close comment missing in:\n{md}"
    );
    assert!(
        md.contains("First paragraph"),
        "first paragraph body missing in:\n{md}"
    );
    assert!(
        md.contains("Second paragraph"),
        "second paragraph body missing in:\n{md}"
    );
    assert!(md.contains("bullet one"), "list item lost in:\n{md}");
    assert!(
        md.contains("Nested annotation body"),
        "nested body lost in:\n{md}"
    );

    let re_imported = MarkdownFormat
        .parse(&md)
        .expect("markdown → lex roundtrip parse");
    let back_to_lex = LexFormat::default()
        .serialize(&re_imported)
        .expect("lex serialise");
    assert!(
        back_to_lex.contains(":: note"),
        "outer note annotation missing after round trip:\n{back_to_lex}"
    );
    assert!(
        back_to_lex.contains(":: warning"),
        "nested warning missing after round trip:\n{back_to_lex}"
    );
    assert!(
        back_to_lex.contains("First paragraph"),
        "first paragraph lost after round trip:\n{back_to_lex}"
    );
    assert!(
        back_to_lex.contains("Second paragraph"),
        "second paragraph lost after round trip:\n{back_to_lex}"
    );
    assert!(
        back_to_lex.contains("bullet one"),
        "list item lost after round trip:\n{back_to_lex}"
    );
    assert!(
        back_to_lex.contains("Nested annotation body"),
        "nested body lost after round trip:\n{back_to_lex}"
    );
}

struct FixedRender(&'static str);
impl LexHandler for FixedRender {
    fn on_render(&self, _: &LabelCtx, _: WireFormat) -> Result<Option<RenderOut>, HandlerError> {
        Ok(Some(RenderOut::String {
            string: self.0.to_string(),
        }))
    }
}

/// #617 acceptance: a non-metadata annotation with a registered
/// markdown render hook produces the handler-defined output in the
/// markdown export. Without splicing, the body annotation would have
/// fallen back to the default `<!-- lex:label -->` comment pair.
#[test]
fn markdown_render_hook_splices_handler_output_into_export() {
    let lex_src = "1. Heading\n\n    :: acme.task ::\n        Default body content.\n";
    let lex_doc = DocumentLoader::from_string(lex_src).parse().expect("parse");

    let registry = Registry::new();
    registry
        .register_namespace(
            "acme",
            vec![schema("acme.task", &["markdown"])],
            Box::new(FixedRender("**HANDLER MARKDOWN**")),
        )
        .expect("register namespace");

    let outcome = serialize_to_markdown_with_registry(&lex_doc, &registry).expect("md serialise");
    assert!(
        outcome.markdown.contains("**HANDLER MARKDOWN**"),
        "handler markdown should be spliced into the export. got:\n{}",
        outcome.markdown
    );
    assert!(
        !outcome.markdown.contains("<!-- lex:acme.task"),
        "default open comment should be replaced by splice. got:\n{}",
        outcome.markdown
    );
    assert!(
        !outcome.markdown.contains("<!-- /lex:acme.task"),
        "default close comment should be replaced by splice. got:\n{}",
        outcome.markdown
    );
    assert!(
        !outcome.markdown.contains("Default body content."),
        "annotation body must be suppressed inside the handler-owned region. got:\n{}",
        outcome.markdown
    );
}

/// #617 acceptance: a non-metadata annotation *without* a registered
/// markdown render hook still gets the default comment-pair rendering.
/// Locks in pre-#617 behavior for the no-handler path.
#[test]
fn unregistered_body_annotation_emits_default_comment_pair() {
    let lex_src = "1. Heading\n\n    :: acme.task ::\n        Annotation body.\n";
    let lex_doc = DocumentLoader::from_string(lex_src).parse().expect("parse");

    // Empty registry — no schema for acme.task; default rendering applies.
    let registry = Registry::new();
    let outcome = serialize_to_markdown_with_registry(&lex_doc, &registry).expect("md serialise");
    assert!(
        outcome.markdown.contains("<!-- lex:acme.task -->"),
        "default open comment missing. got:\n{}",
        outcome.markdown
    );
    assert!(
        outcome.markdown.contains("<!-- /lex:acme.task -->"),
        "default close comment missing. got:\n{}",
        outcome.markdown
    );
    assert!(
        outcome.markdown.contains("Annotation body."),
        "body should render between the comment pair. got:\n{}",
        outcome.markdown
    );
}

/// #617 acceptance: Sub B's `doc.*` schemas (registered in the default
/// `lex.*` builtins) fire correctly during markdown serialization and
/// their per-format `on_render` output replaces the default YAML
/// synthesis for matching doc-scope annotations.
#[test]
fn doc_metadata_schemas_fire_during_markdown_serialization() {
    let lex_src = ":: doc.title :: My Doc\n\n:: doc.author :: Alice\n\nBody.\n";
    let lex_doc = DocumentLoader::from_string(lex_src).parse().expect("parse");

    let md = MarkdownFormat
        .serialize(&lex_doc)
        .expect("markdown serialise");

    // The `doc.*` handlers emit a quoted YAML scalar (`title: "My Doc"\n`).
    // The fallback synthesis used to emit unquoted text (`title: My Doc\n`);
    // the quoted form is the diff that proves the handler fired.
    assert!(
        md.starts_with("---\n"),
        "YAML frontmatter missing at start of output:\n{md}"
    );
    assert!(
        md.contains("title: \"My Doc\"\n"),
        "expected handler-rendered title line (`title: \"My Doc\"`). got:\n{md}"
    );
    assert!(
        md.contains("author: \"Alice\"\n"),
        "expected handler-rendered author line. got:\n{md}"
    );
}

/// Coverage from the prior `lex_metadata_annotations_emit_yaml_frontmatter`
/// test, hoisted to integration so it exercises the default registry
/// path end-to-end: `:: title :: ...` shortcuts still map onto
/// `lex.metadata.*` and the fallback synthesis emits an unquoted YAML
/// scalar. Pins the contract that the `doc.*` handlers do **not** fire
/// for `lex.metadata.*` labels (the shortcut table flip is out of
/// scope for this PR).
#[test]
fn lex_metadata_shortcut_falls_back_to_yaml_synthesis() {
    let lex_src = ":: title :: My Doc\n\n:: author :: Alice\n\nBody.\n";
    let lex_doc = DocumentLoader::from_string(lex_src).parse().expect("parse");

    let md = MarkdownFormat
        .serialize(&lex_doc)
        .expect("markdown serialise");

    assert!(md.starts_with("---\n"), "frontmatter missing in:\n{md}");
    assert!(
        md.contains("title: My Doc"),
        "synthesised title missing in:\n{md}"
    );
    assert!(
        md.contains("author: Alice"),
        "synthesised author missing in:\n{md}"
    );
}
