use super::{asset::AssetKind, normalize_path};
use crate::formats::lex::formatting_rules::FormattingRules;
use std::fs;
use std::io;
use std::path::Path;

pub struct VerbatimSnippetRequest<'a> {
    pub file_path: &'a Path,
    pub document_directory: Option<&'a Path>,
    pub formatting: &'a FormattingRules,
    pub indent_level: usize,
    pub language: Option<&'a str>,
    pub subject: Option<&'a str>,
}

impl<'a> VerbatimSnippetRequest<'a> {
    pub fn new(file_path: &'a Path, formatting: &'a FormattingRules) -> Self {
        Self {
            file_path,
            document_directory: None,
            formatting,
            indent_level: 0,
            language: None,
            subject: None,
        }
    }

    fn indentation(&self) -> String {
        self.formatting.indent_string.repeat(self.indent_level)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerbatimSnippet {
    pub language: String,
    pub text: String,
    pub cursor_offset: usize,
}

pub fn build_verbatim_snippet(request: &VerbatimSnippetRequest<'_>) -> io::Result<VerbatimSnippet> {
    let raw = fs::read(request.file_path)?;
    let (contents, language) = match String::from_utf8(raw) {
        Ok(text) => {
            let normalized = text.replace("\r\n", "\n");
            (Some(normalized), language_label(request))
        }
        Err(_) => (None, media_label(request.file_path)),
    };

    let indent = request.indentation();
    let inner_indent = format!("{}{}", indent, request.formatting.indent_string);
    let subject = subject_line(request);
    let normalized_path = normalize_path(request.file_path, request.document_directory);

    let mut text = String::new();
    text.push_str(&indent);
    text.push_str(&subject);
    text.push('\n');
    text.push('\n');

    match contents {
        Some(body) => {
            if body.is_empty() {
                text.push_str(&inner_indent);
                text.push('\n');
            } else {
                for line in body.split('\n') {
                    text.push_str(&inner_indent);
                    text.push_str(line);
                    text.push('\n');
                }
            }
        }
        None => {
            text.push_str(&inner_indent);
            text.push('\n');
        }
    }

    text.push_str(&indent);
    text.push_str(":: ");
    text.push_str(&language);
    text.push(' ');
    text.push_str("src=\"");
    text.push_str(&normalized_path);
    text.push_str("\" ::\n");

    Ok(VerbatimSnippet {
        language,
        cursor_offset: indent.len(),
        text,
    })
}

fn subject_line(request: &VerbatimSnippetRequest<'_>) -> String {
    if let Some(custom) = request.subject {
        return ensure_trailing_colon(custom);
    }
    request
        .file_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(ensure_trailing_colon)
        .unwrap_or_else(|| "Verbatim:".to_string())
}

fn ensure_trailing_colon(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.ends_with(':') {
        trimmed.to_string()
    } else {
        format!("{trimmed}:")
    }
}

fn language_label(request: &VerbatimSnippetRequest<'_>) -> String {
    if let Some(lang) = request.language {
        return canonical_language(lang);
    }
    request
        .file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(canonical_language)
        .unwrap_or_else(|| "text".to_string())
}

fn canonical_language(value: &str) -> String {
    let token = value.trim().to_ascii_lowercase();
    language_from_token(&token)
        .map(str::to_string)
        .unwrap_or(token)
}

fn language_from_token(token: &str) -> Option<&'static str> {
    match token {
        "bash" | "sh" | "zsh" | "shell" => Some("bash"),
        "bat" | "cmd" => Some("batch"),
        "c" | "h" => Some("c"),
        "cpp" | "cxx" | "cc" | "hpp" | "hh" | "hxx" => Some("cpp"),
        "cs" | "csharp" => Some("csharp"),
        "css" => Some("css"),
        "go" => Some("go"),
        "hs" | "haskell" => Some("haskell"),
        "html" | "htm" => Some("html"),
        "java" => Some("java"),
        "js" | "mjs" | "cjs" | "jsx" => Some("javascript"),
        "json" => Some("json"),
        "kt" | "kts" => Some("kotlin"),
        "latex" | "tex" => Some("latex"),
        "lua" => Some("lua"),
        "md" | "markdown" => Some("markdown"),
        "php" => Some("php"),
        "ps1" | "psm1" | "powershell" => Some("powershell"),
        "py" | "pyw" | "python" => Some("python"),
        "rb" => Some("ruby"),
        "rs" => Some("rust"),
        "scala" => Some("scala"),
        "sql" => Some("sql"),
        "swift" => Some("swift"),
        "toml" => Some("toml"),
        "ts" | "tsx" => Some("typescript"),
        "vue" => Some("vue"),
        "xml" => Some("xml"),
        "yaml" | "yml" => Some("yaml"),
        _ => None,
    }
}

fn media_label(path: &Path) -> String {
    let kind = AssetKind::from_extension(path.extension().and_then(|ext| ext.to_str()));
    kind.label().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn infers_language_from_extension() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.rs");
        fs::write(&file, "fn main() {}\n").unwrap();
        let rules = FormattingRules::default();
        let request = VerbatimSnippetRequest::new(file.as_path(), &rules);
        let snippet = build_verbatim_snippet(&request).unwrap();
        assert!(snippet.text.contains(":: rust"));
        assert!(snippet.text.contains("\" ::"), "closing :: marker missing");
        assert_eq!(snippet.language, "rust");
    }

    #[test]
    fn uses_relative_src_path() {
        let dir = tempdir().unwrap();
        let doc_dir = dir.path();
        let file = doc_dir.join("code").join("example.py");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "print('hi')").unwrap();
        let rules = FormattingRules::default();
        let mut request = VerbatimSnippetRequest::new(file.as_path(), &rules);
        request.document_directory = Some(doc_dir);
        request.language = Some("python");
        let snippet = build_verbatim_snippet(&request).unwrap();
        assert!(snippet.text.contains("src=\"./code/example.py\""));
        assert!(snippet.text.contains(":: python"));
    }

    #[test]
    fn indents_body_lines_relative_to_level() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.txt");
        fs::write(&file, "line1\nline2").unwrap();
        let rules = FormattingRules::default();
        let mut request = VerbatimSnippetRequest::new(file.as_path(), &rules);
        request.indent_level = 1;
        let snippet = build_verbatim_snippet(&request).unwrap();
        let expected = "        line1";
        assert!(snippet.text.contains(expected));
    }

    #[test]
    fn cursor_offset_points_to_subject_start() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("snippet.py");
        fs::write(&file, "print('hi')\n").unwrap();
        let rules = FormattingRules::default();
        let mut request = VerbatimSnippetRequest::new(file.as_path(), &rules);
        request.indent_level = 1;
        let snippet = build_verbatim_snippet(&request).unwrap();
        assert_eq!(snippet.cursor_offset, rules.indent_string.len());
    }

    #[test]
    fn binary_files_use_media_labels() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("image.png");
        fs::write(&file, [0u8, 159, 146, 150]).unwrap();
        let rules = FormattingRules::default();
        let request = VerbatimSnippetRequest::new(file.as_path(), &rules);
        let snippet = build_verbatim_snippet(&request).unwrap();
        assert_eq!(snippet.language, "doc.image");
        assert!(snippet.text.contains(":: doc.image"));
    }
}
