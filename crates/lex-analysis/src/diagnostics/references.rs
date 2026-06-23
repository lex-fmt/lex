//! Opt-in reference diagnostics (`check --references`).
//!
//! Deliberately separate from the always-on analyser: validates internal
//! cross-references over the merged document and emits a `missing-*-target`
//! diagnostic for each dangling reference, validates URL well-formedness
//! (pure parse, no network), and collects non-include file-path references
//! for the CLI's IO-bearing existence check.
//!
//! The public items here ([`analyze_references`], [`collect_file_references`],
//! [`FileReference`]) are re-exported from the `diagnostics` entry module so
//! their `lex_analysis::diagnostics::` paths are unchanged.

use super::{AnalysisDiagnostic, DiagnosticKind, DiagnosticSeverity};
use crate::inline::extract_references;
use lex_core::lex::ast::{ContentItem, Range};
use lex_core::lex::inlines::ReferenceType;

/// Opt-in pass: validate internal cross-references over the (merged)
/// document and emit a `missing-*-target` diagnostic for each dangling
/// in-document reference.
///
/// **Deliberately separate from [`analyze_with_registry`](super::analyze_with_registry)**
/// so the always-on analyser (and thus the LSP, which calls
/// [`analyze_with_rules`](super::analyze_with_rules) on every keystroke) does
/// *not* emit these. `check --references` calls this explicitly; the LSP can
/// opt in later.
///
/// Resolution runs over the single merged tree, so it is bidirectional:
/// a reference resolves against targets defined anywhere in the document
/// — any included fragment or the master — and a `missing-*` fires only
/// when the target is absent from the *whole* tree. Each finding's range
/// carries the reference's origin (via [`extract_references`]), so the
/// caller blames it on the file the reference was authored in.
///
/// Checked kinds and their codes:
///
/// - [`ReferenceType::Session`] → `missing-session-target`
/// - [`ReferenceType::General`] → `missing-definition-target`
/// - [`ReferenceType::AnnotationReference`] → `missing-annotation-target`
/// - [`ReferenceType::Citation`] → `missing-citation-target`
/// - [`ReferenceType::Url`] → `malformed-url` (well-formedness only)
///
/// The `Url` arm is *not* a cross-reference check: it validates the URL
/// is well-formed (a pure, IO-free parse — **no network**, by design;
/// reachability is out of scope, issue #762). It runs in this pass
/// because well-formedness is pure and `--references` already gates it.
///
/// `ToCome` / `NotSure` are intentional placeholders and never flagged;
/// `FootnoteNumber` is validated by the always-on analyser
/// (`footnotes::check_footnotes`); `File` is out of
/// scope here (issue #761). All emitted diagnostics default to
/// [`DiagnosticSeverity::Warning`] — callers apply `[diagnostics.rules]` via
/// [`apply_rules`](super::apply_rules) for per-kind overrides.
pub fn analyze_references(document: &lex_core::lex::ast::Document) -> Vec<AnalysisDiagnostic> {
    use crate::reference_targets::{targets_from_reference_type, ReferenceTarget};
    use crate::references::target_resolves;

    let mut diagnostics = Vec::new();
    crate::utils::for_each_text_content(document, &mut |text| {
        for reference in extract_references(text) {
            let (kind, render): (DiagnosticKind, String) = match &reference.reference_type {
                ReferenceType::Session { target } if !target.trim().is_empty() => (
                    DiagnosticKind::MissingSessionTarget,
                    format!(
                        "Session reference [#{}] has no matching session",
                        target.trim()
                    ),
                ),
                ReferenceType::General { target } if !target.trim().is_empty() => (
                    DiagnosticKind::MissingDefinitionTarget,
                    format!("Reference [{}] has no matching definition", target.trim()),
                ),
                ReferenceType::AnnotationReference { label } if !label.trim().is_empty() => (
                    DiagnosticKind::MissingAnnotationTarget,
                    format!(
                        "Annotation reference [::{}] has no matching annotation",
                        label.trim()
                    ),
                ),
                ReferenceType::Url { target } if !target.trim().is_empty() => {
                    // URL references are validated for well-formedness
                    // only — a pure, IO-free parse check (no network: see
                    // [`url_is_malformed`]). This is self-contained (no
                    // document resolution), so it emits inline and
                    // `continue`s like the citation arm rather than
                    // falling through to the target-resolution tail.
                    let target = target.trim();
                    if url_is_malformed(target) {
                        diagnostics.push(AnalysisDiagnostic {
                            range: reference.range.clone(),
                            severity: DiagnosticSeverity::Warning,
                            kind: DiagnosticKind::MalformedUrl,
                            message: format!("URL [{target}] is malformed"),
                        });
                    }
                    continue;
                }
                ReferenceType::Citation(data) => {
                    // A citation may carry multiple keys; each is its own
                    // potential dangling target. Emit per unresolved key.
                    for key in &data.keys {
                        if key.trim().is_empty() {
                            continue;
                        }
                        let target = ReferenceTarget::CitationKey(key.trim().to_string());
                        if !target_resolves(document, &target) {
                            diagnostics.push(AnalysisDiagnostic {
                                range: reference.range.clone(),
                                severity: DiagnosticSeverity::Warning,
                                kind: DiagnosticKind::MissingCitationTarget,
                                message: format!(
                                    "Citation [@{}] has no matching annotation or definition",
                                    key.trim()
                                ),
                            });
                        }
                    }
                    continue;
                }
                // Placeholders, footnotes (always-on), URL/File (out of
                // scope), and empty-target references: skip.
                _ => continue,
            };

            // Non-citation kinds: resolve via the reference's targets and
            // emit when none match anywhere in the merged tree.
            let resolves = targets_from_reference_type(&reference.reference_type)
                .iter()
                .any(|t| target_resolves(document, t));
            if !resolves {
                diagnostics.push(AnalysisDiagnostic {
                    range: reference.range.clone(),
                    severity: DiagnosticSeverity::Warning,
                    kind,
                    message: render,
                });
            }
        }
    });
    diagnostics
}

/// Is `target` a malformed URL? Pure, IO-free well-formedness check —
/// **never opens a connection**. Classification (`ReferenceType::Url`)
/// already guarantees one of the `http://` / `https://` / `mailto:`
/// scheme prefixes, so this catches what classification can't: embedded
/// spaces, an empty host, and otherwise-unparseable targets.
///
/// A bare `url::Url::parse(...).is_err()` is sufficient: under the WHATWG
/// URL standard the `url` crate implements, the special schemes we
/// validate (`http`/`https`) require a non-empty host, so a missing host
/// (`https://`) already parse-fails with `EmptyHost`; `mailto:` is
/// host-less and parses fine — exactly the behavior we want, with no
/// scheme-specific host check needed.
///
/// A future opt-in `--check-urls-online` would layer network
/// reachability *on top* of this — deliberately unimplemented here
/// (issue #762: reachability out of scope).
pub(super) fn url_is_malformed(target: &str) -> bool {
    url::Url::parse(target).is_err()
}

/// A non-include file-path reference and the range to blame it on.
///
/// Produced by [`collect_file_references`] for the opt-in
/// `check --references` *file-path* pass. The range is origin-stamped
/// (it comes from the reference's authoring file, via
/// [`extract_references`] for inline refs or the verbatim node's own
/// range), so a consumer that resolves `target` relative to that origin
/// — and blames findings on it — stays origin-faithful across an include
/// merge.
#[derive(Debug, Clone)]
pub struct FileReference {
    /// The raw path target as authored (`./x.txt`, `../y`, `/abs`).
    pub target: String,
    /// Origin-stamped range to resolve against and blame.
    pub range: Range,
}

/// Collect every **non-include** file-path reference in the (merged)
/// `document`: inline [`ReferenceType::File`] (`[./x.txt]`, `[../y]`,
/// `[/abs]`) and the `src=` parameter of any verbatim block (image,
/// data, video, …) — `lex.include` excepted (see below).
///
/// This is the pure (no-IO) half of the `check --references` file-path
/// check: it gathers the targets and their origin-stamped ranges; the
/// caller performs filesystem resolution + existence (which needs a
/// resolution root and disk access, neither of which belongs in a pure
/// `&Document` analysis).
///
/// `lex.include src=` is intentionally **not** collected: it is an
/// *annotation*, not a verbatim block, so it never matches the verbatim
/// `src=` arm — and after include expansion it has been spliced out
/// entirely (its path already validated by the base command, #759).
///
/// Inline refs reuse [`extract_references`], whose ranges are already
/// origin-stamped (see `inline::ReferenceWalker::make_range`). Verbatim
/// `src=` carries the verbatim node's own range, which the include
/// resolver stamps with the authoring file's origin.
pub fn collect_file_references(document: &lex_core::lex::ast::Document) -> Vec<FileReference> {
    use lex_core::lex::ast::traits::AstNode;

    let mut refs = Vec::new();

    // Inline `[./x]` file references — origin-stamped via extract_references.
    crate::utils::for_each_text_content(document, &mut |text| {
        for reference in extract_references(text) {
            if let ReferenceType::File { target } = &reference.reference_type {
                if !target.trim().is_empty() {
                    refs.push(FileReference {
                        target: target.clone(),
                        range: reference.range.clone(),
                    });
                }
            }
        }
    });

    // Any verbatim block's `src=` parameter (image, data, video, …). The
    // verbatim's own range carries its origin; `lex.include` is an
    // annotation, not a verbatim block, so it is structurally excluded
    // here.
    //
    // Two normalizations the inline path gets for free but verbatim does
    // not, because a verbatim `src=` parameter is *not* pre-classified:
    //
    // - **Unquote.** `src="./x.png"` stores the raw, still-quoted value
    //   on `Parameter.value`; we resolve a *path*, so unquote via the
    //   canonical `Parameter::unquoted_value` (the same path
    //   `Annotation::include_src` takes) — otherwise existence-checks
    //   look for a filename that literally includes the quotes.
    // - **Skip URLs.** The media `src` is documented as "URL or path", so
    //   `src=https://…/d.png` is a URL, not a local file; checking it on
    //   disk would be a guaranteed false positive. (Inline refs are
    //   already classified `Url` vs `File`, so only this arm needs it.)
    for item in document.root.iter_all_nodes() {
        if let ContentItem::VerbatimBlock(verbatim) = item {
            if let Some(param) = verbatim
                .closing_data
                .parameters
                .iter()
                .find(|p| p.key == "src")
            {
                let target = param.unquoted_value();
                let trimmed = target.trim();
                if !trimmed.is_empty() && !is_url_like(trimmed) {
                    refs.push(FileReference {
                        target: target.clone(),
                        range: verbatim.range().clone(),
                    });
                }
            }
        }
    }

    refs
}

/// Is `src` a URL rather than a local file path? Mirrors the inline
/// reference classifier's URL detection (`http://`, `https://`,
/// `mailto:`) plus a generic `scheme://` catch, so a verbatim
/// `src=<url>` is excluded from the file-path existence check the same
/// way an inline `[<url>]` is classified `Url` and skipped.
///
/// The generic `scheme://` arm requires a *real* URL scheme rather than
/// a bare `"://"` substring: the part before `://` must be a valid
/// RFC 3986 scheme — start with an ASCII letter, then ASCII
/// alphanumerics / `+` / `-` / `.` — and be at least two characters
/// long. The length-≥2 floor is the point of the fix: a single-letter
/// "scheme" is exactly the Windows drive-letter ambiguity, so `C://path`
/// is *not* treated as a URL (it falls through to be resolved / flagged
/// as a platform-absolute path), while every real scheme we care about
/// (`http`, `https`, …) is ≥2 chars and still matches. `mailto:` has no
/// `//`, so it keeps its own explicit prefix arm.
pub(super) fn is_url_like(src: &str) -> bool {
    src.starts_with("http://")
        || src.starts_with("https://")
        || src.starts_with("mailto:")
        || has_url_scheme(src)
}

/// Does `src` begin with a genuine `scheme://` (a length-≥2 RFC 3986
/// scheme), as opposed to a Windows drive path like `C://…`?
fn has_url_scheme(src: &str) -> bool {
    let Some((scheme, _)) = src.split_once("://") else {
        return false;
    };
    scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}
