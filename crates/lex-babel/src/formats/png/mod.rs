//! PNG export built on top of the HTML serializer + headless Chrome.
//!
//! Similar to PDF export, this renders Lex documents to HTML and uses
//! Chrome's screenshot capability to generate a PNG image.

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

/// Format implementation that shells out to Chrome/Chromium to generate PNGs.
#[derive(Default)]
pub struct PngFormat {
    html: HtmlFormat,
}

impl PngFormat {
    pub fn new() -> Self {
        Self {
            html: HtmlFormat::default(),
        }
    }
}

impl Format for PngFormat {
    fn name(&self) -> &str {
        "png"
    }

    fn description(&self) -> &str {
        "HTML-based PNG export via headless Chrome screenshot"
    }

    fn file_extensions(&self) -> &[&str] {
        &["png"]
    }

    fn supports_serialization(&self) -> bool {
        true
    }

    fn serialize(&self, _doc: &Document) -> Result<String, FormatError> {
        Err(FormatError::NotSupported(
            "PNG serialization produces binary output".to_string(),
        ))
    }

    fn serialize_with_options(
        &self,
        doc: &Document,
        options: &HashMap<String, String>,
    ) -> Result<SerializedDocument, FormatError> {
        let profile = PngSizeProfile::from_options(options)?;
        let html = self.html.serialize(doc)?;
        let final_html = inject_screenshot_css(&html, profile.css());
        let png_bytes = render_html_to_png(&final_html, profile)?;
        Ok(SerializedDocument::Binary(png_bytes))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngSizeProfile {
    QuickLook,
    LexEd,
    Mobile,
}

impl PngSizeProfile {
    fn from_options(options: &HashMap<String, String>) -> Result<Self, FormatError> {
        let quicklook = parse_bool_flag(options, "quicklook", false)?;
        let mobile = parse_bool_flag(options, "size-mobile", false)?;
        let lexed = parse_bool_flag(options, "size-lexed", false)?;

        let count = [quicklook, mobile, lexed].iter().filter(|&&x| x).count();
        if count > 1 {
            return Err(FormatError::SerializationError(
                "Cannot enable multiple PNG size profiles at once".to_string(),
            ));
        }

        if quicklook {
            Ok(PngSizeProfile::QuickLook)
        } else if mobile {
            Ok(PngSizeProfile::Mobile)
        } else {
            Ok(PngSizeProfile::LexEd)
        }
    }

    fn css(&self) -> &'static str {
        // Use system fonts for faster rendering (no web font loading delay)
        match self {
            PngSizeProfile::QuickLook => {
                concat!(
                    "body { margin: 20px; background: white; }\n",
                    ".lex-document { max-width: 600px; }\n",
                    "body, h1, h2, h3, h4, h5, h6 { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif !important; }\n",
                    "code, .lex-verbatim code { font-family: 'SF Mono', Menlo, Monaco, 'Courier New', monospace !important; }\n"
                )
            }
            PngSizeProfile::LexEd => {
                concat!(
                    "body { margin: 40px; background: white; }\n",
                    ".lex-document { max-width: 800px; }\n",
                    "body, h1, h2, h3, h4, h5, h6 { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif !important; }\n",
                    "code, .lex-verbatim code { font-family: 'SF Mono', Menlo, Monaco, 'Courier New', monospace !important; }\n"
                )
            }
            PngSizeProfile::Mobile => {
                concat!(
                    "body { margin: 10px; background: white; }\n",
                    ".lex-document { max-width: 350px; }\n",
                    "body, h1, h2, h3, h4, h5, h6 { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif !important; }\n",
                    "code, .lex-verbatim code { font-family: 'SF Mono', Menlo, Monaco, 'Courier New', monospace !important; }\n"
                )
            }
        }
    }

    fn viewport(&self) -> (u32, u32) {
        match self {
            PngSizeProfile::QuickLook => (680, 800),
            PngSizeProfile::LexEd => (1280, 960),
            PngSizeProfile::Mobile => (450, 900),
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

fn inject_screenshot_css(html: &str, css: &str) -> String {
    let style_tag = format!("<style data-lex-png>\n{css}\n</style>");
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

fn render_html_to_png(html: &str, profile: PngSizeProfile) -> Result<Vec<u8>, FormatError> {
    let chrome = resolve_chrome_binary()?;
    let temp_dir =
        tempdir().map_err(|e| FormatError::SerializationError(format!("Temp dir error: {e}")))?;
    let html_path = temp_dir.path().join("lex-export.html");
    let mut html_file =
        fs::File::create(&html_path).map_err(|e| FormatError::SerializationError(e.to_string()))?;
    html_file
        .write_all(html.as_bytes())
        .map_err(|e| FormatError::SerializationError(e.to_string()))?;

    let png_path = temp_dir.path().join("lex-export.png");
    let file_url = Url::from_file_path(&html_path).map_err(|_| {
        FormatError::SerializationError(
            "Failed to construct file:// URL for HTML input".to_string(),
        )
    })?;

    let screenshot_arg = format!("--screenshot={}", png_path.display());
    let window_arg = {
        let (w, h) = profile.viewport();
        format!("--window-size={w},{h}")
    };

    let status = Command::new(&chrome)
        .arg("--headless")
        .arg("--disable-gpu")
        .arg("--no-sandbox")
        .arg("--disable-dev-shm-usage")
        .arg("--hide-scrollbars")
        .arg(&screenshot_arg)
        .arg(&window_arg)
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

    fs::read(&png_path).map_err(|e| FormatError::SerializationError(e.to_string()))
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
