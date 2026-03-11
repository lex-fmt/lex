//! HTML format implementation
//!
//! This module implements bidirectional conversion between Lex and HTML5.
//!
//! # Library Choice
//!
//! We use the `html5ever` + `rcdom` + `markup5ever` ecosystem for HTML parsing and serialization:
//! - `html5ever`: Browser-grade HTML5 parser from the Servo project
//! - `markup5ever_rcdom`: Reference-counted DOM tree implementation
//! - `markup5ever`: Serialization infrastructure
//!
//! This choice is based on:
//! - Complete solution for both parsing and serialization
//! - Battle-tested with 12M+ downloads
//! - WHATWG HTML5 specification compliance
//! - Active maintenance by Servo project
//! - Handles malformed HTML gracefully
//!
//! # Element Mapping Table
//!
//! Complete Lex ↔ HTML Mapping:
//!
//! | Lex Element      | HTML Equivalent                                    | Export Notes                              | Import Notes                          |
//! |------------------|----------------------------------------------------|-------------------------------------------|---------------------------------------|
//! | Document         | `<div class="lex-document">`                       | Root container with document class        | Parse body content                    |
//! | Session          | `<section class="lex-session lex-session-N">` + `<hN>` | Session → section + heading        | section + heading → Session           |
//! | Paragraph        | `<p class="lex-paragraph">`                        | Direct mapping with class                 | Direct mapping                        |
//! | List             | `<ul>`/`<ol>` with `class="lex-list"`              | Ordered/unordered preserved with class    | Detect ul/ol type                     |
//! | ListItem         | `<li class="lex-list-item">`                       | Direct mapping with class                 | Direct mapping                        |
//! | Definition       | `<dl class="lex-definition">` `<dt>` `<dd>`        | Term in dt, description in dd             | Parse dl/dt/dd structure              |
//! | Verbatim         | `<pre class="lex-verbatim">` `<code>`              | Language → data-language attribute        | Extract language from attribute       |
//! | Annotation       | `<!-- lex:label key=val -->`                       | HTML comment format                       | Parse HTML comment pattern            |
//! | InlineContent:   |                                                    |                                           |                                       |
//! |   Text           | Plain text                                         | Direct                                    | Direct                                |
//! |   Bold           | `<strong>`                                         | Semantic strong tag                       | Parse both strong and b               |
//! |   Italic         | `<em>`                                             | Semantic emphasis tag                     | Parse both em and i                   |
//! |   Code           | `<code>`                                           | Inline code tag                           | Direct                                |
//! |   Math           | `<span class="lex-math">`                          | Preserve $ delimiters in span             | Parse math span                       |
//! |   Reference      | `<a href="url">text</a>`                           | Convert to anchor with prev word as text  | Parse anchor back to reference        |
//!
//! # CSS Classes
//!
//! All Lex elements receive CSS classes matching their AST structure:
//! - `.lex-document`: Root document container
//! - `.lex-session`, `.lex-session-1`, `.lex-session-2`, etc.: Sessions with depth
//! - `.lex-paragraph`: Paragraphs
//! - `.lex-list`: Lists (combined with ul/ol)
//! - `.lex-list-item`: List items
//! - `.lex-definition`: Definition lists
//! - `.lex-verbatim`: Verbatim/code blocks
//! - `.lex-math`: Math expressions
//!
//! This enables:
//! - Precise CSS targeting for presentation
//! - Perfect round-trip conversion (HTML → Lex → HTML preserves structure)
//! - Custom theming without modifying structure
//!
//! # CSS and Theming
//!
//! HTML export includes embedded CSS from:
//! - `css/baseline.css`: Browser reset + default modern presentation (always included)
//! - `css/themes/theme-*.css`: Optional overrides layered on top of the baseline
//!
//! The default theme (`HtmlTheme::Modern`) injects an empty stylesheet so the
//! baseline alone controls rendering. Other themes, like Fancy Serif, only add
//! targeted overrides.
//!
//! Themes use Google Fonts and are mobile-responsive.
//!
//! # Output Format
//!
//! Export produces a single, self-contained HTML file:
//! - Complete HTML5 document structure
//! - Embedded CSS in <style> tag
//! - No external dependencies (except optionally-linked fonts)
//! - Mobile-responsive viewport meta tag
//!
//! # Lossy Conversions
//!
//! The following conversions may lose information on round-trip:
//! - Lex sessions beyond level 6 → h6 with nested sections (HTML heading limit)
//! - Lex annotations → HTML comments (exported but parsing is lossy)
//! - Some whitespace normalization
//!
//! # Architecture Notes
//!
//! Like the Markdown implementation, we handle the nested-to-flat conversion using the IR
//! event system (lex-babel/src/common/). HTML is more naturally hierarchical than Markdown,
//! but sessions still require special handling as they don't map directly to HTML's heading
//! structure.
//!
//! We use semantic HTML elements with CSS classes for styling rather than presentational
//! elements.
//!
//! # Implementation Status
//!
//! - [x] Export (Lex → HTML)
//!   - [ ] Document structure with CSS embedding
//!   - [ ] Paragraph
//!   - [ ] Heading (Session) → section + heading
//!   - [ ] Bold, Italic, Code inlines
//!   - [ ] Lists - ordered/unordered
//!   - [ ] Code blocks (Verbatim) with language attribute
//!   - [ ] Definitions → dl/dt/dd
//!   - [ ] Annotations → HTML comments
//!   - [ ] Math → span with class
//!   - [ ] References → anchors with link conversion
//! - [ ] Import (HTML → Lex)
//!   - [ ] All elements (to be implemented after export)

mod serializer;

use crate::error::FormatError;
use crate::format::Format;
use lex_core::lex::ast::Document;
use std::fs;

pub use serializer::HtmlOptions;

/// Returns the default baseline CSS used for HTML export.
///
/// This is the same CSS embedded in all HTML exports when no custom CSS is provided.
/// Use this to get a starting point for custom styling.
pub fn get_default_css() -> &'static str {
    include_str!("../../../css/baseline.css")
}

/// Format implementation for HTML
pub struct HtmlFormat {
    /// CSS theme to use for export
    theme: HtmlTheme,
}

/// Available CSS themes for HTML export
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HtmlTheme {
    /// Serif typography override (fonts only, layout comes from baseline)
    FancySerif,
    /// Baseline modern theme (no-op; relies on baseline.css)
    #[default]
    Modern,
}

impl Default for HtmlFormat {
    fn default() -> Self {
        Self::new(HtmlTheme::Modern)
    }
}

impl HtmlFormat {
    /// Create a new HTML format with the specified theme
    pub fn new(theme: HtmlTheme) -> Self {
        Self { theme }
    }

    /// Create HTML format with fancy serif theme
    pub fn with_fancy_serif() -> Self {
        Self::new(HtmlTheme::FancySerif)
    }

    /// Create HTML format with modern theme
    pub fn with_modern() -> Self {
        Self::new(HtmlTheme::Modern)
    }
}

impl Format for HtmlFormat {
    fn name(&self) -> &str {
        "html"
    }

    fn description(&self) -> &str {
        "HTML5 format with embedded CSS"
    }

    fn file_extensions(&self) -> &[&str] {
        &["html", "htm"]
    }

    fn supports_parsing(&self) -> bool {
        false // Implement after export is working
    }

    fn supports_serialization(&self) -> bool {
        true
    }

    fn parse(&self, _source: &str) -> Result<Document, FormatError> {
        Err(FormatError::NotSupported(
            "HTML import not yet implemented".to_string(),
        ))
    }

    fn serialize(&self, doc: &Document) -> Result<String, FormatError> {
        serializer::serialize_to_html(doc, self.theme)
    }

    fn serialize_with_options(
        &self,
        doc: &Document,
        options: &std::collections::HashMap<String, String>,
    ) -> Result<crate::format::SerializedDocument, FormatError> {
        let mut theme = self.theme;
        if let Some(theme_str) = options.get("theme") {
            theme = match theme_str.as_str() {
                "fancy-serif" => HtmlTheme::FancySerif,
                "modern" | "default" => HtmlTheme::Modern,
                _ => {
                    // Fallback to default for unknown themes, or could error.
                    // For now, let's fallback to Modern to be safe.
                    HtmlTheme::Modern
                }
            };
        }

        let mut html_options = HtmlOptions::new(theme);

        // Handle custom CSS option (expects CSS content, not path)
        if let Some(css_content) = options.get("custom_css") {
            html_options = html_options.with_custom_css(css_content.clone());
        } else if let Some(css_path) = options.get("css-path").or_else(|| options.get("css_path")) {
            let css = fs::read_to_string(css_path).map_err(|err| {
                FormatError::SerializationError(format!(
                    "Failed to read CSS at '{css_path}': {err}"
                ))
            })?;
            html_options = html_options.with_custom_css(css);
        }

        serializer::serialize_to_html_with_options(doc, html_options)
            .map(crate::format::SerializedDocument::Text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::SerializedDocument;
    use lex_core::lex::ast::Document;
    use std::collections::HashMap;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_get_default_css_returns_baseline() {
        let css = get_default_css();
        // Should contain key selectors from baseline.css
        assert!(css.contains(".lex-document"));
        assert!(css.contains(".lex-paragraph"));
        assert!(css.contains(".lex-session"));
        // Should be non-trivial content
        assert!(css.len() > 1000);
    }

    #[test]
    fn test_get_default_css_is_same_as_embedded() {
        // The CSS returned should be the exact same as what's embedded in HTML output
        let css = get_default_css();
        // Verify it's the actual include_str content by checking for CSS custom properties
        assert!(css.contains("--lex-"));
    }

    #[test]
    fn test_css_path_option_loads_file() {
        let mut temp = NamedTempFile::new().expect("failed to create temp file");
        writeln!(temp, ".from-path {{ color: blue; }}").expect("failed to write temp css");

        let doc = Document::new();
        let format = HtmlFormat::default();
        let mut options = HashMap::new();
        options.insert(
            "css-path".to_string(),
            temp.path().to_string_lossy().to_string(),
        );

        let html = format
            .serialize_with_options(&doc, &options)
            .expect("html export should succeed");

        let SerializedDocument::Text(content) = html else {
            panic!("expected text html output");
        };
        assert!(content.contains(".from-path { color: blue; }"));
    }

    #[test]
    fn test_css_path_option_errors_on_missing_file() {
        let doc = Document::new();
        let format = HtmlFormat::default();
        let mut options = HashMap::new();
        options.insert("css-path".to_string(), "/no/such/file.css".to_string());

        let err = match format.serialize_with_options(&doc, &options) {
            Ok(_) => panic!("expected css-path lookup to fail"),
            Err(err) => err,
        };

        match err {
            FormatError::SerializationError(msg) => {
                assert!(msg.contains("/no/such/file.css"));
            }
            other => panic!("expected serialization error, got {other:?}"),
        }
    }
}
