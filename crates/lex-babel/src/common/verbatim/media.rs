use super::VerbatimHandler;
use crate::ir::nodes::{Audio, DocNode, Image, Video};
use std::collections::HashMap;

/// Handler for `doc.image` verbatim blocks.
pub struct ImageHandler;

impl VerbatimHandler for ImageHandler {
    fn label(&self) -> &str {
        "doc.image"
    }

    fn to_ir(&self, content: &str, params: &HashMap<String, String>) -> Option<DocNode> {
        // Content is usually empty for doc.image, params hold the data
        // Or content could be the alt text?
        // Let's assume params: src, alt, title.
        // If content is present, maybe treat it as alt text if alt param is missing?

        let src = params.get("src").cloned().unwrap_or_default();
        let alt = params
            .get("alt")
            .cloned()
            .unwrap_or_else(|| content.trim().to_string());
        let title = params.get("title").cloned();

        Some(DocNode::Image(Image { src, alt, title }))
    }

    fn convert_from_ir(&self, node: &DocNode) -> Option<(String, HashMap<String, String>)> {
        if let DocNode::Image(image) = node {
            let mut params = HashMap::new();
            params.insert("src".to_string(), image.src.clone());
            if !image.alt.is_empty() {
                params.insert("alt".to_string(), image.alt.clone());
            }
            if let Some(title) = &image.title {
                params.insert("title".to_string(), title.clone());
            }
            Some((String::new(), params))
        } else {
            None
        }
    }
}

/// Handler for `doc.video` verbatim blocks.
pub struct VideoHandler;

impl VerbatimHandler for VideoHandler {
    fn label(&self) -> &str {
        "doc.video"
    }

    fn to_ir(&self, _content: &str, params: &HashMap<String, String>) -> Option<DocNode> {
        let src = params.get("src").cloned().unwrap_or_default();
        let title = params.get("title").cloned();
        let poster = params.get("poster").cloned();

        Some(DocNode::Video(Video { src, title, poster }))
    }

    fn convert_from_ir(&self, node: &DocNode) -> Option<(String, HashMap<String, String>)> {
        if let DocNode::Video(video) = node {
            let mut params = HashMap::new();
            params.insert("src".to_string(), video.src.clone());
            if let Some(title) = &video.title {
                params.insert("title".to_string(), title.clone());
            }
            if let Some(poster) = &video.poster {
                params.insert("poster".to_string(), poster.clone());
            }
            Some((String::new(), params))
        } else {
            None
        }
    }
}

/// Handler for `doc.audio` verbatim blocks.
pub struct AudioHandler;

impl VerbatimHandler for AudioHandler {
    fn label(&self) -> &str {
        "doc.audio"
    }

    fn to_ir(&self, _content: &str, params: &HashMap<String, String>) -> Option<DocNode> {
        let src = params.get("src").cloned().unwrap_or_default();
        let title = params.get("title").cloned();

        Some(DocNode::Audio(Audio { src, title }))
    }

    fn convert_from_ir(&self, node: &DocNode) -> Option<(String, HashMap<String, String>)> {
        if let DocNode::Audio(audio) = node {
            let mut params = HashMap::new();
            params.insert("src".to_string(), audio.src.clone());
            if let Some(title) = &audio.title {
                params.insert("title".to_string(), title.clone());
            }
            Some((String::new(), params))
        } else {
            None
        }
    }
}
