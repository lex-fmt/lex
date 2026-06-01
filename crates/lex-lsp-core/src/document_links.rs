use lex_core::lex::ast::links::DocumentLink as AstDocumentLink;
use lex_core::lex::ast::Document;

pub fn collect_document_links(document: &Document) -> Vec<AstDocumentLink> {
    document.find_all_links()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::parsing;

    #[test]
    fn collects_links() {
        let source = "Visit [https://example.com] and include [./data.csv].\n";
        let document = parsing::parse_document(source).expect("parse fixture");
        let links = collect_document_links(&document);
        assert_eq!(links.len(), 2);
    }

    // Reference-anchoring (PR C): the document-link range is the resolved
    // *anchor span*, not the brackets. These assert the range the LSP feature
    // provider hands to `build_document_link` for each anchor shape, proving the
    // server only ever emits a standard `range + target` — no editor-side
    // special handling is needed for any case.

    fn sole_link(source: &str) -> AstDocumentLink {
        let document = parsing::parse_document(source).expect("parse fixture");
        let mut links = collect_document_links(&document);
        assert_eq!(links.len(), 1, "expected exactly one link: {links:?}");
        links.remove(0)
    }

    #[test]
    fn whole_element_anchor_range_is_the_head_line() {
        // Reference line under a session title → range covers the title.
        let source = "Getting Started\n[./readme.txt]\n\n    Body.\n\n";
        let link = sole_link(source);
        assert_eq!(link.target, "./readme.txt");
        assert_eq!(&source[link.range.span.clone()], "Getting Started");
    }

    #[test]
    fn self_link_range_is_the_reference_text() {
        // Reference line with a blank line above → self-link, range = its text.
        let source = "Upstream:\n\n[https://github.com/lex-fmt/lex]\n\n";
        let link = sole_link(source);
        assert_eq!(link.target, "https://github.com/lex-fmt/lex");
        assert_eq!(
            &source[link.range.span.clone()],
            "[https://github.com/lex-fmt/lex]"
        );
    }

    #[test]
    fn inline_word_anchor_range_is_the_word() {
        // Inline reference → range covers the anchored preceding word.
        let source = "the project website [https://lex.ing] is fast.\n\n";
        let link = sole_link(source);
        assert_eq!(link.target, "https://lex.ing");
        assert_eq!(&source[link.range.span.clone()], "website");
    }
}
