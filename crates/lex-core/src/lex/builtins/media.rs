//! Schemas for the `lex.media.*` family of verbatim labels:
//! `lex.media.image`, `lex.media.video`, `lex.media.audio`.
//!
//! `on_ir_build` is declared on all three; the hook bodies live in
//! [`crate::lex::builtins`] (`resolve_media_image` / `_video` /
//! `_audio`) and emit the typed [`WireNode::Image`],
//! [`WireNode::Video`], [`WireNode::Audio`] variants introduced with
//! `wire_version: 2`.
//!
//! Pre-#615 these dispatched through `on_resolve`; the unified
//! registry surface (#615) moved them onto the IR-construction
//! lifecycle hook so verbatim-hydration handlers don't share a
//! lifecycle with AST-substitution handlers (`lex.include`).

use lex_extension::schema::{
    BodyKind, BodyPresence, BodyShape, Capabilities, HookSet, ParamSpec, ParamType, Schema,
};
use std::collections::BTreeMap;

pub const LEX_MEDIA_IMAGE: &str = "lex.media.image";
pub const LEX_MEDIA_VIDEO: &str = "lex.media.video";
pub const LEX_MEDIA_AUDIO: &str = "lex.media.audio";

fn string_param(required: bool, description: &'static str) -> ParamSpec {
    ParamSpec {
        ty: ParamType::String,
        required,
        default: None,
        description: Some(description.into()),
        pattern: None,
        values: Vec::new(),
    }
}

/// Common shape for media labels: verbatim attachment, optional text
/// body (image uses it as an alt-text fallback; video/audio ignore it),
/// no hooks until Phase 3.
fn media_schema(
    label: &'static str,
    description: &'static str,
    params: BTreeMap<String, ParamSpec>,
    body_description: &'static str,
) -> Schema {
    Schema {
        schema_version: 1,
        label: label.into(),
        description: Some(description.into()),
        params,
        attaches_to: vec!["verbatim".into()],
        body: BodyShape {
            kind: BodyKind::Text,
            presence: BodyPresence::Optional,
            description: Some(body_description.into()),
        },
        verbatim_label: true,
        capabilities: Capabilities::default(),
        hooks: HookSet {
            ir_build: true,
            ..HookSet::default()
        },
        handler: None,
    }
}

pub fn lex_media_image_schema() -> Schema {
    let mut params = BTreeMap::new();
    params.insert(
        "src".into(),
        string_param(true, "Source URL or path of the image."),
    );
    params.insert(
        "alt".into(),
        string_param(
            false,
            "Alternative text. Falls back to the verbatim body's first non-empty line when omitted.",
        ),
    );
    params.insert(
        "title".into(),
        string_param(false, "Tooltip / accessible title for the image."),
    );
    media_schema(
        LEX_MEDIA_IMAGE,
        "Image media block. The verbatim body, when present, is treated as the \
         alt-text fallback if no `alt=` parameter is supplied.",
        params,
        "Optional alt-text fallback; ignored when an explicit `alt=` parameter is present.",
    )
}

pub fn lex_media_video_schema() -> Schema {
    let mut params = BTreeMap::new();
    params.insert(
        "src".into(),
        string_param(true, "Source URL or path of the video."),
    );
    params.insert(
        "title".into(),
        string_param(false, "Accessible title for the video."),
    );
    params.insert(
        "poster".into(),
        string_param(false, "Poster image shown before playback begins."),
    );
    media_schema(
        LEX_MEDIA_VIDEO,
        "Video media block.",
        params,
        "Reserved for renderer-specific extensions; ignored by the built-in renderers.",
    )
}

pub fn lex_media_audio_schema() -> Schema {
    let mut params = BTreeMap::new();
    params.insert(
        "src".into(),
        string_param(true, "Source URL or path of the audio file."),
    );
    params.insert(
        "title".into(),
        string_param(false, "Accessible title for the audio clip."),
    );
    media_schema(
        LEX_MEDIA_AUDIO,
        "Audio media block.",
        params,
        "Reserved for renderer-specific extensions; ignored by the built-in renderers.",
    )
}

/// All `lex.media.*` schemas, in declaration order.
pub fn all_schemas() -> Vec<Schema> {
    vec![
        lex_media_image_schema(),
        lex_media_video_schema(),
        lex_media_audio_schema(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_media_schema_is_a_verbatim_label() {
        for schema in all_schemas() {
            assert!(
                schema.verbatim_label,
                "{} must be a verbatim label",
                schema.label
            );
            assert_eq!(
                schema.attaches_to,
                vec!["verbatim".to_string()],
                "{} attaches to verbatim",
                schema.label
            );
            assert!(
                schema
                    .params
                    .get("src")
                    .map(|p| p.required)
                    .unwrap_or(false),
                "{} must declare `src` as a required parameter",
                schema.label
            );
        }
    }

    #[test]
    fn image_schema_carries_optional_alt_and_title() {
        let schema = lex_media_image_schema();
        let alt = schema.params.get("alt").expect("alt declared");
        let title = schema.params.get("title").expect("title declared");
        assert!(!alt.required);
        assert!(!title.required);
    }

    #[test]
    fn video_schema_carries_optional_poster() {
        let schema = lex_media_video_schema();
        let poster = schema.params.get("poster").expect("poster declared");
        assert!(!poster.required);
    }

    #[test]
    fn audio_schema_has_no_poster() {
        let schema = lex_media_audio_schema();
        assert!(
            !schema.params.contains_key("poster"),
            "audio schemas have no poster parameter"
        );
    }

    #[test]
    fn media_schemas_declare_ir_build_hook() {
        // #615: media labels declare the IR-build lifecycle hook so
        // they go through the unified `dispatch_ir_build` surface
        // alongside `lex.tabular.table`. `on_resolve` is reserved for
        // AST substitution (`lex.include`); the validate + render
        // hooks stay off for media — future-phase work.
        for schema in all_schemas() {
            assert!(
                schema.hooks.ir_build,
                "{} ir_build must be on (#615 unified surface)",
                schema.label
            );
            assert!(
                !schema.hooks.resolve,
                "{} resolve must be off after #615 migration",
                schema.label
            );
            assert!(
                !schema.hooks.validate,
                "{} validate must stay off",
                schema.label
            );
            assert!(
                schema.hooks.render.is_empty(),
                "{} render must stay off",
                schema.label
            );
        }
    }

    #[test]
    fn media_schemas_round_trip_through_json() {
        for schema in all_schemas() {
            let json = serde_json::to_string(&schema).expect("serialize");
            let back: Schema = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, schema, "round trip for {}", schema.label);
        }
    }
}
