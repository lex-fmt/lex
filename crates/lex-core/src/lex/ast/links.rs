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

use super::anchoring::{ReferenceAnchor, ReferenceLine};
use super::elements::Verbatim;
use super::inline_positions::{walk_text_content_positions, InlinePositionVisitor};
use super::range::{Position, Range};
use super::text_content::TextContent;
use super::{Document, Session};
use crate::lex::inlines::{AnchorDirection, ReferenceInline, ReferenceType, WordAnchor};
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

        // Reference lines (whole-element anchors and self-links) live outside
        // the structural tree — they are removed from the line stream before
        // parsing (see `crate::lex::anchoring`) and collected on the document.
        // Each becomes a `DocumentLink` whose range is the *anchored span*
        // (the head line for a whole-element anchor, the reference's own text
        // for a self-link), so editors underline/navigate the anchored text
        // rather than the bracketed reference.
        for ref_line in self.reference_lines() {
            collect_reference_line_link(ref_line, &mut links);
        }

        links
    }
}

/// Emit a [`DocumentLink`] for a reference line (§2.3.2). Only link-like Url/File
/// reference types become document links — Session/General reference types have
/// no navigable Url/File target today, mirroring the inline collector which only
/// surfaces Url/File. Marker-style types never reach here (they are not reference
/// lines).
///
/// - [`ReferenceAnchor::WholeElement`]: range = the anchored head line
///   (`anchor_range`); target = the reference's Url/File.
/// - [`ReferenceAnchor::SelfLink`]: range = the reference's own bracketed text
///   (`reference_range`); target = the reference's Url/File.
fn collect_reference_line_link(ref_line: &ReferenceLine, out: &mut Vec<DocumentLink>) {
    let (target, link_type) = match &ref_line.reference.reference_type {
        ReferenceType::Url { target } => (target.clone(), LinkType::Url),
        ReferenceType::File { target } => (target.clone(), LinkType::File),
        _ => return,
    };

    let range = match &ref_line.anchor {
        ReferenceAnchor::WholeElement { anchor_range, .. } => anchor_range.clone(),
        ReferenceAnchor::SelfLink => ref_line.reference_range.clone(),
    };

    out.push(DocumentLink::new(range, target, link_type));
}

/// Walks `text`'s inline tree and pushes a [`DocumentLink`] for each URL and
/// File reference, with a range covering exactly the `[bracketed]` reference.
///
/// LSP `textDocument/documentLink` ranges drive the clickable + visually
/// underlined area in editors. Using the containing paragraph or title range
/// would underline the whole element — which is exactly the bug this function
/// is replacing.
///
/// The cursor work is delegated to the shared
/// [`crate::lex::ast::inline_positions::walk_text_content_positions`] visitor;
/// this function only contributes the link-shaping logic in `LinkCollector`.
fn collect_text_content_links(text: &TextContent, out: &mut Vec<DocumentLink>) {
    let mut collector = LinkCollector::new(out);
    walk_text_content_positions(text, &mut collector);
}

/// Visitor that emits a [`DocumentLink`] per URL/File reference. All other
/// inline node variants are intentionally ignored (footnote/citation/session/
/// annotation/TK refs do not become document links).
///
/// ## Range widening for inline word anchors (§2.3.1)
///
/// An *inline* URL/File reference (one that shares its line with other text)
/// anchors a single word — the word immediately *preceding* it (default) or,
/// when it is the first token on the line, the word immediately *following* it.
/// The anchor-resolution pass records that word (and direction) on
/// `ReferenceInline::word_anchor`. To make the link underline/navigate the
/// *word* rather than the `[bracketed]` text, the collector computes the word's
/// source range from the adjacent `Plain` node:
///
/// - `Preceding`: the word lies at the end of the most-recently-visited `Plain`
///   node (`last_plain`), which is emitted before the reference. Computed
///   immediately.
/// - `Following`: the word lies at the start of the *next* `Plain` node, which
///   is visited after the reference. The reference is recorded as `pending` and
///   resolved when that `Plain` arrives.
///
/// The word stored on `WordAnchor` is *cleaned* (surrounding punctuation
/// trimmed) by the resolver; we locate that cleaned substring inside the plain
/// text so the range covers exactly the word, not its surrounding punctuation.
/// If the word cannot be located in the adjacent plain text (e.g. it was
/// flattened across a formatting span, or the plain node was escaped in a way
/// that shifts byte math), the link falls back to the `[bracketed]` range.
struct LinkCollector<'a> {
    out: &'a mut Vec<DocumentLink>,
    /// The most recently visited `Plain` node's range + text, used to resolve a
    /// `Preceding` word anchor (the word at the end of the text before a
    /// reference).
    last_plain: Option<PlainSpan>,
    /// A reference whose `Following` word anchor is waiting for the next `Plain`
    /// node to be visited so its start word can be located.
    pending_following: Option<PendingFollowing>,
}

/// A `Plain` inline node captured for word-anchor range computation: the node's
/// source range and the (unescaped) text the range covers.
struct PlainSpan {
    range: Range,
    text: String,
}

/// A reference awaiting the next `Plain` node to resolve a `Following` anchor.
struct PendingFollowing {
    word: String,
    target: String,
    link_type: LinkType,
    /// The bracket-bounded fallback range, used if the word can't be located.
    bracket_range: Range,
}

impl<'a> LinkCollector<'a> {
    fn new(out: &'a mut Vec<DocumentLink>) -> Self {
        Self {
            out,
            last_plain: None,
            pending_following: None,
        }
    }

    /// Bracket-bounded range of a reference: open marker start → close marker end.
    fn bracket_range(open_marker: &Range, close_marker: &Range) -> Range {
        Range::new(
            open_marker.span.start..close_marker.span.end,
            open_marker.start,
            close_marker.end,
        )
    }

    fn push(&mut self, range: Range, target: String, link_type: LinkType) {
        self.out.push(DocumentLink::new(range, target, link_type));
    }
}

impl<'a> InlinePositionVisitor for LinkCollector<'a> {
    fn visit_plain(&mut self, range: &Range, text: &str) {
        // Resolve any reference waiting on a following word first — this plain
        // node is the text that follows it.
        if let Some(pending) = self.pending_following.take() {
            let plain = PlainSpan {
                range: range.clone(),
                text: text.to_string(),
            };
            let resolved = locate_word_range(&plain, &pending.word, WordEnd::Start)
                .unwrap_or(pending.bracket_range);
            self.push(resolved, pending.target, pending.link_type);
        }
        self.last_plain = Some(PlainSpan {
            range: range.clone(),
            text: text.to_string(),
        });
    }

    fn visit_reference(
        &mut self,
        open_marker: &Range,
        _content: &Range,
        close_marker: &Range,
        data: &ReferenceInline,
    ) {
        let (target, link_type) = match &data.reference_type {
            ReferenceType::Url { target } => (target.clone(), LinkType::Url),
            ReferenceType::File { target } => (target.clone(), LinkType::File),
            _ => return,
        };
        let bracket_range = Self::bracket_range(open_marker, close_marker);

        match &data.word_anchor {
            // Inline reference anchoring the preceding word: resolve against the
            // last plain node we visited.
            Some(WordAnchor {
                word,
                direction: AnchorDirection::Preceding,
            }) => {
                let range = self
                    .last_plain
                    .as_ref()
                    .and_then(|plain| locate_word_range(plain, word, WordEnd::End))
                    .unwrap_or(bracket_range);
                self.push(range, target, link_type);
            }
            // Inline reference anchoring the following word: defer until the
            // next plain node is visited.
            Some(WordAnchor {
                word,
                direction: AnchorDirection::Following,
            }) => {
                self.pending_following = Some(PendingFollowing {
                    word: word.clone(),
                    target,
                    link_type,
                    bracket_range,
                });
            }
            // No word anchor (reference lines are handled separately; a lone
            // marker reference has no word). Fall back to the bracket range.
            None => {
                self.push(bracket_range, target, link_type);
            }
        }
    }
}

/// Which end of the plain text the anchored word sits at.
#[derive(Clone, Copy)]
enum WordEnd {
    /// The word is the *last* whitespace-delimited token (preceding anchor).
    End,
    /// The word is the *first* whitespace-delimited token (following anchor).
    Start,
}

/// Compute the source [`Range`] of `word` within `plain`, looking at the
/// appropriate end of the plain text.
///
/// `word` is the *cleaned* anchor word (surrounding punctuation already trimmed
/// by the resolver). We find the matching whitespace-delimited token at the
/// requested end, then locate the cleaned word inside it so trailing/leading
/// punctuation (`website,` → `website`) is excluded from the range.
///
/// Returns `None` (caller falls back to the bracket range) when the word can't
/// be located — e.g. the anchor was flattened across a formatting span, so the
/// adjacent plain node doesn't literally contain it.
fn locate_word_range(plain: &PlainSpan, word: &str, end: WordEnd) -> Option<Range> {
    let text = &plain.text;
    // The token at the requested end, with its byte offset within `text`.
    let token = match end {
        WordEnd::End => last_token(text),
        WordEnd::Start => first_token(text),
    }?;
    // Locate the cleaned word inside the token (punctuation trimmed). The token
    // contains the word as a contiguous substring (cleaning only strips leading
    // and trailing chars), so a single `find` recovers its offset.
    let word_in_token = token.text.find(word)?;
    let word_start = token.offset + word_in_token;
    let word_end = word_start + word.len();

    // Map byte offsets within the plain text to source coordinates. The plain
    // node is single-line (inline parsing is per-line), so the source byte span
    // is the plain node's span offset by these byte positions, and columns
    // advance from the plain node's start column. This holds when the plain
    // text's bytes line up 1:1 with the source (the common, escape-free case);
    // if an escape shifted the bytes, `find` would still give a plausible
    // offset but the column math could drift — acceptable since the worst case
    // is a slightly-off underline, and callers can fall back to brackets.
    let base = &plain.range;
    let span = (base.span.start + word_start)..(base.span.start + word_end);
    let start_col = base.start.column + utf16_width(&text[..word_start]);
    let end_col = base.start.column + utf16_width(&text[..word_end]);
    Some(Range::new(
        span,
        Position::new(base.start.line, start_col),
        Position::new(base.start.line, end_col),
    ))
}

/// A whitespace-delimited token with its byte offset within the parent text.
struct Token<'a> {
    text: &'a str,
    offset: usize,
}

/// The last whitespace-delimited token of `text`, with its byte offset.
fn last_token(text: &str) -> Option<Token<'_>> {
    let tok = text.split_whitespace().next_back()?;
    // `split_whitespace` doesn't give offsets; the last token ends at the last
    // non-whitespace byte, so find it from the trimmed end.
    let trimmed_end = text.trim_end().len();
    let offset = trimmed_end - tok.len();
    Some(Token { text: tok, offset })
}

/// The first whitespace-delimited token of `text`, with its byte offset.
fn first_token(text: &str) -> Option<Token<'_>> {
    let tok = text.split_whitespace().next()?;
    let offset = text.len() - text.trim_start().len();
    Some(Token { text: tok, offset })
}

/// UTF-16 code-unit width of `s` — matches the column units used by the inline
/// position walker (LSP default `positionEncoding`).
fn utf16_width(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
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
    // Range-precision tests (inline word anchors, §2.3.1)
    //
    // The LSP `textDocument/documentLink` response uses each link's `range`
    // to decide what is clickable and what gets the link decoration in the
    // editor. Editors (notably VSCode) render the entire range as an
    // underlined link.
    //
    // An *inline* reference (one that shares its line with other text) anchors
    // a single word — the word immediately *preceding* it by default. So the
    // link range covers that anchored word, not the `[bracketed]` reference and
    // not the surrounding paragraph. This is the PR-C widening: editors now
    // underline/navigate the word, matching how the reference renders.
    // -----------------------------------------------------------------------

    use super::super::range::Position;

    #[test]
    fn test_url_link_range_covers_preceding_word_in_paragraph() {
        // "Check out [https://example.com] for more info."
        //  0123456789^
        // The reference shares its line with text, so it anchors the preceding
        // word "out" (bytes 6..9), not the brackets.
        let source = "Check out [https://example.com] for more info.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.target, "https://example.com");

        let captured = &source[link.range.span.clone()];
        assert_eq!(
            link.range.span,
            6..9,
            "inline link range must cover the anchored word 'out'. Captured: {captured:?}"
        );
        assert_eq!(captured, "out");
        assert_eq!(link.range.start, Position::new(0, 6));
        assert_eq!(link.range.end, Position::new(0, 9));
    }

    #[test]
    fn test_file_link_range_covers_preceding_word_in_paragraph() {
        // "See [./README.md] for details." → anchors the preceding word "See".
        let source = "See [./README.md] for details.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.target, "./README.md");

        let captured = &source[link.range.span.clone()];
        assert_eq!(
            link.range.span,
            0..3,
            "inline link range must cover the anchored word 'See'. Captured: {captured:?}"
        );
        assert_eq!(captured, "See");
        assert_eq!(link.range.start, Position::new(0, 0));
        assert_eq!(link.range.end, Position::new(0, 3));
    }

    #[test]
    fn test_following_word_anchor_range() {
        // First-on-line reference anchors the *following* word "is".
        // "[https://lex.ing] is the home page."
        let source = "[https://lex.ing] is the home page.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.target, "https://lex.ing");

        let captured = &source[link.range.span.clone()];
        assert_eq!(captured, "is", "following-anchor link must cover 'is'");
        let is_start = source.find("is").unwrap();
        assert_eq!(link.range.span, is_start..is_start + 2);
    }

    #[test]
    fn test_word_anchor_excludes_trailing_punctuation() {
        // The preceding token is "website," but the anchor word is "website"
        // (punctuation trimmed), so the range must exclude the comma.
        let source = "the project website, [https://x.example] is fast.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let captured = &source[links[0].range.span.clone()];
        assert_eq!(captured, "website", "range must exclude the trailing comma");
    }

    #[test]
    fn test_abutting_word_anchor_range() {
        // "Hello[./file.txt] World" → abutting preceding word "Hello".
        let source = "Hello[./file.txt] World\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let captured = &source[links[0].range.span.clone()];
        assert_eq!(captured, "Hello");
    }

    #[test]
    fn test_multiple_links_anchor_distinct_words() {
        // "Visit [https://example.com] and check [./docs.md]."
        // URL anchors "Visit", file anchors "check".
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

        assert_eq!(&source[url.range.span.clone()], "Visit");
        assert_eq!(&source[file.range.span.clone()], "check");
    }

    #[test]
    fn test_long_paragraph_with_single_file_ref_anchors_only_the_word() {
        // Reproduces the dodot architecture.lex case: a long paragraph that
        // contains a single file reference. The link's range covers only the
        // anchored word "see", never the whole paragraph.
        let source = "\
This document describes how dodot is organized. It is the conceptual view. \
For concrete types, crate layout, and trait signatures, see [./types.lex].\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.target, "./types.lex");

        let captured = &source[link.range.span.clone()];
        assert_eq!(
            captured, "see",
            "inline link range must cover only the anchored word, not the paragraph"
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

        // Inline reference in the title → anchors the preceding word "See".
        assert_eq!(
            &source[link.range.span.clone()],
            "See",
            "nested-session title link anchors the preceding word"
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

        // Inline reference in the title → anchors the preceding word "Visit".
        assert_eq!(&source[link.range.span.clone()], "Visit");
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

    // -----------------------------------------------------------------------
    // Reference lines: whole-element anchors and self-links (§2.3.2)
    //
    // A reference line (`[ref]` alone on its line) is removed from the
    // structural stream and collected on the document. PR C surfaces it as a
    // `DocumentLink` whose range is the *anchored span*: the head line of the
    // element above (whole-element anchor), or the reference's own text when
    // there is no content line above (self-link). These prove the LSP emits a
    // standard range + target — the editor needs no special handling.
    // -----------------------------------------------------------------------

    #[test]
    fn test_reference_line_whole_element_anchors_session_title() {
        // The reference line anchors the entire session title "Getting Started".
        let source = "Getting Started\n[./readme.txt]\n\n    Welcome to the docs.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1, "one whole-element link; got {links:?}");
        let link = &links[0];
        assert_eq!(link.target, "./readme.txt");
        assert_eq!(link.link_type, LinkType::File);

        // Range covers the head line, not the `[./readme.txt]` reference line.
        assert_eq!(
            &source[link.range.span.clone()],
            "Getting Started",
            "whole-element link must cover the anchored head line"
        );
        // Positions point at the title line (line 0), full width.
        assert_eq!(link.range.start, Position::new(0, 0));
        assert_eq!(link.range.end, Position::new(0, "Getting Started".len()));
    }

    #[test]
    fn test_reference_line_whole_element_anchors_list_item() {
        // Anchors the whole "Water" list item; the `- ` marker is excluded.
        let source = "- Food\n- Water\n[https://water.example]\n- Bread\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.target, "https://water.example");
        assert_eq!(link.link_type, LinkType::Url);
        assert_eq!(
            &source[link.range.span.clone()],
            "Water",
            "list-item anchor excludes the `- ` marker"
        );
    }

    #[test]
    fn test_reference_line_whole_element_anchors_definition_subject() {
        // Anchors the definition term "API Endpoint"; the trailing `:` excluded.
        let source =
            "API Endpoint:\n[./endpoint.txt]\n    A URL that provides access to a resource.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.target, "./endpoint.txt");
        assert_eq!(
            &source[link.range.span.clone()],
            "API Endpoint",
            "subject anchor excludes the trailing colon"
        );
    }

    #[test]
    fn test_reference_line_self_link_range_is_the_reference_text() {
        // No content line directly above (blank line above) → self-link. The
        // link covers the reference's own `[bracketed]` text.
        let source = "See the upstream project:\n\n[https://github.com/lex-fmt/lex]\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1, "one self-link; got {links:?}");
        let link = &links[0];
        assert_eq!(link.target, "https://github.com/lex-fmt/lex");
        assert_eq!(
            &source[link.range.span.clone()],
            "[https://github.com/lex-fmt/lex]",
            "self-link covers the reference's own bracketed text"
        );
    }

    #[test]
    fn test_reference_line_self_link_at_start_of_document() {
        // First line of the document → no content above → self-link.
        let source = "[https://lex.ing]\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();

        assert_eq!(links.len(), 1);
        assert_eq!(
            &source[links[0].range.span.clone()],
            "[https://lex.ing]",
            "self-link covers the reference's own bracketed text"
        );
    }

    #[test]
    fn test_marker_reference_line_is_not_a_document_link() {
        // A footnote on its own line is a marker-style reference: not a
        // reference line and not a document link.
        let source = "Some claim.\n[42]\n\n:: 42 :: A footnote.\n\n";
        let doc = parse_document(source).unwrap();
        let links = doc.find_all_links();
        assert!(
            links.is_empty(),
            "marker-style references are not document links: {links:?}"
        );
    }
}
