//! Source-level migration for legacy bare labels.
//!
//! Phase 5 of the label-semantics refactor tracked in
//! [#570](https://github.com/lex-fmt/lex/issues/570). Parses a `.lex`
//! source string, identifies every legacy bare label that
//! [`NormalizeLabels`](crate::lex::assembling::stages::NormalizeLabels)
//! would rewrite at parse time, and produces a rewritten source string
//! with the labels migrated to their canonical `lex.*` form.
//!
//! This is what powers `lexd migrate-labels`: an explicit, source-level
//! pass users can run to migrate their `.lex` files once and stop
//! relying on the silent parse-time rewrite.
//!
//! # Why source-level, not AST-level
//!
//! [`NormalizeLabels`] already migrates the AST in memory — it's
//! invoked unconditionally by `STRING_TO_AST` since #570 Phase 3b.
//! Source-level migration is different: it produces a rewritten `.lex`
//! file that no longer carries the legacy form, so future parses don't
//! need the in-flight rewrite at all. This is the user-facing
//! deliverable for the "two minor versions to migrate" deprecation
//! window the issue called out.
//!
//! The key trick: after `STRING_TO_AST` runs, every `Label.value` in
//! the AST is *canonical*, but `Label.location.span` still points at
//! the **original** source bytes — which still carry the legacy form.
//! So we walk the parsed AST collecting `(span, legacy_text)` pairs
//! and rewrite the source in reverse byte order. No re-parsing, no
//! regex heuristics, no ambiguity.

use crate::lex::assembling::stages::normalize_labels::{canonical_for, LEGACY_TO_CANONICAL};
use crate::lex::ast::elements::annotation::Annotation;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::label::Label;
use crate::lex::ast::elements::verbatim::Verbatim;
use crate::lex::ast::Document;
use crate::lex::transforms::standard::STRING_TO_AST;

/// One legacy-label rewrite site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelMigration {
    /// Byte range in the original source that holds the legacy label.
    pub byte_range: std::ops::Range<usize>,
    /// Legacy label as it appears in the source.
    pub from: &'static str,
    /// Canonical replacement.
    pub to: &'static str,
}

/// The result of a migration pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    /// The rewritten source, with every legacy label replaced by its
    /// canonical form. Equals the input verbatim when
    /// [`migrations`](Self::migrations) is empty.
    pub rewritten: String,
    /// One entry per legacy label site found in the input. Empty when
    /// the source has no legacy labels.
    pub migrations: Vec<LabelMigration>,
}

impl MigrationOutcome {
    /// True when the migration pass found any legacy labels to
    /// rewrite. `lexd migrate-labels --check` exits non-zero when this
    /// is true.
    pub fn is_modified(&self) -> bool {
        !self.migrations.is_empty()
    }
}

/// Walk `src`'s parsed AST and migrate every legacy bare label found
/// to its canonical `lex.*` form. Returns the rewritten source plus a
/// per-site list of what changed.
///
/// Returns `Err` only when the source fails to parse — the migration
/// pass needs a clean parse to locate label spans. Soft diagnostics
/// from the parser are ignored; only hard parse errors abort.
pub fn migrate_labels_in_source(src: &str) -> Result<MigrationOutcome, MigrationError> {
    let doc = STRING_TO_AST
        .run(src.to_string())
        .map_err(|e| MigrationError::ParseFailed {
            message: e.to_string(),
        })?;

    let mut sites = Vec::new();
    collect_sites(&doc, src, &mut sites);

    let rewritten = apply_migrations(src, &sites);
    Ok(MigrationOutcome {
        rewritten,
        migrations: sites,
    })
}

/// Errors surfaced by [`migrate_labels_in_source`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    /// The parser rejected the source. The migration pass needs a
    /// clean parse to locate label spans, so it cannot proceed.
    ParseFailed { message: String },
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseFailed { message } => write!(f, "parse failed: {message}"),
        }
    }
}

impl std::error::Error for MigrationError {}

fn collect_sites(doc: &Document, src: &str, sites: &mut Vec<LabelMigration>) {
    for ann in &doc.annotations {
        check_label(&ann.data.label, src, sites);
        for child in ann.children.iter() {
            collect_in_item(child, src, sites);
        }
    }
    for ann in &doc.root.annotations {
        check_label(&ann.data.label, src, sites);
        for child in ann.children.iter() {
            collect_in_item(child, src, sites);
        }
    }
    for item in doc.root.children.iter() {
        collect_in_item(item, src, sites);
    }
}

fn collect_in_item(item: &ContentItem, src: &str, sites: &mut Vec<LabelMigration>) {
    match item {
        ContentItem::Annotation(a) => check_annotation(a, src, sites),
        ContentItem::VerbatimBlock(v) => check_verbatim(v, src, sites),
        _ => {}
    }
    if let Some(attached) = attached_annotations(item) {
        for ann in attached.iter() {
            check_annotation(ann, src, sites);
        }
    }
    if let Some(children) = item.children() {
        for child in children.iter() {
            collect_in_item(child, src, sites);
        }
    }
}

fn check_annotation(annotation: &Annotation, src: &str, sites: &mut Vec<LabelMigration>) {
    check_label(&annotation.data.label, src, sites);
    for child in annotation.children.iter() {
        collect_in_item(child, src, sites);
    }
}

fn check_verbatim(verbatim: &Verbatim, src: &str, sites: &mut Vec<LabelMigration>) {
    check_label(&verbatim.closing_data.label, src, sites);
}

fn attached_annotations(item: &ContentItem) -> Option<&Vec<Annotation>> {
    match item {
        ContentItem::Session(s) => Some(&s.annotations),
        ContentItem::Paragraph(p) => Some(&p.annotations),
        ContentItem::Definition(d) => Some(&d.annotations),
        ContentItem::List(l) => Some(&l.annotations),
        ContentItem::ListItem(li) => Some(&li.annotations),
        ContentItem::VerbatimBlock(v) => Some(&v.annotations),
        ContentItem::Table(t) => Some(&t.annotations),
        _ => None,
    }
}

fn check_label(label: &Label, src: &str, sites: &mut Vec<LabelMigration>) {
    // After NormalizeLabels runs (which happens in STRING_TO_AST since
    // Phase 3b), `label.value` is canonical. But the label's span
    // still points at the original source bytes — so the source slice
    // is the *legacy* form when one was used.
    //
    // The parser's label span typically captures one trailing
    // whitespace byte (separator between the label and either the
    // next param or the closing `::`). Trim the slice to the
    // actual label characters and adjust the byte range we report so
    // the rewrite drops in cleanly without disturbing the surrounding
    // whitespace.
    let span = &label.location.span;
    let start = span.start;
    let end = span.end;
    if start > end || end > src.len() {
        // Defensive: parser should always emit valid spans, but if a
        // synthetic label slipped through we don't want to panic.
        return;
    }
    let raw = &src[start..end];
    let leading_ws = raw.bytes().take_while(|b| b.is_ascii_whitespace()).count();
    let trailing_ws = raw
        .bytes()
        .rev()
        .take_while(|b| b.is_ascii_whitespace())
        .count();
    let trim_start = start + leading_ws;
    let trim_end = end.saturating_sub(trailing_ws);
    if trim_start >= trim_end {
        return;
    }
    let slice = &src[trim_start..trim_end];
    if let Some(canonical) = canonical_for(slice) {
        // Sanity check: the AST value should be the canonical form
        // NormalizeLabels rewrote it to.
        debug_assert_eq!(
            label.value, canonical,
            "NormalizeLabels should have rewritten {slice} to {canonical}, got {} in AST",
            label.value
        );
        let from = LEGACY_TO_CANONICAL
            .iter()
            .find(|(l, _)| *l == slice)
            .map(|(l, _)| *l)
            .expect("canonical_for matched but legacy table lookup didn't");
        sites.push(LabelMigration {
            byte_range: trim_start..trim_end,
            from,
            to: canonical,
        });
    }
}

fn apply_migrations(src: &str, sites: &[LabelMigration]) -> String {
    if sites.is_empty() {
        return src.to_string();
    }
    // Apply in reverse byte order so earlier replacements don't shift
    // later offsets. The walker visits in document order; reverse the
    // collected list to apply from end to start.
    let mut result = src.to_string();
    let mut sorted: Vec<&LabelMigration> = sites.iter().collect();
    sorted.sort_by(|a, b| b.byte_range.start.cmp(&a.byte_range.start));
    for site in sorted {
        result.replace_range(site.byte_range.clone(), site.to);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_legacy_labels_returns_input_unchanged() {
        let src = "Hello world.\n\n:: lex.metadata.title :: My Doc\n";
        let out = migrate_labels_in_source(src).expect("migrate ok");
        assert_eq!(out.rewritten, src);
        assert!(out.migrations.is_empty());
        assert!(!out.is_modified());
    }

    #[test]
    fn single_legacy_metadata_label_is_rewritten() {
        let src = ":: title :: My Document\n\nBody.\n";
        let out = migrate_labels_in_source(src).expect("migrate ok");
        assert!(out.is_modified());
        assert_eq!(out.migrations.len(), 1);
        assert_eq!(out.migrations[0].from, "title");
        assert_eq!(out.migrations[0].to, "lex.metadata.title");
        assert!(
            out.rewritten.contains(":: lex.metadata.title ::"),
            "rewritten source must contain the canonical label: {}",
            out.rewritten
        );
        assert!(
            !out.rewritten.contains(":: title ::"),
            "legacy label must be replaced: {}",
            out.rewritten
        );
    }

    #[test]
    fn every_legacy_label_round_trips_through_migration() {
        // Each legacy label, when used in source, produces exactly one
        // migration entry with the right canonical replacement.
        for (legacy, canonical) in LEGACY_TO_CANONICAL
            .iter()
            .filter(|(_, c)| c.starts_with("lex.metadata."))
        {
            let src = format!(":: {legacy} :: value\n\nBody.\n");
            let out = migrate_labels_in_source(&src).unwrap_or_else(|e| {
                panic!("migrate failed for {legacy}: {e}");
            });
            assert!(
                out.is_modified(),
                "migration must rewrite :: {legacy} :: ..."
            );
            assert_eq!(out.migrations[0].from, *legacy);
            assert_eq!(out.migrations[0].to, *canonical);
            assert!(
                out.rewritten.contains(&format!(":: {canonical} ::")),
                "rewritten source must contain :: {canonical} ::, got: {}",
                out.rewritten
            );
        }
    }

    #[test]
    fn verbatim_doc_table_label_is_rewritten() {
        let src = "Table:\n\n    | a | b |\n    |---|---|\n    | 1 | 2 |\n:: doc.table ::\n";
        let out = migrate_labels_in_source(src).expect("migrate ok");
        assert!(out.is_modified());
        assert_eq!(out.migrations.len(), 1);
        assert_eq!(out.migrations[0].from, "doc.table");
        assert_eq!(out.migrations[0].to, "lex.tabular.table");
        assert!(out.rewritten.contains(":: lex.tabular.table ::"));
        assert!(!out.rewritten.contains(":: doc.table ::"));
    }

    #[test]
    fn verbatim_doc_image_video_audio_labels_rewrite() {
        for (legacy, canonical) in [
            ("doc.image", "lex.media.image"),
            ("doc.video", "lex.media.video"),
            ("doc.audio", "lex.media.audio"),
        ] {
            let src = format!("Media:\n    caption\n:: {legacy} src=file ::\n");
            let out = migrate_labels_in_source(&src).expect("migrate ok");
            assert!(out.is_modified(), ":: {legacy} :: must trigger migration");
            assert_eq!(out.migrations[0].from, legacy);
            assert_eq!(out.migrations[0].to, canonical);
            assert!(
                out.rewritten.contains(&format!(":: {canonical} ")),
                "expected canonical :: {canonical} :: in {}",
                out.rewritten
            );
        }
    }

    #[test]
    fn multiple_legacy_labels_all_rewrite_with_correct_offsets() {
        let src = ":: title :: My Doc\n:: author :: Alice\n\nBody.\n";
        let out = migrate_labels_in_source(src).expect("migrate ok");
        assert_eq!(
            out.migrations.len(),
            2,
            "two legacy labels must produce two migrations: {:?}",
            out.migrations
        );
        assert!(out.rewritten.contains(":: lex.metadata.title ::"));
        assert!(out.rewritten.contains(":: lex.metadata.author ::"));
        assert!(!out.rewritten.contains(":: title ::"));
        assert!(!out.rewritten.contains(":: author ::"));
    }

    #[test]
    fn non_legacy_labels_are_left_alone() {
        let src = ":: acme.custom param=value :: body\n\nBody.\n";
        let out = migrate_labels_in_source(src).expect("migrate ok");
        assert!(!out.is_modified());
        assert_eq!(out.rewritten, src);
    }

    #[test]
    fn already_canonical_labels_are_left_alone() {
        let src = ":: lex.metadata.title :: My Doc\n:: lex.media.image src=x ::\n";
        let out = migrate_labels_in_source(src).expect("migrate ok");
        assert!(!out.is_modified(), "canonical labels must not be migrated");
        assert_eq!(out.rewritten, src);
    }

    #[test]
    fn body_text_containing_legacy_words_is_not_rewritten() {
        // Important: "title" inside paragraph body text isn't a label
        // and must not be touched.
        let src = "This paragraph mentions the title and author words.\n";
        let out = migrate_labels_in_source(src).expect("migrate ok");
        assert!(!out.is_modified(), "body words must not be rewritten");
        assert_eq!(out.rewritten, src);
    }

    #[test]
    fn migrations_have_correct_byte_ranges() {
        // Span sanity: `from` slice from the input at the migration's
        // byte range must equal the legacy label string.
        let src = ":: title :: My Doc\n\nBody.\n";
        let out = migrate_labels_in_source(src).expect("migrate ok");
        let m = &out.migrations[0];
        let slice = &src[m.byte_range.clone()];
        assert_eq!(slice, m.from, "byte range must point at the legacy text");
    }
}
