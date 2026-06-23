//! Tests for render-hook dispatch.
//!
//! Split by theme into sibling submodules; the shared fixtures
//! (`parse`, `schema`, the handler stubs and sample documents) live here
//! so every theme module reaches them through `use super::*`.
//!
//! - `dispatch_plan` — registry / format gating and the shape of the
//!   [`RenderPlan`](super::RenderPlan) the walk produces.
//! - `walk` — walk/visit ordering behaviour (labelled verbatim dispatch,
//!   table header-before-body traversal).
//! - `html_pipeline` — end-to-end HTML serialization + splice behaviour.
//! - `body_flatten` — `BodyKind::Text` body flattening
//!   (`super::super::flatten_text_body`).

use super::*;
use lex_core::lex::loader::DocumentLoader;
use lex_extension::schema::{
    BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, RenderHook, Schema,
};
use lex_extension::{HandlerError, LexHandler};
use std::collections::BTreeMap;

mod body_flatten;
mod dispatch_plan;
mod html_pipeline;
mod walk;

pub(super) fn parse(src: &str) -> Document {
    let ast = DocumentLoader::from_string(src).parse().expect("parse");
    crate::to_ir(&ast)
}

pub(super) fn schema(label: &str, formats: &[&str]) -> Schema {
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

pub(super) struct EchoRender;
impl LexHandler for EchoRender {
    fn on_render(&self, ctx: &LabelCtx, _fmt: Format) -> Result<Option<RenderOut>, HandlerError> {
        Ok(Some(RenderOut::String {
            string: format!("<RENDERED label=\"{}\"/>", ctx.label),
        }))
    }
}

// -- Splice fixtures. The splice mechanism replaces the default
// `<!-- lex:label -->` ... `<!-- /lex:label -->` comment pair (and
// any content between) with the handler's raw HTML when the
// handler returns `RenderOut::String`. WireAst and `Ok(None)`
// continue to fall through to default rendering.

pub(super) fn registry_with_string_handler(label: &str, html_output: &'static str) -> Registry {
    struct Fixed(&'static str);
    impl LexHandler for Fixed {
        fn on_render(&self, _: &LabelCtx, _: Format) -> Result<Option<RenderOut>, HandlerError> {
            Ok(Some(RenderOut::String {
                string: self.0.to_string(),
            }))
        }
    }
    let registry = Registry::new();
    registry
        .register_namespace(
            label.split_once('.').map(|(ns, _)| ns).unwrap_or(label),
            vec![schema(label, &["html"])],
            Box::new(Fixed(html_output)),
        )
        .unwrap();
    registry
}

pub(super) const DOC_WITH_SCOPED_ANNOTATION: &str =
    "1. Heading\n\n    :: acme.task ::\n        Body that should be replaced.\n";
