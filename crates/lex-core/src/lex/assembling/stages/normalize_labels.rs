//! Normalize legacy bare labels to their canonical `lex.*` form.
//!
//! Phase 2 of the label-semantics refactor tracked in
//! [#570](https://github.com/lex-fmt/lex/issues/570). Walks a parsed
//! [`Document`] and rewrites annotation labels matching the
//! pre-extension whitelist (`title`, `author`, `date`, `tags`,
//! `category`, `template`, `publishing-date`, `front-matter`) plus the
//! four verbatim labels (`doc.table`, `doc.image`, `doc.video`,
//! `doc.audio`) to their canonical `lex.metadata.*` / `lex.tabular.*` /
//! `lex.media.*` equivalents.
//!
//! # Activation lifecycle
//!
//! Phase 2 of #570 shipped this stage as a module without wiring it
//! into the default
//! [`STRING_TO_AST`](crate::lex::transforms::standard::STRING_TO_AST)
//! pipeline. **Phase 3b wired it up**: the rewrite now runs between
//! `AttachAnnotations` and `ApplyTableConfig`, so every Document
//! emitted by `STRING_TO_AST` carries canonical labels. The legacy
//! whitelists in `lex-babel` (`ir/from_lex.rs`'s frontmatter
//! promotion and `common/verbatim/VerbatimRegistry`'s handler
//! registrations) were updated in the same PR to recognize the
//! canonical names — both halves of the bare-label flip landed
//! together so no intermediate state was broken. A follow-up phase
//! retires the legacy paths altogether once render hooks (#570
//! Phase 4) are live.
//!
//! # Why warnings are silent
//!
//! The issue's Phase 2 spec calls for a "parse-time deprecation
//! warning" alongside the rewrite. Lex-core has no production
//! diagnostic-collection vehicle today
//! ([`Document::diagnostics`](crate::lex::ast::Document::diagnostics) is
//! pull-based and not wired through the LSP / CLI; the LSP uses
//! `lex-analysis::diagnostics`). Surfacing a warning therefore either
//! requires adding storage on the AST (invasive, breaks
//! `PartialEq`-based test fixtures) or a side channel that the LSP must
//! opt into. Phase 5 ships the user-facing surface naturally: a
//! `lexd migrate-labels` subcommand walks raw source, lists every legacy
//! site by line/column, and offers an in-place rewrite. The rewrite
//! itself stays silent here.

use crate::lex::ast::elements::annotation::Annotation;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::verbatim::Verbatim;
use crate::lex::ast::Document;
use crate::lex::transforms::{Runnable, TransformError};

/// Pairings of legacy bare label → canonical `lex.*` form.
///
/// Ordering mirrors the legacy whitelist in
/// `crates/lex-babel/src/ir/from_lex.rs` (metadata first), followed by
/// the four verbatim labels owned by the legacy
/// `lex-babel::common::verbatim::VerbatimRegistry`. Anything not in
/// this table is left untouched — third-party and user-defined labels
/// are out of scope.
pub const LEGACY_TO_CANONICAL: &[(&str, &str)] = &[
    ("title", "lex.metadata.title"),
    ("author", "lex.metadata.author"),
    ("date", "lex.metadata.date"),
    ("tags", "lex.metadata.tags"),
    ("category", "lex.metadata.category"),
    ("template", "lex.metadata.template"),
    ("publishing-date", "lex.metadata.publishing-date"),
    ("front-matter", "lex.metadata.front-matter"),
    ("doc.table", "lex.tabular.table"),
    ("doc.image", "lex.media.image"),
    ("doc.video", "lex.media.video"),
    ("doc.audio", "lex.media.audio"),
];

/// Lookup the canonical form for a legacy label, if any.
pub fn canonical_for(label: &str) -> Option<&'static str> {
    LEGACY_TO_CANONICAL
        .iter()
        .find(|(legacy, _)| *legacy == label)
        .map(|(_, canonical)| *canonical)
}

/// Post-parse pass that rewrites legacy labels to their canonical form.
pub struct NormalizeLabels;

impl NormalizeLabels {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NormalizeLabels {
    fn default() -> Self {
        Self::new()
    }
}

impl Runnable<Document, Document> for NormalizeLabels {
    fn run(&self, mut input: Document) -> Result<Document, TransformError> {
        for annotation in input.annotations.iter_mut() {
            rewrite_annotation(annotation);
        }
        for annotation in input.root.annotations.iter_mut() {
            rewrite_annotation(annotation);
        }
        for child in input.root.children.as_mut_vec().iter_mut() {
            rewrite_in_item(child);
        }
        Ok(input)
    }
}

fn rewrite_in_item(item: &mut ContentItem) {
    match item {
        ContentItem::Annotation(a) => {
            if let Some(canonical) = canonical_for(&a.data.label.value) {
                a.data.label.value = canonical.to_string();
            }
        }
        ContentItem::VerbatimBlock(v) => rewrite_verbatim_label(v),
        ContentItem::Table(t) => rewrite_in_table(t),
        _ => {}
    }
    if let Some(attached) = attached_annotations_mut(item) {
        for annotation in attached.iter_mut() {
            rewrite_annotation(annotation);
        }
    }
    if let Some(children) = item.children_mut() {
        for child in children.iter_mut() {
            rewrite_in_item(child);
        }
    }
}

fn rewrite_in_table(table: &mut crate::lex::ast::Table) {
    // `ContentItem::children_mut` returns `None` for tables (their
    // structure lives in rows/cells, not a flat children list), so an
    // explicit walk is needed to reach annotations nested inside cells
    // or footnotes. Without this, a legacy label inside a table cell
    // would slip through the pipeline untouched.
    for row in table
        .header_rows
        .iter_mut()
        .chain(table.body_rows.iter_mut())
    {
        for cell in row.cells.iter_mut() {
            for child in cell.children.as_mut_vec().iter_mut() {
                rewrite_in_item(child);
            }
        }
    }
    if let Some(footnotes) = table.footnotes.as_mut() {
        for annotation in footnotes.annotations.iter_mut() {
            rewrite_annotation(annotation);
        }
        // List children are ListItems; their `children` slot reaches
        // through `children_mut`, but we still need to walk
        // `list.items` directly because List items use the typed
        // `items` collection rather than a plain children list.
        for item in footnotes.items.as_mut_vec().iter_mut() {
            rewrite_in_item(item);
        }
    }
}

fn rewrite_annotation(annotation: &mut Annotation) {
    if let Some(canonical) = canonical_for(&annotation.data.label.value) {
        annotation.data.label.value = canonical.to_string();
    }
    for child in annotation.children.as_mut_vec().iter_mut() {
        rewrite_in_item(child);
    }
}

fn rewrite_verbatim_label(verbatim: &mut Verbatim) {
    if let Some(canonical) = canonical_for(&verbatim.closing_data.label.value) {
        verbatim.closing_data.label.value = canonical.to_string();
    }
}

fn attached_annotations_mut(item: &mut ContentItem) -> Option<&mut Vec<Annotation>> {
    match item {
        ContentItem::Session(s) => Some(&mut s.annotations),
        ContentItem::Paragraph(p) => Some(&mut p.annotations),
        ContentItem::Definition(d) => Some(&mut d.annotations),
        ContentItem::List(l) => Some(&mut l.annotations),
        ContentItem::ListItem(li) => Some(&mut li.annotations),
        ContentItem::VerbatimBlock(v) => Some(&mut v.annotations),
        ContentItem::Table(t) => Some(&mut t.annotations),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::transforms::standard::STRING_TO_AST;

    /// Parse + invoke the normalize stage explicitly. `STRING_TO_AST`
    /// already runs `NormalizeLabels` after Phase 3b's wire-up, so
    /// the second call here is idempotent — kept so the tests stay
    /// readable as direct exercises of the stage's behaviour rather
    /// than depending on pipeline order.
    fn parse(src: &str) -> Document {
        let doc = STRING_TO_AST.run(src.to_string()).expect("parse ok");
        NormalizeLabels::new().run(doc).expect("normalize ok")
    }

    fn find_first_annotation_label(doc: &Document) -> Option<String> {
        doc.annotations
            .first()
            .map(|a| a.data.label.value.clone())
            .or_else(|| find_inline_annotation_label(&doc.root.children))
    }

    fn find_inline_annotation_label(items: &[ContentItem]) -> Option<String> {
        for item in items {
            if let ContentItem::Annotation(a) = item {
                return Some(a.data.label.value.clone());
            }
            if let Some(children) = item.children() {
                if let Some(label) = find_inline_annotation_label(children) {
                    return Some(label);
                }
            }
        }
        None
    }

    fn find_first_verbatim_label(items: &[ContentItem]) -> Option<String> {
        for item in items {
            if let ContentItem::VerbatimBlock(v) = item {
                return Some(v.closing_data.label.value.clone());
            }
            if let Some(children) = item.children() {
                if let Some(label) = find_first_verbatim_label(children) {
                    return Some(label);
                }
            }
        }
        None
    }

    #[test]
    fn canonical_for_recognizes_every_legacy_label() {
        for (legacy, canonical) in LEGACY_TO_CANONICAL {
            assert_eq!(
                canonical_for(legacy),
                Some(*canonical),
                "lookup must round-trip for {legacy}"
            );
        }
    }

    #[test]
    fn canonical_for_returns_none_for_unknown_labels() {
        assert!(canonical_for("acme.custom").is_none());
        assert!(canonical_for("lex.include").is_none());
        assert!(canonical_for("").is_none());
    }

    #[test]
    fn legacy_to_canonical_table_covers_phase_1_targets() {
        // Locks the rewrite set to the 8 metadata + 1 tabular + 3 media
        // schemas that Phase 1 (#575) registered. If those families grow,
        // this list grows with them.
        assert_eq!(LEGACY_TO_CANONICAL.len(), 12);
        let metadata_count = LEGACY_TO_CANONICAL
            .iter()
            .filter(|(_, c)| c.starts_with("lex.metadata."))
            .count();
        let tabular_count = LEGACY_TO_CANONICAL
            .iter()
            .filter(|(_, c)| c.starts_with("lex.tabular."))
            .count();
        let media_count = LEGACY_TO_CANONICAL
            .iter()
            .filter(|(_, c)| c.starts_with("lex.media."))
            .count();
        assert_eq!(metadata_count, 8);
        assert_eq!(tabular_count, 1);
        assert_eq!(media_count, 3);
    }

    #[test]
    fn document_level_title_annotation_is_rewritten() {
        // Annotation single-line form: `:: label :: inline content`.
        let doc = parse(":: title :: My Document\n\nBody.\n");
        assert_eq!(
            find_first_annotation_label(&doc).as_deref(),
            Some("lex.metadata.title"),
            "document-level :: title :: must rewrite to lex.metadata.title"
        );
    }

    #[test]
    fn every_metadata_label_rewrites() {
        for (legacy, canonical) in LEGACY_TO_CANONICAL
            .iter()
            .filter(|(_, c)| c.starts_with("lex.metadata."))
        {
            let src = format!(":: {legacy} :: value\n\nBody.\n");
            let doc = parse(&src);
            assert_eq!(
                find_first_annotation_label(&doc).as_deref(),
                Some(*canonical),
                ":: {legacy} :: must rewrite to {canonical}"
            );
        }
    }

    #[test]
    fn doc_table_verbatim_rewrites_to_lex_tabular_table() {
        // Lex verbatim syntax: subject line ending in `:`, indented body,
        // then `:: label ::` as the closer.
        let src = "Table:\n\n    | a | b |\n    |---|---|\n    | 1 | 2 |\n:: doc.table ::\n";
        let doc = parse(src);
        assert_eq!(
            find_first_verbatim_label(&doc.root.children).as_deref(),
            Some("lex.tabular.table"),
            ":: doc.table :: verbatim must rewrite to lex.tabular.table"
        );
    }

    #[test]
    fn doc_image_verbatim_rewrites_to_lex_media_image() {
        let src = "Image:\n    alt text\n:: doc.image src=x.png ::\n";
        let doc = parse(src);
        assert_eq!(
            find_first_verbatim_label(&doc.root.children).as_deref(),
            Some("lex.media.image"),
        );
    }

    #[test]
    fn doc_video_and_audio_verbatims_rewrite() {
        for (legacy, canonical) in [
            ("doc.video", "lex.media.video"),
            ("doc.audio", "lex.media.audio"),
        ] {
            let src = format!("Media:\n    caption\n:: {legacy} src=file ::\n");
            let doc = parse(&src);
            assert_eq!(
                find_first_verbatim_label(&doc.root.children).as_deref(),
                Some(canonical),
                ":: {legacy} :: must rewrite to {canonical}"
            );
        }
    }

    #[test]
    fn non_legacy_labels_are_left_alone() {
        let src = ":: acme.custom param=value :: body\n\nBody.\n";
        let doc = parse(src);
        let label = find_first_annotation_label(&doc);
        assert_eq!(
            label.as_deref(),
            Some("acme.custom"),
            "third-party labels must be preserved verbatim"
        );
    }

    #[test]
    fn lex_include_label_is_left_alone() {
        // Already-canonical lex.* labels must not be rewritten.
        let src = ":: lex.include src=other.lex ::\n\nBody.\n";
        let doc = parse(src);
        let label = find_first_annotation_label(&doc);
        assert_eq!(label.as_deref(), Some("lex.include"));
    }

    #[test]
    fn rewrite_preserves_annotation_parameters() {
        // The rewrite touches the label only — params and body
        // content must survive unchanged.
        let src = ":: author email=alice@example.com :: Alice\n\nBody.\n";
        let doc = parse(src);
        let first = doc
            .annotations
            .first()
            .or_else(|| {
                doc.root.children.iter().find_map(|item| match item {
                    ContentItem::Annotation(a) => Some(a),
                    _ => None,
                })
            })
            .expect("annotation parsed");
        assert_eq!(first.data.label.value, "lex.metadata.author");
        let email_param = first
            .data
            .parameters
            .iter()
            .find(|p| p.key == "email")
            .expect("email param preserved");
        assert_eq!(email_param.value, "alice@example.com");
    }

    #[test]
    fn rewrite_preserves_label_location() {
        // Range info must round-trip — the LSP relies on label location
        // for goto-definition and similar features. Rewrite mutates
        // `label.value` in place without touching `label.location`.
        let src = ":: title :: T\n\nBody.\n";
        let doc = parse(src);
        let first = doc
            .annotations
            .first()
            .or_else(|| {
                doc.root.children.iter().find_map(|item| match item {
                    ContentItem::Annotation(a) => Some(a),
                    _ => None,
                })
            })
            .expect("annotation parsed");
        let loc = &first.data.label.location;
        assert_ne!(
            loc.start, loc.end,
            "rewrite must preserve the label's source location, not zero it"
        );
    }

    #[test]
    fn rewrite_recurses_into_table_cell_block_children() {
        // Regression for Copilot's PR 576 callout: `ContentItem::Table`
        // returns `None` from `children_mut()` (its structure lives in
        // rows/cells), so the generic walker won't reach legacy labels
        // nested inside a cell's block content. Today's parser does
        // not emit block children in cells — cell annotations land in
        // the inline `content: TextContent` — but the AST surface
        // allows it via `TableCell::with_children`, and Phase 3's
        // wire-up may begin using that slot. Test the contract
        // directly by building the AST programmatically.
        use crate::lex::ast::elements::annotation::Annotation;
        use crate::lex::ast::elements::label::Label;
        use crate::lex::ast::elements::table::{Table, TableCell, TableRow};
        use crate::lex::ast::elements::typed_content::ContentElement;
        use crate::lex::ast::elements::verbatim::VerbatimBlockMode;
        use crate::lex::ast::text_content::TextContent;

        let inner_annotation = Annotation::marker(Label::from_string("title"));
        let cell = TableCell::new(TextContent::from_string("inline".into(), None))
            .with_children(vec![ContentElement::Annotation(inner_annotation)]);
        let row = TableRow::new(vec![cell]);
        let table = Table::new(
            TextContent::from_string("Data".into(), None),
            Vec::new(),
            vec![row],
            VerbatimBlockMode::Inflow,
        );

        let mut doc = Document::new();
        doc.root
            .children
            .as_mut_vec()
            .push(ContentItem::Table(Box::new(table)));

        let doc = NormalizeLabels::new().run(doc).expect("normalize ok");

        // Reach into the table and confirm the nested annotation got
        // rewritten.
        let table = doc
            .root
            .children
            .iter()
            .find_map(|item| match item {
                ContentItem::Table(t) => Some(t),
                _ => None,
            })
            .expect("table present");
        let cell = &table.body_rows[0].cells[0];
        let nested_label = cell
            .children
            .iter()
            .find_map(|item| match item {
                ContentItem::Annotation(a) => Some(a.data.label.value.as_str()),
                _ => None,
            })
            .expect("nested annotation in cell.children");
        assert_eq!(
            nested_label, "lex.metadata.title",
            "legacy label inside a table cell's block children must be rewritten"
        );
    }

    #[test]
    fn rewrite_recurses_into_annotation_children() {
        // The walker must reach annotations nested inside another
        // annotation's content body — `rewrite_annotation` calls
        // `rewrite_in_item` on each child, which in turn handles inline
        // `ContentItem::Annotation` labels.
        let src = ":: outer ::\n    :: author :: Alice\n";
        let doc = parse(src);
        let outer = doc.annotations.first().expect("outer annotation parsed");
        // Find the inner annotation in outer's children.
        let inner_label = outer.children.iter().find_map(|item| match item {
            ContentItem::Annotation(a) => Some(a.data.label.value.clone()),
            _ => None,
        });
        assert_eq!(
            inner_label.as_deref(),
            Some("lex.metadata.author"),
            "nested annotation inside outer must be rewritten"
        );
    }
}
