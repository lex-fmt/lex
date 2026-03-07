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
use super::range::Range;
use super::{Document, Session};
use crate::lex::inlines::{InlineNode, ReferenceType};
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

        // Check for links in the session title
        if let Some(inlines) = self.title.inlines() {
            for inline in inlines {
                if let InlineNode::Reference { data, .. } = inline {
                    match &data.reference_type {
                        ReferenceType::Url { target } => {
                            // Use header location if available, otherwise session location
                            let range = self.header_location().unwrap_or(&self.location).clone();
                            let link = DocumentLink::new(range, target.clone(), LinkType::Url);
                            links.push(link);
                        }
                        ReferenceType::File { target } => {
                            let range = self.header_location().unwrap_or(&self.location).clone();
                            let link = DocumentLink::new(range, target.clone(), LinkType::File);
                            links.push(link);
                        }
                        _ => {}
                    }
                }
            }
        }

        // Use existing iter_all_references() API to find URL and File references
        for paragraph in self.iter_paragraphs_recursive() {
            for line_item in &paragraph.lines {
                if let ContentItem::TextLine(line) = line_item {
                    // Use inlines() method which returns the parsed inlines without requiring mutable access
                    if let Some(inlines) = line.content.inlines() {
                        for inline in inlines {
                            if let InlineNode::Reference { data, .. } = inline {
                                match &data.reference_type {
                                    ReferenceType::Url { target } => {
                                        // Use paragraph's range since we don't have inline-level ranges yet
                                        let link = DocumentLink::new(
                                            paragraph.range().clone(),
                                            target.clone(),
                                            LinkType::Url,
                                        );
                                        links.push(link);
                                    }
                                    ReferenceType::File { target } => {
                                        let link = DocumentLink::new(
                                            paragraph.range().clone(),
                                            target.clone(),
                                            LinkType::File,
                                        );
                                        links.push(link);
                                    }
                                    _ => {
                                        // Other reference types are not clickable links
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Iterate all verbatim blocks to find src parameters
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
        self.root.find_all_links()
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
}
