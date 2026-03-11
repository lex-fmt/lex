//! PDF export built on top of the HTML serializer + headless Chrome.
//!
//! The implementation renders Lex documents to HTML using the existing HTML
//! format, injects page-size specific CSS, then shells out to a Chrome/Chromium
//! binary running in headless mode to print the page to PDF.

use crate::error::FormatError;
use crate::format::{Format, SerializedDocument};
use crate::formats::html::HtmlFormat;
use lex_core::lex::ast::Document;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;
use url::Url;
use which::which;

/// Format implementation that shells out to Chrome/Chromium to generate PDFs.
#[derive(Default)]
pub struct PdfFormat {
    html: HtmlFormat,
}

impl PdfFormat {
    pub fn new() -> Self {
        Self {
            html: HtmlFormat::default(),
        }
    }
}

impl Format for PdfFormat {
    fn name(&self) -> &str {
        "pdf"
    }

    fn description(&self) -> &str {
        "HTML-based PDF export via headless Chrome"
    }

    fn file_extensions(&self) -> &[&str] {
        &["pdf"]
    }

    fn supports_serialization(&self) -> bool {
        true
    }

    fn serialize(&self, _doc: &Document) -> Result<String, FormatError> {
        Err(FormatError::NotSupported(
            "PDF serialization produces binary output".to_string(),
        ))
    }

    fn serialize_with_options(
        &self,
        doc: &Document,
        options: &HashMap<String, String>,
    ) -> Result<SerializedDocument, FormatError> {
        let profile = PdfSizeProfile::from_options(options)?;
        let html = self.html.serialize(doc)?;
        let final_html = inject_page_css(&html, profile.print_css());
        let pdf_bytes = render_html_to_pdf(&final_html, profile)?;
        Ok(SerializedDocument::Binary(pdf_bytes))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PdfSizeProfile {
    LexEd,
    Mobile,
}

impl PdfSizeProfile {
    fn from_options(options: &HashMap<String, String>) -> Result<Self, FormatError> {
        let mobile = parse_bool_flag(options, "size-mobile", false)?;
        let lexed = parse_bool_flag(options, "size-lexed", !mobile)?;

        if mobile && lexed {
            return Err(FormatError::SerializationError(
                "Cannot enable both lexed and mobile PDF sizing at once".to_string(),
            ));
        }

        if mobile {
            Ok(PdfSizeProfile::Mobile)
        } else {
            Ok(PdfSizeProfile::LexEd)
        }
    }

    fn print_css(&self) -> &'static str {
        match self {
            PdfSizeProfile::LexEd =>
                "@page { size: 210mm 297mm; margin: 18mm; }\nbody { margin: 0; }\n",
            PdfSizeProfile::Mobile =>
                "@page { size: 90mm 160mm; margin: 5mm; }\nbody { margin: 0; }\n.lex-document { max-width: calc(90mm - 10mm); }\n",
        }
    }

    fn viewport(&self) -> (u32, u32) {
        match self {
            PdfSizeProfile::LexEd => (1280, 960),
            PdfSizeProfile::Mobile => (450, 900),
        }
    }
}

fn parse_bool_flag(
    options: &HashMap<String, String>,
    key: &str,
    default: bool,
) -> Result<bool, FormatError> {
    if let Some(value) = options.get(key) {
        if value.is_empty() {
            return Ok(true);
        }
        match value.to_lowercase().as_str() {
            "true" | "1" | "yes" | "y" => Ok(true),
            "false" | "0" | "no" | "n" => Ok(false),
            other => Err(FormatError::SerializationError(format!(
                "Invalid boolean value '{other}' for --extra-{key}"
            ))),
        }
    } else {
        Ok(default)
    }
}

fn inject_page_css(html: &str, css: &str) -> String {
    let style_tag = format!("<style data-lex-pdf>\n{css}\n</style>");
    if let Some(idx) = html.find("</head>") {
        let mut output = String::with_capacity(html.len() + style_tag.len());
        output.push_str(&html[..idx]);
        output.push_str(&style_tag);
        output.push_str(&html[idx..]);
        output
    } else {
        format!("{style_tag}{html}")
    }
}

fn render_html_to_pdf(html: &str, profile: PdfSizeProfile) -> Result<Vec<u8>, FormatError> {
    let chrome = resolve_chrome_binary()?;
    let temp_dir =
        tempdir().map_err(|e| FormatError::SerializationError(format!("Temp dir error: {e}")))?;
    let html_path = temp_dir.path().join("lex-export.html");
    let mut html_file =
        fs::File::create(&html_path).map_err(|e| FormatError::SerializationError(e.to_string()))?;
    html_file
        .write_all(html.as_bytes())
        .map_err(|e| FormatError::SerializationError(e.to_string()))?;

    let pdf_path = temp_dir.path().join("lex-export.pdf");
    let file_url = Url::from_file_path(&html_path).map_err(|_| {
        FormatError::SerializationError(
            "Failed to construct file:// URL for HTML input".to_string(),
        )
    })?;

    let pdf_arg = format!("--print-to-pdf={}", pdf_path.display());
    let window_arg = {
        let (w, h) = profile.viewport();
        format!("--window-size={w},{h}")
    };

    let status = Command::new(&chrome)
        .arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--no-sandbox")
        .arg("--disable-dev-shm-usage")
        .arg("--no-pdf-header-footer")
        .arg(pdf_arg)
        .arg(window_arg)
        .arg(file_url.as_str())
        .status()
        .map_err(|e| {
            FormatError::SerializationError(format!(
                "Failed to launch Chrome ({}): {}",
                chrome.display(),
                e
            ))
        })?;

    if !status.success() {
        return Err(FormatError::SerializationError(format!(
            "Chrome exited with status {status}"
        )));
    }

    fs::read(&pdf_path).map_err(|e| FormatError::SerializationError(e.to_string()))
}

fn resolve_chrome_binary() -> Result<PathBuf, FormatError> {
    if let Some(path) = env::var_os("LEX_CHROME_BIN") {
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    for var in ["GOOGLE_CHROME_BIN", "CHROME_BIN"] {
        if let Some(path) = env::var_os(var) {
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    for candidate in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
        "msedge",
    ] {
        if let Ok(path) = which(candidate) {
            return Ok(path);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let default_path = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
        let candidate = PathBuf::from(default_path);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let candidates = [
            r"C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
            r"C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
        ];
        for candidate in candidates {
            let path = PathBuf::from(candidate);
            if path.exists() {
                return Ok(path);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let candidates = [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium-browser",
            "/usr/bin/chromium",
        ];
        for candidate in candidates {
            let path = PathBuf::from(candidate);
            if path.exists() {
                return Ok(path);
            }
        }
    }

    Err(FormatError::SerializationError(
        "Unable to locate a Chrome/Chromium binary. Set LEX_CHROME_BIN to override the detection."
            .to_string(),
    ))
}
