//! Resolve label input forms to their canonical `lex.*` spelling and
//! tag each label site with the form the user wrote.
//!
//! This stage implements the namespace-policy rules normatively
//! defined in `comms/specs/general.lex` §4 — see [PR 1 of
//! #584](https://github.com/lex-fmt/lex/pull/586) for the foundation
//! (`LabelForm` enum + `form` field on [`Label`]) and PR 2 (this
//! revision) for the resolution logic + rejection.
//!
//! # Resolution order
//!
//! For each label-bearing AST node, the parser-time pipeline runs the
//! resolution order from §4.2:
//!
//! 1. **Shortcut.** If the input is a key in [`SHORTCUT_TABLE`],
//!    resolve to its canonical and tag [`LabelForm::Shortcut`].
//! 2. **Input as-is.** Otherwise, if the input names a registered
//!    [`builtins::CANONICAL_LABELS`] entry verbatim, keep it and tag
//!    [`LabelForm::Canonical`]; if the input has the community shape
//!    (one or more dots, not in a reserved prefix) and no `lex.<input>`
//!    canonical exists, tag [`LabelForm::Community`] — registry
//!    validation is deferred to the analysis stage.
//! 3. **Prefix strip.** Otherwise, if `lex.<input>` names a registered
//!    canonical, resolve to that canonical and tag [`LabelForm::Stripped`].
//! 4. **Reject.** Otherwise, the stage in [`Strict`] mode returns a
//!    [`TransformError`] with the offending input; in [`Permissive`]
//!    mode the label is left unchanged (used by the migration tool,
//!    which needs to walk legacy source bytes).
//!
//! Step 2's community branch deliberately preempts step 3's
//! prefix-strip when the input has registered community semantics;
//! the parser cannot know which dotted inputs have community handlers
//! today, so it tags Community whenever no `lex.<input>` canonical
//! exists. Core promises not to ship `lex.<owner>.<repo>`-shaped
//! canonicals whose stripped form would shadow a third-party label;
//! see the §4.2 closing paragraph.
//!
//! # Strict vs Permissive
//!
//! The standard parse pipeline ([`STRING_TO_AST`](crate::lex::transforms::standard::STRING_TO_AST))
//! uses strict mode: `doc.*` (reserved-forbidden) and unrecognized
//! bare labels surface as `TransformError`s, which propagate out as
//! parse errors. The migration tool ([`crate::lex::migrate`]) uses
//! permissive mode so it can parse legacy `doc.*` source and rewrite
//! it before the source reaches a strict-mode parse.

use crate::lex::ast::elements::annotation::Annotation;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::label::{Label, LabelForm};
use crate::lex::ast::elements::verbatim::Verbatim;
use crate::lex::ast::Document;
use crate::lex::builtins;
use crate::lex::transforms::{Runnable, TransformError};

/// Curated one-segment shortcuts for high-traffic `lex.*` canonicals.
///
/// Normative; the same table appears verbatim in
/// `comms/specs/general.lex` §4.2. Adding a new entry is a minor
/// version bump; removing one is breaking and should not happen.
pub const SHORTCUT_TABLE: &[(&str, &str)] = &[
    ("table", "lex.tabular.table"),
    ("image", "lex.media.image"),
    ("video", "lex.media.video"),
    ("audio", "lex.media.audio"),
    ("author", "lex.metadata.author"),
    ("title", "lex.metadata.title"),
    ("tags", "lex.metadata.tags"),
    ("date", "lex.metadata.date"),
    ("include", "lex.include"),
    ("notes", "lex.notes"),
];

/// Reserved prefix the namespace policy forbids third parties (and
/// users) from authoring. Authoring any `doc.<anything>` label is a
/// parse error under [`Mode::Strict`].
const FORBIDDEN_PREFIX: &str = "doc.";

/// Reserved prefix for the core namespace. Inputs starting with this
/// prefix follow the "canonical" branch of the resolution order: the
/// label must name a registered [`builtins::CANONICAL_LABELS`] entry.
const LEX_PREFIX: &str = "lex.";

/// The resolved spelling + form classification for a label input, or a
/// structured reason the input cannot be accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Accept; carry forward `(canonical, form)` to the AST.
    Resolved(String, LabelForm),
    /// Reject; the input cannot be authored. Strict mode propagates
    /// this as a parse error.
    Rejected(RejectReason),
}

/// Why a label input was rejected. Surfaces in the strict-mode
/// [`TransformError`] message and (in PR 4 of #584) in the analysis
/// stage's diagnostics with a quickfix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// Input started with `doc.`. The prefix is reserved-forbidden.
    Forbidden { input: String },
    /// Input starts with `lex.` but does not name a registered canonical.
    UnknownCanonical { input: String },
}

impl RejectReason {
    /// Render the reason as a user-facing message. Used by both the
    /// strict-mode `TransformError` and PR 4's analysis-time
    /// diagnostic.
    pub fn message(&self) -> String {
        match self {
            Self::Forbidden { input } => format!(
                "label `{input}` uses the reserved `doc.*` prefix \
                 (forbidden under namespace policy; see general.lex §4.1)"
            ),
            Self::UnknownCanonical { input } => {
                format!("label `{input}` is not a registered `lex.*` canonical")
            }
        }
    }
}

/// Resolution mode. See module-level docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Standard parse pipeline behavior — rejected inputs surface as
    /// `TransformError`. Used by [`STRING_TO_AST`].
    Strict,
    /// Migration-tool behavior — rejected inputs leave the label
    /// unchanged, value and form untouched, so the walker can still
    /// reach legacy spellings to rewrite them.
    Permissive,
}

/// Classify a single label string against the namespace policy.
/// Pure function over the input string; exposed for unit tests + the
/// CLI's pre-format validation (PR 5).
pub fn classify_label(input: &str) -> Resolution {
    // 1. Shortcut table.
    if let Some((_, canonical)) = SHORTCUT_TABLE.iter().find(|(s, _)| *s == input) {
        return Resolution::Resolved((*canonical).to_string(), LabelForm::Shortcut);
    }

    // doc.* is reserved-forbidden — reject before any other branch.
    if input.starts_with(FORBIDDEN_PREFIX) {
        return Resolution::Rejected(RejectReason::Forbidden {
            input: input.to_string(),
        });
    }

    // 2a. `lex.*` literal — must be a registered canonical.
    if input.starts_with(LEX_PREFIX) {
        if builtins::is_canonical_label(input) {
            return Resolution::Resolved(input.to_string(), LabelForm::Canonical);
        }
        return Resolution::Rejected(RejectReason::UnknownCanonical {
            input: input.to_string(),
        });
    }

    // 3. Prefix-strip: `lex.<input>` would be a registered canonical?
    let candidate = format!("{LEX_PREFIX}{input}");
    if builtins::is_canonical_label(&candidate) {
        return Resolution::Resolved(candidate, LabelForm::Stripped);
    }

    // 2b. Community shape — at least one dot, no registered canonical
    //     under the prefix-strip rule. Defer registry validation to
    //     the analysis stage.
    if input.contains('.') {
        return Resolution::Resolved(input.to_string(), LabelForm::Community);
    }

    // 4. Bare input with no shortcut and no `lex.<input>` canonical:
    //    accept as Community. The parser is deliberately permissive
    //    here so document-scoped reference identifiers (footnote
    //    numbers, labeled-footnote `^name` markers, citation keys)
    //    parse without each needing a dedicated carve-out. PR 4 of
    //    #584 adds an analysis-time lint that flags suspicious bare
    //    names (e.g. close matches to known shortcuts) so typo
    //    prevention moves up the stack rather than into parse-time
    //    rejection. See `comms/specs/general.lex` §4.2 step 4.
    Resolution::Resolved(input.to_string(), LabelForm::Community)
}

/// Reverse-lookup the [`SHORTCUT_TABLE`]: given a canonical `lex.*`
/// label, return its blessed shortcut, if any. Used by emitters that
/// preserve the user's source spelling on roundtrip — when a label
/// was classified as `LabelForm::Shortcut`, this is the spelling to
/// emit.
pub fn shortcut_for_canonical(canonical: &str) -> Option<&'static str> {
    SHORTCUT_TABLE
        .iter()
        .find(|(_, c)| *c == canonical)
        .map(|(shortcut, _)| *shortcut)
}

/// Return the source-form spelling for `label`. Formatters call this
/// to emit the same spelling the user wrote, honoring the
/// form-preservation contract from `comms/specs/general.lex` §4.3.
///
/// - `Canonical` / `Community` → the stored `value` verbatim.
/// - `Stripped` → `lex.` prefix stripped from the canonical.
/// - `Shortcut` → the blessed shortcut from [`SHORTCUT_TABLE`].
///
/// Falls back to `value` for any malformed combination (e.g. a
/// `Shortcut`-tagged label whose canonical isn't in the table —
/// shouldn't happen but defensively keeps emission lossless).
pub fn source_spelling(label: &Label) -> &str {
    match label.form {
        LabelForm::Canonical | LabelForm::Community => &label.value,
        LabelForm::Stripped => label.value.strip_prefix(LEX_PREFIX).unwrap_or(&label.value),
        LabelForm::Shortcut => shortcut_for_canonical(&label.value).unwrap_or(&label.value),
    }
}

/// Post-parse pass that resolves and tags label sites against the
/// namespace policy.
pub struct NormalizeLabels {
    mode: Mode,
}

impl NormalizeLabels {
    /// Strict-mode constructor used by the standard parse pipeline.
    pub fn new() -> Self {
        Self { mode: Mode::Strict }
    }

    /// Permissive-mode constructor used by the migration tool.
    pub fn permissive() -> Self {
        Self {
            mode: Mode::Permissive,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
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
            rewrite_annotation(annotation, self.mode)?;
        }
        for annotation in input.root.annotations.iter_mut() {
            rewrite_annotation(annotation, self.mode)?;
        }
        for child in input.root.children.as_mut_vec().iter_mut() {
            rewrite_in_item(child, self.mode)?;
        }
        Ok(input)
    }
}

/// Resolve `label` in place against the namespace policy. Strict mode
/// surfaces rejections as `TransformError`; permissive mode leaves
/// rejected labels untouched (used by the migration tool).
fn normalize_label(label: &mut Label, mode: Mode) -> Result<(), TransformError> {
    apply_resolution(label, classify_label(&label.value), mode)
}

// (verbatim labels share the same resolution as annotations now that
// bare unknowns tag Community; the dedicated carve-out is gone.)

fn apply_resolution(
    label: &mut Label,
    resolution: Resolution,
    mode: Mode,
) -> Result<(), TransformError> {
    match resolution {
        Resolution::Resolved(canonical, form) => {
            label.value = canonical;
            label.form = form;
            Ok(())
        }
        Resolution::Rejected(reason) => match mode {
            Mode::Strict => Err(TransformError::StageFailed {
                stage: "NormalizeLabels".to_string(),
                message: reason.message(),
            }),
            Mode::Permissive => Ok(()),
        },
    }
}

fn rewrite_in_item(item: &mut ContentItem, mode: Mode) -> Result<(), TransformError> {
    match item {
        ContentItem::Annotation(a) => normalize_label(&mut a.data.label, mode)?,
        ContentItem::VerbatimBlock(v) => rewrite_verbatim_label(v, mode)?,
        ContentItem::Table(t) => rewrite_in_table(t, mode)?,
        _ => {}
    }
    if let Some(attached) = attached_annotations_mut(item) {
        for annotation in attached.iter_mut() {
            rewrite_annotation(annotation, mode)?;
        }
    }
    if let Some(children) = item.children_mut() {
        for child in children.iter_mut() {
            rewrite_in_item(child, mode)?;
        }
    }
    Ok(())
}

fn rewrite_in_table(table: &mut crate::lex::ast::Table, mode: Mode) -> Result<(), TransformError> {
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
                rewrite_in_item(child, mode)?;
            }
        }
    }
    if let Some(footnotes) = table.footnotes.as_mut() {
        for annotation in footnotes.annotations.iter_mut() {
            rewrite_annotation(annotation, mode)?;
        }
        // List children are ListItems; their `children` slot reaches
        // through `children_mut`, but we still need to walk
        // `list.items` directly because List items use the typed
        // `items` collection rather than a plain children list.
        for item in footnotes.items.as_mut_vec().iter_mut() {
            rewrite_in_item(item, mode)?;
        }
    }
    Ok(())
}

fn rewrite_annotation(annotation: &mut Annotation, mode: Mode) -> Result<(), TransformError> {
    normalize_label(&mut annotation.data.label, mode)?;
    for child in annotation.children.as_mut_vec().iter_mut() {
        rewrite_in_item(child, mode)?;
    }
    Ok(())
}

fn rewrite_verbatim_label(verbatim: &mut Verbatim, mode: Mode) -> Result<(), TransformError> {
    normalize_label(&mut verbatim.closing_data.label, mode)
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

    // ── source_spelling round-trip tests ────────────────────────────────

    #[test]
    fn source_spelling_canonical_returns_value() {
        use crate::lex::ast::elements::label::Label;
        let label = Label::from_string("lex.metadata.author").with_form(LabelForm::Canonical);
        assert_eq!(source_spelling(&label), "lex.metadata.author");
    }

    #[test]
    fn source_spelling_stripped_drops_lex_prefix() {
        use crate::lex::ast::elements::label::Label;
        let label = Label::from_string("lex.metadata.author").with_form(LabelForm::Stripped);
        assert_eq!(source_spelling(&label), "metadata.author");
    }

    #[test]
    fn source_spelling_shortcut_reverse_looks_up_table() {
        use crate::lex::ast::elements::label::Label;
        let label = Label::from_string("lex.metadata.author").with_form(LabelForm::Shortcut);
        assert_eq!(source_spelling(&label), "author");
    }

    #[test]
    fn source_spelling_community_returns_value() {
        use crate::lex::ast::elements::label::Label;
        let label = Label::from_string("acme.task").with_form(LabelForm::Community);
        assert_eq!(source_spelling(&label), "acme.task");
    }

    #[test]
    fn source_spelling_round_trips_every_shortcut_table_entry() {
        // For each (shortcut, canonical) row, building a Shortcut-form
        // Label with the canonical value must round-trip to the
        // shortcut. Locks the SHORTCUT_TABLE forward + reverse maps in
        // lockstep.
        use crate::lex::ast::elements::label::Label;
        for (shortcut, canonical) in SHORTCUT_TABLE {
            let label = Label::from_string(canonical).with_form(LabelForm::Shortcut);
            assert_eq!(
                source_spelling(&label),
                *shortcut,
                "round-trip mismatch for canonical {canonical}"
            );
        }
    }

    #[test]
    fn shortcut_for_canonical_returns_none_for_unmapped_canonical() {
        // `lex.metadata.category` has no shortcut entry (it's
        // intentionally not in the table per §4.2 — its bare form
        // reads ambiguously). Reverse lookup returns None; emitters
        // fall back to the stripped form via the spelling helper's
        // own logic.
        assert!(shortcut_for_canonical("lex.metadata.category").is_none());
        assert!(shortcut_for_canonical("acme.task").is_none());
        assert!(shortcut_for_canonical("").is_none());
    }

    // ── classify_label pure-function tests ──────────────────────────────

    #[test]
    fn classify_shortcut_table_entries() {
        // Every entry in SHORTCUT_TABLE resolves to its canonical with
        // form=Shortcut.
        for (input, canonical) in SHORTCUT_TABLE {
            assert_eq!(
                classify_label(input),
                Resolution::Resolved((*canonical).to_string(), LabelForm::Shortcut),
                "shortcut {input} must resolve to {canonical}"
            );
        }
    }

    #[test]
    fn classify_lex_canonical_input_as_canonical() {
        // `lex.*` literal authored verbatim resolves to itself,
        // form=Canonical, when registered.
        for canonical in ["lex.include", "lex.metadata.author", "lex.tabular.table"] {
            assert_eq!(
                classify_label(canonical),
                Resolution::Resolved(canonical.to_string(), LabelForm::Canonical),
            );
        }
    }

    #[test]
    fn classify_lex_unknown_canonical_rejects() {
        // `lex.X` that isn't registered surfaces an UnknownCanonical
        // rejection — strict mode propagates it; permissive mode
        // leaves the label alone.
        assert_eq!(
            classify_label("lex.foobar"),
            Resolution::Rejected(RejectReason::UnknownCanonical {
                input: "lex.foobar".to_string()
            })
        );
    }

    #[test]
    fn classify_stripped_form_resolves_against_canonical_set() {
        // Multi-segment input that prepends to a known canonical
        // resolves Stripped.
        assert_eq!(
            classify_label("metadata.author"),
            Resolution::Resolved("lex.metadata.author".to_string(), LabelForm::Stripped),
        );
        assert_eq!(
            classify_label("tabular.table"),
            Resolution::Resolved("lex.tabular.table".to_string(), LabelForm::Stripped),
        );
        assert_eq!(
            classify_label("media.image"),
            Resolution::Resolved("lex.media.image".to_string(), LabelForm::Stripped),
        );
    }

    #[test]
    fn classify_stripped_form_works_for_non_shortcut_metadata() {
        // The four metadata labels without a shortcut entry must still
        // be reachable via prefix-strip (this is the §4.2 contract:
        // prefix-strip is universal, shortcuts are curated additions).
        for stripped in [
            "metadata.category",
            "metadata.template",
            "metadata.publishing-date",
            "metadata.front-matter",
        ] {
            let canonical = format!("lex.{stripped}");
            assert_eq!(
                classify_label(stripped),
                Resolution::Resolved(canonical.clone(), LabelForm::Stripped),
                "{stripped} must resolve to {canonical}"
            );
        }
    }

    #[test]
    fn classify_community_labels_tag_as_community() {
        // Dotted non-reserved inputs without a prefix-strip match tag
        // Community. Registry validation is deferred to analysis.
        for community in ["acme.task", "mycompany.review", "owner.repo.subtype"] {
            assert_eq!(
                classify_label(community),
                Resolution::Resolved(community.to_string(), LabelForm::Community),
                "{community} must tag as Community"
            );
        }
    }

    #[test]
    fn classify_doc_prefix_rejects() {
        // `doc.*` is reserved-forbidden under §4.1; every doc.X input
        // must reject with Forbidden, including the four legacy
        // entries (doc.table / doc.image / doc.video / doc.audio).
        for forbidden in [
            "doc.table",
            "doc.image",
            "doc.video",
            "doc.audio",
            "doc.random",
        ] {
            assert_eq!(
                classify_label(forbidden),
                Resolution::Rejected(RejectReason::Forbidden {
                    input: forbidden.to_string()
                }),
                "{forbidden} must reject as Forbidden"
            );
        }
    }

    #[test]
    fn classify_unknown_bare_tags_as_community() {
        // Bare inputs that aren't in SHORTCUT_TABLE and have no
        // matching `lex.<input>` canonical are accepted as Community
        // (per the spec's §4.2 step 4 — analysis lints typos rather
        // than parse-time rejection). This covers footnote IDs (`42`,
        // `^name`), citation keys (`spec2025`), and unrecognized but
        // user-authored marker labels.
        for community in ["42", "^name", "spec2025", "foobar"] {
            assert_eq!(
                classify_label(community),
                Resolution::Resolved(community.to_string(), LabelForm::Community),
                "{community} must tag as Community"
            );
        }
    }

    // ── End-to-end NormalizeLabels (strict mode) tests ──────────────────

    /// Parse through the standard strict-mode pipeline. `STRING_TO_AST`
    /// already invokes `NormalizeLabels::new()` (strict) as one of its
    /// stages, so the resulting document is fully classified.
    fn parse(src: &str) -> Document {
        STRING_TO_AST.run(src.to_string()).expect("parse ok")
    }

    fn parse_strict(src: &str) -> Result<Document, TransformError> {
        STRING_TO_AST.run(src.to_string())
    }

    #[test]
    fn shortcut_title_resolves_to_canonical_with_shortcut_form() {
        let doc = parse(":: title :: My Document\n\nBody.\n");
        let ann = doc.annotations.first().expect("title annotation");
        assert_eq!(ann.data.label.value, "lex.metadata.title");
        assert_eq!(ann.data.label.form, LabelForm::Shortcut);
    }

    #[test]
    fn stripped_metadata_resolves_to_canonical_with_stripped_form() {
        let doc = parse(":: metadata.category :: tech\n\nBody.\n");
        let ann = doc.annotations.first().expect("category annotation");
        assert_eq!(ann.data.label.value, "lex.metadata.category");
        assert_eq!(ann.data.label.form, LabelForm::Stripped);
    }

    #[test]
    fn lex_canonical_input_keeps_value_and_canonical_form() {
        let doc = parse(":: lex.include src=other.lex ::\n\nBody.\n");
        let ann = doc.annotations.first().expect("include annotation");
        assert_eq!(ann.data.label.value, "lex.include");
        assert_eq!(ann.data.label.form, LabelForm::Canonical);
    }

    #[test]
    fn community_label_keeps_value_and_community_form() {
        let doc = parse(":: acme.custom param=value :: body\n\nBody.\n");
        let ann = doc.annotations.first().expect("acme annotation");
        assert_eq!(ann.data.label.value, "acme.custom");
        assert_eq!(ann.data.label.form, LabelForm::Community);
    }

    #[test]
    fn doc_table_verbatim_rejects_in_strict_mode() {
        // Was accepted under PR 1; strict mode now rejects per §4.1.
        let src = "Table:\n\n    | a | b |\n    |---|---|\n    | 1 | 2 |\n:: doc.table ::\n";
        let err = parse_strict(src).expect_err("doc.table must be rejected in strict mode");
        match err {
            TransformError::StageFailed { stage, message } => {
                assert_eq!(stage, "NormalizeLabels");
                assert!(
                    message.contains("doc.table") && message.contains("reserved"),
                    "message should call out the forbidden prefix; got: {message}"
                );
            }
            _ => panic!("expected StageFailed; got: {err:?}"),
        }
    }

    #[test]
    fn unknown_bare_annotation_tags_as_community_in_strict_mode() {
        // Per §4.2 step 4, bare unknowns are accepted as Community at
        // parse time; PR 4 of #584 wires up the typo-prevention lint
        // in lex-analysis. This test pins the parser-side behavior so
        // a future regression to hard-reject can't silently break
        // existing documents.
        let doc = parse(":: category :: foo\n\nBody.\n");
        let ann = doc.annotations.first().expect("category annotation");
        assert_eq!(ann.data.label.value, "category");
        assert_eq!(ann.data.label.form, LabelForm::Community);
    }

    #[test]
    fn unknown_lex_prefix_rejects_in_strict_mode() {
        // `lex.foobar` looks canonical-shaped but isn't registered.
        let err = parse_strict(":: lex.foobar ::\n\nBody.\n")
            .expect_err("unregistered lex.* canonical must reject");
        match err {
            TransformError::StageFailed { message, .. } => {
                assert!(message.contains("lex.foobar"));
            }
            _ => panic!("expected StageFailed; got: {err:?}"),
        }
    }

    #[test]
    fn permissive_mode_keeps_doc_table_unchanged() {
        // The migration tool needs to walk legacy source; permissive
        // mode classifies what it can and silently leaves the rest.
        let src = "Table:\n\n    | a | b |\n    |---|---|\n    | 1 | 2 |\n:: doc.table ::\n";
        // Bypass STRING_TO_AST (which always uses strict mode); run
        // the earlier stages explicitly and finish with permissive
        // NormalizeLabels.
        use crate::lex::assembling::stages::{ApplyTableConfig, AttachAnnotations, AttachRoot};
        use crate::lex::transforms::stages::ParseInlines;
        use crate::lex::transforms::standard::LEXING;
        let source = format!("{src}\n");
        let tokens = LEXING.run(source.clone()).expect("tokens");
        let mut output =
            crate::lex::parsing::engine::parse_from_flat_tokens(tokens, &source).expect("parse");
        output.root = ParseInlines::new().run(output.root).expect("inlines");
        let mut doc = AttachRoot::new().run(output).expect("attach root");
        doc = AttachAnnotations::new().run(doc).expect("attach anns");
        let doc = NormalizeLabels::permissive()
            .run(doc)
            .expect("permissive must not error");
        // ApplyTableConfig runs after normalize in the standard pipeline,
        // but isn't needed for this assertion.
        let _ = ApplyTableConfig::new();

        let verbatim_label = find_first_verbatim_label(&doc.root.children);
        assert_eq!(
            verbatim_label.as_deref(),
            Some("doc.table"),
            "permissive mode must leave doc.* untouched so the migration tool can rewrite it"
        );
    }

    #[test]
    fn shortcut_table_covers_normative_entries() {
        // Lock the table to exactly the 10 shortcuts §4.2 names.
        // Adding a label requires updating both this test and the
        // comms spec — the constraint is intentional.
        assert_eq!(SHORTCUT_TABLE.len(), 10);
        let names: Vec<&str> = SHORTCUT_TABLE.iter().map(|(s, _)| *s).collect();
        assert!(names.contains(&"table"));
        assert!(names.contains(&"image"));
        assert!(names.contains(&"video"));
        assert!(names.contains(&"audio"));
        assert!(names.contains(&"author"));
        assert!(names.contains(&"title"));
        assert!(names.contains(&"tags"));
        assert!(names.contains(&"date"));
        assert!(names.contains(&"include"));
        assert!(names.contains(&"notes"));
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
        // Same regression as PR 576's Copilot callout — walker must
        // reach annotations nested inside a cell's block content
        // (TableCell::with_children path) since
        // ContentItem::children_mut returns None for Tables.
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
        let nested = cell
            .children
            .iter()
            .find_map(|item| match item {
                ContentItem::Annotation(a) => Some(&a.data.label),
                _ => None,
            })
            .expect("nested annotation in cell.children");
        assert_eq!(nested.value, "lex.metadata.title");
        assert_eq!(nested.form, LabelForm::Shortcut);
    }

    #[test]
    fn rewrite_recurses_into_annotation_children() {
        // The walker reaches annotations nested inside another
        // annotation's body content. The outer label must itself be
        // an accepted form — `frontmatter` was an ad-hoc legacy name
        // and is rejected today, so use `metadata.front-matter`
        // (Stripped) as the outer to verify nested resolution.
        let src = ":: metadata.front-matter ::\n    :: author :: Alice\n";
        let doc = parse(src);
        let outer = doc.annotations.first().expect("outer annotation parsed");
        assert_eq!(outer.data.label.value, "lex.metadata.front-matter");
        assert_eq!(outer.data.label.form, LabelForm::Stripped);
        let inner = outer
            .children
            .iter()
            .find_map(|item| match item {
                ContentItem::Annotation(a) => Some(&a.data.label),
                _ => None,
            })
            .expect("nested annotation");
        assert_eq!(inner.value, "lex.metadata.author");
        assert_eq!(inner.form, LabelForm::Shortcut);
    }
}
