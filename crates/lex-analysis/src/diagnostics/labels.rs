//! Label-policy diagnostics: forbidden `doc.*` prefixes and unknown
//! `lex.*` canonicals.
//!
//! Walks every label site in the document and re-classifies via
//! [`classify_label`](lex_core::lex::assembling::stages::normalize_labels::classify_label),
//! emitting diagnostics for sites that strict-mode parsing would have
//! rejected. The LSP-side permissive parse keeps the AST building so these
//! surface as in-place diagnostics rather than a wholesale parse failure.

use super::{AnalysisDiagnostic, DiagnosticKind, DiagnosticSeverity};
use lex_core::lex::ast::{Annotation, ContentItem, Document, Session};

/// Walk every label site in the document and re-classify via
/// [`classify_label`](lex_core::lex::assembling::stages::normalize_labels::classify_label).
/// Emits diagnostics for sites that strict-mode parsing would have
/// rejected — `doc.*` (forbidden) and unknown `lex.*` (not a
/// registered canonical). The LSP-side permissive parse keeps the
/// AST building so these surface as in-place diagnostics rather than
/// as a wholesale parse failure.
pub(super) fn check_labels(document: &Document, diagnostics: &mut Vec<AnalysisDiagnostic>) {
    use lex_core::lex::assembling::stages::normalize_labels::{
        classify_label, RejectReason, Resolution,
    };
    use lex_core::lex::ast::Label;

    fn emit(label: &Label, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        if let Resolution::Rejected(reason) = classify_label(&label.value) {
            // Reuse the normative wording from `RejectReason::message()`
            // so the strict-mode parser error and the permissive-mode
            // analysis diagnostic stay literally identical — no chance
            // of wording drift between the two surfaces.
            let message = reason.message();
            let kind = match reason {
                RejectReason::Forbidden { .. } => DiagnosticKind::ForbiddenLabelPrefix,
                RejectReason::UnknownCanonical { .. } => DiagnosticKind::UnknownLexCanonical,
            };
            diagnostics.push(AnalysisDiagnostic {
                range: label.location.clone(),
                severity: DiagnosticSeverity::Error,
                kind,
                message,
            });
        }
    }

    // Unified dispatch: every ContentItem flows through `walk_item`,
    // which emits the type-specific label sites (annotation label,
    // verbatim closer label, table cells/footnotes) exactly once and
    // then defers to `attached_annotations` + `item.children()` for
    // the uniform recursion. The earlier shape had type-specific
    // walkers (`walk_annotation`, `walk_verbatim`, `walk_table`) that
    // descended on their own and then `walk_item` descended again —
    // duplicate-walk regression caught by Copilot's review on PR 589.
    fn walk_item(item: &ContentItem, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        match item {
            ContentItem::Annotation(a) => emit(&a.data.label, diagnostics),
            ContentItem::VerbatimBlock(v) => emit(&v.closing_data.label, diagnostics),
            ContentItem::Table(t) => {
                for row in t.header_rows.iter().chain(t.body_rows.iter()) {
                    for cell in &row.cells {
                        for child in cell.children.iter() {
                            walk_item(child, diagnostics);
                        }
                    }
                }
                if let Some(footnotes) = t.footnotes.as_ref() {
                    for ann in footnotes.annotations() {
                        walk_annotation(ann, diagnostics);
                    }
                    for fn_item in footnotes.items.iter() {
                        walk_item(fn_item, diagnostics);
                    }
                }
            }
            _ => {}
        }
        // Attached annotations (sessions, paragraphs, lists, list
        // items, verbatim blocks, tables — see `attached_annotations`).
        if let Some(attached) = attached_annotations(item) {
            for annotation in attached {
                walk_annotation(annotation, diagnostics);
            }
        }
        // Generic child descent. For ContentItem::Annotation,
        // `item.children()` returns the annotation's body children, so
        // type-specific walking of nested annotations is not needed.
        if let Some(children) = item.children() {
            for child in children {
                walk_item(child, diagnostics);
            }
        }
    }

    fn walk_annotation(annotation: &Annotation, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        emit(&annotation.data.label, diagnostics);
        for child in annotation.children.iter() {
            walk_item(child, diagnostics);
        }
    }

    fn walk_session(session: &Session, diagnostics: &mut Vec<AnalysisDiagnostic>) {
        for annotation in session.annotations() {
            walk_annotation(annotation, diagnostics);
        }
        for child in &session.children {
            walk_item(child, diagnostics);
        }
    }

    fn attached_annotations(item: &ContentItem) -> Option<&[Annotation]> {
        match item {
            ContentItem::Session(s) => Some(s.annotations()),
            ContentItem::Paragraph(p) => Some(p.annotations()),
            ContentItem::Definition(d) => Some(d.annotations()),
            ContentItem::List(l) => Some(l.annotations()),
            ContentItem::ListItem(li) => Some(li.annotations()),
            ContentItem::VerbatimBlock(v) => Some(v.annotations()),
            ContentItem::Table(t) => Some(t.annotations()),
            _ => None,
        }
    }

    // Document-level annotations.
    for annotation in document.annotations() {
        walk_annotation(annotation, diagnostics);
    }
    // Root session walks.
    walk_session(&document.root, diagnostics);
}
