//! Unclosed-annotation diagnostic (lex#700).
//!
//! Warns on paragraph lines shaped like an annotation header (`:: label`)
//! that never close the `:: ::` marker. There is no "open form": such a
//! line is kept as paragraph text rather than dropped, so this surfaces
//! that the author likely meant an annotation and forgot the trailing `::`.

use super::{AnalysisDiagnostic, DiagnosticKind, DiagnosticSeverity};
use lex_core::lex::ast::{ContentItem, Document};

/// Warn on paragraph lines that look like an annotation header but never close
/// the `:: ::` marker (lex#700). There is no "open form": `:: label` with no
/// closing `::` is not a recognized element, so the parser keeps it as paragraph
/// text rather than dropping it. This surfaces that — the author likely meant an
/// annotation and forgot the trailing `::`.
pub(super) fn check_unclosed_annotations(
    document: &Document,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
    fn emit(
        tl: &lex_core::lex::ast::elements::paragraph::TextLine,
        out: &mut Vec<AnalysisDiagnostic>,
    ) {
        if looks_like_unclosed_annotation(tl.text()) {
            out.push(AnalysisDiagnostic {
                range: tl.location.clone(),
                severity: DiagnosticSeverity::Warning,
                kind: DiagnosticKind::UnclosedAnnotation,
                message: "this line looks like an annotation but has no closing `::`, \
                          so it is treated as text. Close the marker to make it an \
                          annotation, e.g. `:: label ::`."
                    .to_string(),
            });
        }
    }

    fn walk(item: &ContentItem, out: &mut Vec<AnalysisDiagnostic>) {
        if let ContentItem::Paragraph(p) = item {
            for line in &p.lines {
                if let ContentItem::TextLine(tl) = line {
                    emit(tl, out);
                }
            }
        }
        if let Some(children) = item.children() {
            for child in children {
                walk(child, out);
            }
        }
    }

    for child in &document.root.children {
        walk(child, diagnostics);
    }
}

/// True when a line is shaped like an annotation header (`:: label …`) but has no
/// closing `::`. Detection is intentionally a lightweight text heuristic — by the
/// time content reaches the analyser, a *closed* annotation is already its own
/// node, so any `::`-leading paragraph line is the unclosed shape.
pub(super) fn looks_like_unclosed_annotation(text: &str) -> bool {
    let Some(rest) = text.trim().strip_prefix("::") else {
        return false;
    };
    // A second *structural* `::` means a closed marker — not the unclosed shape.
    // Scan quote-aware so a `::` inside a quoted parameter value (e.g.
    // `:: note foo=":: value"`) does not count as a close, matching how the
    // lexer's structural-marker detection treats it.
    let mut in_quotes = false;
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes && chars.peek() == Some(&':') => return false,
            _ => {}
        }
    }
    // Require whitespace after the opening marker, then a label-shaped token
    // (label.lex: a letter, then letters/digits/`_`/`-`/`.`).
    let label = rest.trim_start();
    rest.len() != label.len() && label.chars().next().is_some_and(|c| c.is_alphabetic())
}
