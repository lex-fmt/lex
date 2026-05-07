//! Document link extraction for LSP support
//!
//! This module provides APIs for extracting clickable links from Lex documents,
//! enabling the LSP "document links" feature that makes URLs and file references
//! clickable in editors.
//!
//! ## Problem
//!
//! The LSP document links feature needs to find all clickable links:
//! - URLs in text (`[https://example.com]`)
//! - File references (`[./file.txt]`)
//! - Verbatim block `src` parameters (images, includes)
//!
//! While `ReferenceType::Url` and `ReferenceType::File` exist, there's no API to
//! extract all links from a document.
//!
//! ## Solution
//!
//! This module provides:
//! - `DocumentLink` struct representing a link with its location and type
//! - `find_all_links()` methods on Document and Session
//! - `src_parameter()` method on Verbatim to access src parameters
//!
//! ## Link Types
//!
//! 1. **URL links**: `[https://example.com]` - HTTP/HTTPS URLs
//! 2. **File links**: `[./file.txt]`, `[../path/to/file.md]` - File references
//! 3. **Verbatim src**: `:: image src=./image.png ::` - External resource references

use super::elements::Verbatim;
use super::range::{Position, Range};
use super::text_content::TextContent;
use super::{Document, Session};
use crate::lex::inlines::{InlineNode, ReferenceInline, ReferenceType};
use std::fmt;

/// Represents a document link with its location and type
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentLink {
    pub range: Range,
    pub target: String,
    pub link_type: LinkType,
}

impl DocumentLink {
    pub fn new(range: Range, target: String, link_type: LinkType) -> Self {
        Self {
            range,
            target,
            link_type,
        }
    }
}

impl fmt::Display for DocumentLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} link: {} at {}",
            self.link_type, self.target, self.range.start
        )
    }
}

/// Type of document link
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkType {
    /// HTTP/HTTPS URL
    Url,
    /// File reference (relative or absolute path)
    File,
    /// Verbatim block src parameter
    VerbatimSrc,
}

impl Verbatim {
    /// Get the src parameter value if present
    ///
    /// The src parameter is commonly used for:
    /// - Image sources: `:: image src=./diagram.png ::`
    /// - File includes: `:: include src=./code.rs ::`
    /// - External resources: `:: data src=./data.csv ::`
    ///
    /// # Returns
    /// The value of the `src` parameter, or None if not present
    ///
    /// # Example
    /// ```rust,ignore
    /// if let Some(src) = verbatim.src_parameter() {
    ///     // Make src clickable in editor
    ///     println!("Link to: {}", src);
    /// }
    /// ```
    pub fn src_parameter(&self) -> Option<&str> {
        self.closing_data
            .parameters
            .iter()
            .find(|p| p.key == "src")
            .map(|p| p.value.as_str())
    }
}

impl Session {
    /// Find all links at any depth in this session
    ///
    /// This searches recursively through all content to find:
    /// - URL references: `[https://example.com]`
    /// - File references: `[./path/to/file.txt]`
    /// - Verbatim src parameters: `src=./image.png`
    ///
    /// # Returns
    /// Vector of all links found in this session and its descendants
    ///
    /// # Example
    /// ```rust,ignore
    /// let links = session.find_all_links();
    /// for link in links {
    ///     println!("Found {} link: {}", link.link_type, link.target);
    /// }
    /// ```
    pub fn find_all_links(&self) -> Vec<DocumentLink> {
        use super::elements::content_item::ContentItem;
        use super::traits::AstNode;

        let mut links = Vec::new();

        // Links in this session's title and every nested session's title.
        //
        // `Document::find_all_links` invokes us on the implicit root session
        // (whose title is empty), so without the recursive sweep below we
        // would silently drop every URL/File reference that appears in a
        // section heading — `1. See [./handlers.lex] for details` and
        // similar — even though paragraph-body refs were correctly found.
        collect_text_content_links(&self.title, &mut links);
        for nested in self.iter_sessions_recursive() {
            collect_text_content_links(&nested.title, &mut links);
        }

        // Paragraphs (recursively into nested sessions).
        for paragraph in self.iter_paragraphs_recursive() {
            for line_item in &paragraph.lines {
                if let ContentItem::TextLine(line) = line_item {
                    collect_text_content_links(&line.content, &mut links);
                }
            }
        }

        // Verbatim `src` parameters — these aren't bracketed inline references,
        // so the verbatim's range stays as-is.
        for (item, _depth) in self.iter_all_nodes_with_depth() {
            if let ContentItem::VerbatimBlock(verbatim) = item {
                if let Some(src) = verbatim.src_parameter() {
                    let link = DocumentLink::new(
                        verbatim.range().clone(),
                        src.to_string(),
                        LinkType::VerbatimSrc,
                    );
                    links.push(link);
                }
            }
        }

        links
    }
}

impl Document {
    /// Find all links in the entire document
    ///
    /// This searches the entire document tree to find all clickable links:
    /// - URL references in text
    /// - File references in text
    /// - Verbatim block src parameters
    ///
    /// # Returns
    /// Vector of all links found in the document
    ///
    /// # Example
    /// ```rust,ignore
    /// let doc = parse_document(source)?;
    /// let links = doc.find_all_links();
    /// for link in links {
    ///     // Make link clickable in LSP
    ///     send_document_link(link.range, link.target);
    /// }
    /// ```
    pub fn find_all_links(&self) -> Vec<DocumentLink> {
        let mut links = Vec::new();
        if let Some(title) = &self.title {
            collect_text_content_links(&title.content, &mut links);
        }
        links.extend(self.root.find_all_links());
        links
    }
}

/// Walks `text`'s inline tree and pushes a [`DocumentLink`] for each URL and
/// File reference, with a range covering exactly the `[bracketed]` reference.
///
/// LSP `textDocument/documentLink` ranges drive the clickable + visually
/// underlined area in editors. Using the containing paragraph or title range
/// would underline the whole element — which is exactly the bug this function
/// is replacing.
///
/// The cursor logic mirrors `lex-analysis::semantic_tokens::InlineWalker`
/// (which produces semantic-token ranges over the same raw text). The two
/// implementations must stay in sync; consolidating them into a single
/// inline-position walker in `lex-core` is a follow-up.
fn collect_text_content_links(text: &TextContent, out: &mut Vec<DocumentLink>) {
    let Some(base_range) = text.location.as_ref() else {
        return;
    };
    let Some(nodes) = text.inlines() else {
        return;
    };
    let raw = text.as_string();
    if raw.is_empty() {
        return;
    }
    let mut locator = ReferenceLocator {
        raw,
        base_range,
        cursor: 0,
    };
    locator.walk_nodes(nodes, out);
}

/// Cursor-tracking walker that produces precise byte/position ranges for
/// inline `Reference` nodes. Only emits links for URL and File reference
/// types; other types intentionally fall through (footnote/citation/session/
/// annotation/TK refs do not become document links).
struct ReferenceLocator<'a> {
    raw: &'a str,
    base_range: &'a Range,
    cursor: usize,
}

impl<'a> ReferenceLocator<'a> {
    fn walk_nodes(&mut self, nodes: &'a [InlineNode], out: &mut Vec<DocumentLink>) {
        for node in nodes {
            self.walk_node(node, out);
        }
    }

    fn walk_node(&mut self, node: &'a InlineNode, out: &mut Vec<DocumentLink>) {
        match node {
            InlineNode::Plain { text, .. } => self.advance_unescaped(text),
            InlineNode::Strong { content, .. } => self.walk_container(content, '*', out),
            InlineNode::Emphasis { content, .. } => self.walk_container(content, '_', out),
            InlineNode::Code { text, .. } => self.skip_literal(text, '`'),
            InlineNode::Math { text, .. } => self.skip_literal(text, '#'),
            InlineNode::Reference { data, .. } => self.emit_reference(data, out),
        }
    }

    fn walk_container(
        &mut self,
        content: &'a [InlineNode],
        marker: char,
        out: &mut Vec<DocumentLink>,
    ) {
        let m = marker.len_utf8();
        self.cursor += m;
        self.walk_nodes(content, out);
        self.cursor += m;
    }

    fn skip_literal(&mut self, text: &str, marker: char) {
        let m = marker.len_utf8();
        self.cursor += m;
        self.cursor += text.len();
        self.cursor += m;
    }

    fn emit_reference(&mut self, data: &'a ReferenceInline, out: &mut Vec<DocumentLink>) {
        let start = self.cursor;
        // `[` + content + `]`. Reference content is literal — no escape rules
        // apply, so the raw inline content length is the byte length.
        self.cursor += 1 + data.raw.len() + 1;
        let end = self.cursor;
        let (target, link_type) = match &data.reference_type {
            ReferenceType::Url { target } => (target.clone(), LinkType::Url),
            ReferenceType::File { target } => (target.clone(), LinkType::File),
            _ => return,
        };
        let range = self.make_range(start, end);
        out.push(DocumentLink::new(range, target, link_type));
    }

    /// Mirrors the inline parser's escape rules so cursor advances through
    /// raw bytes match each unescaped char emitted into a `Plain` node.
    /// `\X` where `X` is non-alphanumeric → consumes 2 raw bytes (`\` + `X`)
    /// for 1 unescaped char. Any other backslash stays literal.
    fn advance_unescaped(&mut self, text: &str) {
        for _expected in text.chars() {
            if self.cursor >= self.raw.len() {
                break;
            }
            let raw_ch = self.raw[self.cursor..].chars().next().unwrap();
            if raw_ch == '\\' {
                if self.cursor + 1 >= self.raw.len() {
                    self.cursor += 1;
                } else {
                    let next_ch = self.raw[self.cursor + 1..].chars().next();
                    match next_ch {
                        Some(nc) if !nc.is_alphanumeric() => {
                            self.cursor += 1 + nc.len_utf8();
                        }
                        _ => {
                            self.cursor += 1;
                        }
                    }
                }
            } else {
                self.cursor += raw_ch.len_utf8();
            }
        }
    }

    fn make_range(&self, start: usize, end: usize) -> Range {
        let start_pos = self.position_at(start);
        let end_pos = self.position_at(end);
        Range::new(
            (self.base_range.span.start + start)..(self.base_range.span.start + end),
            start_pos,
            end_pos,
        )
    }

    fn position_at(&self, offset: usize) -> Position {
        let mut line = self.base_range.start.line;
        let mut column = self.base_range.start.column;
        for ch in self.raw[..offset].chars() {
            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += ch.len_utf8();
            }
        }
        Position::new(line, column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::parsing::parse_document;

    #[test]
    fn test_url_link_extraction() {
        let source = "Check out [https://example.com] for more info.\n\n";
        let doc = parse_document(source).unwrap();

        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].link_type, LinkType::Url);
        assert_eq!(links[0].target, "https://example.com");
    }

    #[test]
    fn test_file_link_extraction() {
        let source = "See [./README.md] for details.\n\n";
        let doc = parse_document(source).unwrap();

        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].link_type, LinkType::File);
        assert_eq!(links[0].target, "./README.md");
    }

    #[test]
    fn test_multiple_links() {
        let source = "Visit [https://example.com] and check [./docs.md].\n\n";
        let doc = parse_document(source).unwrap();

        let links = doc.find_all_links();

        assert_eq!(links.len(), 2);
        assert!(links.iter().any(|l| l.link_type == LinkType::Url));
        assert!(links.iter().any(|l| l.link_type == LinkType::File));
    }

    #[test]
    fn test_verbatim_src_parameter() {
        let source =
            "Sunset Photo:\n    As the sun sets over the ocean.\n:: image src=./diagram.png ::\n\n";
        let doc = parse_document(source).unwrap();

        let links = doc.find_all_links();

        // Find verbatim src link
        let src_links: Vec<_> = links
            .iter()
            .filter(|l| l.link_type == LinkType::VerbatimSrc)
            .collect();
        assert_eq!(
            src_links.len(),
            1,
            "Expected 1 verbatim src link, found {}. All links: {:?}",
            src_links.len(),
            links
        );
        assert_eq!(src_links[0].target, "./diagram.png");
    }

    #[test]
    fn test_verbatim_src_parameter_method() {
        use super::super::elements::{Data, Label, Parameter};

        let verbatim = Verbatim::with_subject(
            "Test".to_string(),
            Data::new(
                Label::new("image".to_string()),
                vec![Parameter::new("src".to_string(), "./test.png".to_string())],
            ),
        );

        assert_eq!(verbatim.src_parameter(), Some("./test.png"));

        // Test verbatim without src parameter
        let verbatim_no_src = Verbatim::with_subject(
            "Test".to_string(),
            Data::new(Label::new("code".to_string()), vec![]),
        );

        assert_eq!(verbatim_no_src.src_parameter(), None);
    }

    #[test]
    fn test_no_links() {
        let source = "Just plain text with no links.\n\n";
        let doc = parse_document(source).unwrap();

        let links = doc.find_all_links();

        assert_eq!(links.len(), 0);
    }

    #[test]
    fn test_footnote_not_a_link() {
        let source = "Text with footnote [42].\n\n";
        let doc = parse_document(source).unwrap();

        let links = doc.find_all_links();

        // Footnote references are not clickable links
        assert_eq!(links.len(), 0);
    }

    #[test]
    fn test_nested_session_links() {
        let source = "Outer Session\n\n    Inner session with [https://example.com].\n\n";
        let doc = parse_document(source).unwrap();

        let links = doc.find_all_links();

        // Should find link in nested session
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "https://example.com");
    }

    // -----------------------------------------------------------------------
    // Range-precision tests
    //
    // The LSP `textDocument/documentLink` response uses each link's `range`
    // to decide what is clickable and what gets the link decoration in the
    // editor. Editors (notably VSCode) render the entire range as an
    // underlined link. So the range must cover *only* the `[bracketed]`
    // reference, not the surrounding paragraph or title line.
    // -----------------------------------------------------------------------

    use super::super::range::Position;

    #[test]
    fn test_url_link_range_is_bracket_bounded_in_paragraph() {
        // Byte map of source line:
        //   "Check out [https://example.com] for more info."
        //    0123456789^                   ^
        //              10                  30 (inclusive ']' position)
        let source = "Check out [https://example.com] for more info.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.target, "https://example.com");

        let captured = &source[link.range.span.clone()];
        assert_eq!(
            link.range.span,
            10..31,
            "DocumentLink range must cover only the [bracketed] reference, not the whole paragraph. \
             Captured text: {captured:?}"
        );
        assert_eq!(link.range.start, Position::new(0, 10));
        assert_eq!(link.range.end, Position::new(0, 31));
    }

    #[test]
    fn test_file_link_range_is_bracket_bounded_in_paragraph() {
        // Byte map of source line:
        //   "See [./README.md] for details."
        //    0123^         ^
        //        4         16 (inclusive ']')
        let source = "See [./README.md] for details.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.target, "./README.md");

        let captured = &source[link.range.span.clone()];
        assert_eq!(
            link.range.span,
            4..17,
            "DocumentLink range must cover only the [bracketed] reference, not the whole paragraph. \
             Captured text: {captured:?}"
        );
        assert_eq!(link.range.start, Position::new(0, 4));
        assert_eq!(link.range.end, Position::new(0, 17));
    }

    #[test]
    fn test_multiple_links_have_distinct_bracket_bounded_ranges() {
        // Byte map:
        //   "Visit [https://example.com] and check [./docs.md]."
        //    0     6                    27          38       49
        let source = "Visit [https://example.com] and check [./docs.md].\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 2);

        let url = links
            .iter()
            .find(|l| l.link_type == LinkType::Url)
            .expect("url link");
        let file = links
            .iter()
            .find(|l| l.link_type == LinkType::File)
            .expect("file link");

        assert_eq!(
            url.range.span,
            6..27,
            "URL link captured: {:?}",
            &source[url.range.span.clone()]
        );
        assert_eq!(
            file.range.span,
            38..49,
            "File link captured: {:?}",
            &source[file.range.span.clone()]
        );
    }

    #[test]
    fn test_long_paragraph_with_single_file_ref_does_not_include_surrounding_text_in_range() {
        // Reproduces the dodot architecture.lex case: a long paragraph that
        // contains a single file reference. Before the fix, the link's range
        // covered the whole paragraph so VSCode underlined every word.
        let source = "\
This document describes how dodot is organized. It is the conceptual view. \
For concrete types, crate layout, and trait signatures, see [./types.lex].\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.target, "./types.lex");

        let bracket_start = source.find("[./types.lex]").expect("bracket present");
        let bracket_end = bracket_start + "[./types.lex]".len();

        let captured = &source[link.range.span.clone()];
        assert_eq!(
            link.range.span,
            bracket_start..bracket_end,
            "Link range must be bracket-bounded. Got captured text: {captured:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Nested-session title coverage
    //
    // `Session::find_all_links` originally only inspected `self.title`, while
    // `Document::find_all_links` calls it on the implicit root session whose
    // title is empty. Paragraph traversal recurses into nested sessions, but
    // nested-session *titles* never get scanned. So URL/File refs that appear
    // in a section heading like
    //
    //     1. See [./handlers.lex] for the phase list
    //
    //         (body)
    //
    // were silently dropped from the LSP `documentLink` response, and editors
    // had no clickable surface on the heading.
    // -----------------------------------------------------------------------

    #[test]
    fn test_file_ref_in_nested_session_title_produces_link() {
        // "Doc title" + blank + indent → outer session whose title is
        // "Doc title". Then the indented "See [./other.lex] for details"
        // line, followed by a blank and a deeper indent, becomes a *nested*
        // session whose title contains a file reference.
        let source =
            "Doc title\n\n    See [./other.lex] for details\n\n        nested content here.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(
            links.len(),
            1,
            "expected one link for the file ref in the nested-session title; got {links:?}"
        );
        let link = &links[0];
        assert_eq!(link.target, "./other.lex");
        assert_eq!(link.link_type, LinkType::File);

        let bracket_start = source.find("[./other.lex]").expect("bracket present");
        let bracket_end = bracket_start + "[./other.lex]".len();
        assert_eq!(
            link.range.span,
            bracket_start..bracket_end,
            "Nested-session title link must be bracket-bounded. Got captured text: {:?}",
            &source[link.range.span.clone()]
        );
    }

    #[test]
    fn test_url_ref_in_nested_session_title_produces_link() {
        let source = "Doc title\n\n    Visit [https://example.com] today\n\n        body line.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.target, "https://example.com");
        assert_eq!(link.link_type, LinkType::Url);

        let bracket_start = source
            .find("[https://example.com]")
            .expect("bracket present");
        let bracket_end = bracket_start + "[https://example.com]".len();
        assert_eq!(link.range.span, bracket_start..bracket_end);
    }

    #[test]
    fn test_refs_in_both_outer_and_nested_session_titles_produce_links() {
        // The outer title also contains a file reference, so both the outer
        // and nested titles should each contribute one link, distinct from
        // any links found in paragraphs.
        let source = "\
Top [./top.lex] section

    Inner [./inner.lex] subsection

        See also [./body.lex] in the body.
";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(
            links.len(),
            3,
            "expected three links (outer-title, inner-title, body); got {links:?}"
        );
        let targets: Vec<&str> = links.iter().map(|l| l.target.as_str()).collect();
        assert!(targets.contains(&"./top.lex"));
        assert!(targets.contains(&"./inner.lex"));
        assert!(targets.contains(&"./body.lex"));
    }
}
