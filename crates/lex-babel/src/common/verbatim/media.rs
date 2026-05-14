use crate::ir::nodes::{Audio, DocNode, Image, Video};
use std::collections::HashMap;

/// Build an IR `Image` node from a verbatim's body text + params.
/// Used by `from_lex_verbatim` to hydrate `lex.media.image` verbatim
/// blocks. Caller passes the verbatim's content (alt-text fallback)
/// and the `src` / `alt` / `title` parameter map.
pub(crate) fn image_from_params(content: &str, params: &HashMap<String, String>) -> DocNode {
    let src = params.get("src").cloned().unwrap_or_default();
    let alt = params
        .get("alt")
        .cloned()
        .unwrap_or_else(|| content.trim().to_string());
    let title = params.get("title").cloned();
    DocNode::Image(Image { src, alt, title })
}

/// Build an IR `Video` node from a verbatim's params. The verbatim
/// body is ignored — `lex.media.video` takes `src` / `title` /
/// `poster` parameters only.
pub(crate) fn video_from_params(params: &HashMap<String, String>) -> DocNode {
    let src = params.get("src").cloned().unwrap_or_default();
    let title = params.get("title").cloned();
    let poster = params.get("poster").cloned();
    DocNode::Video(Video { src, title, poster })
}

/// Build an IR `Audio` node from a verbatim's params.
/// `lex.media.audio` takes `src` / `title` parameters only.
pub(crate) fn audio_from_params(params: &HashMap<String, String>) -> DocNode {
    let src = params.get("src").cloned().unwrap_or_default();
    let title = params.get("title").cloned();
    DocNode::Audio(Audio { src, title })
}
