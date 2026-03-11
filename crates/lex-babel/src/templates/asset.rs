use super::normalize_path;
use crate::formats::lex::formatting_rules::FormattingRules;
use std::path::Path;

/// Classification for generated asset snippets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Image,
    Video,
    Audio,
    Data,
}

impl AssetKind {
    pub fn from_extension(ext: Option<&str>) -> Self {
        let ext = ext.unwrap_or("").to_ascii_lowercase();
        if matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg"
        ) {
            AssetKind::Image
        } else if matches!(ext.as_str(), "mp4" | "mov" | "webm" | "avi" | "mkv") {
            AssetKind::Video
        } else if matches!(ext.as_str(), "mp3" | "wav" | "flac" | "ogg" | "aac") {
            AssetKind::Audio
        } else {
            AssetKind::Data
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AssetKind::Image => "doc.image",
            AssetKind::Video => "doc.video",
            AssetKind::Audio => "doc.audio",
            AssetKind::Data => "doc.data",
        }
    }
}

/// Request describing how the snippet should be generated.
pub struct AssetSnippetRequest<'a> {
    /// Path to the asset on disk.
    pub asset_path: &'a Path,
    /// Directory of the document being edited (used to compute relative paths).
    pub document_directory: Option<&'a Path>,
    /// Formatting rules to respect when building indentation.
    pub formatting: &'a FormattingRules,
    /// Logical indentation level (0 for column aligned, 1 for nested, etc.).
    pub indent_level: usize,
}

impl<'a> AssetSnippetRequest<'a> {
    pub fn new(asset_path: &'a Path, formatting: &'a FormattingRules) -> Self {
        Self {
            asset_path,
            document_directory: None,
            formatting,
            indent_level: 0,
        }
    }

    /// Compute the indentation string for this request.
    fn indentation(&self) -> String {
        self.formatting.indent_string.repeat(self.indent_level)
    }
}

/// Result returned to callers so they can apply the snippet as a text edit.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetSnippet {
    pub kind: AssetKind,
    pub text: String,
    /// Byte offset (from the start of `text`) where editors can place the caret.
    pub cursor_offset: usize,
}

/// Build a canonical asset snippet for the provided request.
pub fn build_asset_snippet(request: &AssetSnippetRequest<'_>) -> AssetSnippet {
    let kind =
        AssetKind::from_extension(request.asset_path.extension().and_then(|ext| ext.to_str()));
    let normalized_path = normalize_path(request.asset_path, request.document_directory);
    let indent = request.indentation();

    let mut text = String::new();
    text.push_str(&indent);
    text.push_str(":: ");
    text.push_str(kind.label());
    text.push_str(" src=\"");
    text.push_str(&normalized_path);
    text.push_str("\"\n");

    AssetSnippet {
        kind,
        cursor_offset: text.len(),
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn chooses_kind_based_on_extension() {
        let png = NamedTempFile::new().unwrap();
        let path = png.path().with_extension("png");
        let rules = FormattingRules::default();
        let request = AssetSnippetRequest::new(path.as_path(), &rules);
        let snippet = build_asset_snippet(&request);
        assert_eq!(snippet.kind, AssetKind::Image);
        assert!(snippet.text.contains(":: doc.image"));
    }

    #[test]
    fn normalizes_to_relative_path_when_possible() {
        let temp = tempdir().unwrap();
        let doc_dir = temp.path();
        let asset_path = doc_dir.join("assets").join("diagram.png");
        std::fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
        std::fs::write(&asset_path, b"binary").unwrap();

        let rules = FormattingRules::default();
        let request = AssetSnippetRequest {
            asset_path: asset_path.as_path(),
            document_directory: Some(doc_dir),
            formatting: &rules,
            indent_level: 0,
        };
        let snippet = build_asset_snippet(&request);
        assert!(snippet.text.contains("src=\"./assets/diagram.png\""));
    }

    #[test]
    fn indent_level_is_respected() {
        let path = Path::new("./diagram.png");
        let rules = FormattingRules::default();
        let request = AssetSnippetRequest {
            asset_path: path,
            document_directory: None,
            formatting: &rules,
            indent_level: 2,
        };
        let snippet = build_asset_snippet(&request);
        assert!(snippet.text.starts_with("        ::"));
    }

    #[test]
    fn non_image_extensions_fallback_to_data() {
        let path = Path::new("./archive.zip");
        let rules = FormattingRules::default();
        let request = AssetSnippetRequest::new(path, &rules);
        let snippet = build_asset_snippet(&request);
        assert_eq!(snippet.kind, AssetKind::Data);
        assert!(snippet.text.contains(":: doc.data"));
    }
}
