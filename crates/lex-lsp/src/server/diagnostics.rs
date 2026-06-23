//! Diagnostic construction: map internal error / analysis types into
//! LSP [`Diagnostic`]s.
//!
//! Three producers live here:
//! - [`include_error_to_diagnostic`] — an [`IncludeError`] from include
//!   resolution, pinned to the offending `lex.include` site when the
//!   error carries one.
//! - [`registry_setup_diagnostic`] — a document-head diagnostic for the
//!   (should-never-happen) registry-registration collision.
//! - [`to_lsp_diagnostic`] — an [`AnalysisDiagnostic`] from the stateless
//!   analysis stage.

use lex_analysis::diagnostics::{AnalysisDiagnostic, DiagnosticKind};
use lex_core::lex::includes::IncludeError;
use tower_lsp::lsp_types::Diagnostic;

use super::convert::{head_range, to_lsp_range};

/// Map an [`IncludeError`] to an LSP [`Diagnostic`].
///
/// The diagnostic's range points at the offending `lex.include`
/// annotation when the error carries one (Cycle, DepthExceeded,
/// NotFound, ContainerPolicy, MissingSrc, TotalIncludesExceeded,
/// FileTooLarge); otherwise it falls back to the document head
/// (line 0, column 0) so the user at least sees something in the
/// editor's diagnostics panel.
pub(crate) fn include_error_to_diagnostic(err: &IncludeError) -> Diagnostic {
    let (range, code, message) = match err {
        IncludeError::Cycle { include_site, .. } => {
            (to_lsp_range(include_site), "include-cycle", err.to_string())
        }
        IncludeError::DepthExceeded { include_site, .. } => (
            to_lsp_range(include_site),
            "include-depth-exceeded",
            err.to_string(),
        ),
        IncludeError::RootEscape { .. } => (head_range(), "include-root-escape", err.to_string()),
        IncludeError::AbsolutePath { .. } => {
            (head_range(), "include-absolute-path", err.to_string())
        }
        IncludeError::NotFound { include_site, .. } => (
            to_lsp_range(include_site),
            "include-not-found",
            err.to_string(),
        ),
        IncludeError::ParseFailed { .. } => (head_range(), "include-parse-failed", err.to_string()),
        IncludeError::ContainerPolicy { include_site, .. } => (
            to_lsp_range(include_site),
            "include-container-policy",
            err.to_string(),
        ),
        IncludeError::LoaderIo { .. } => (head_range(), "include-loader-io", err.to_string()),
        IncludeError::MissingSrc { include_site } => (
            to_lsp_range(include_site),
            "include-missing-src",
            err.to_string(),
        ),
        IncludeError::TotalIncludesExceeded { include_site, .. } => (
            to_lsp_range(include_site),
            "include-total-exceeded",
            err.to_string(),
        ),
        IncludeError::FileTooLarge { include_site, .. } => (
            to_lsp_range(include_site),
            "include-file-too-large",
            err.to_string(),
        ),
        IncludeError::HandlerFailed { include_site, .. } => (
            to_lsp_range(include_site),
            "include-handler-failed",
            err.to_string(),
        ),
    };
    Diagnostic {
        range,
        severity: Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR),
        code: Some(tower_lsp::lsp_types::NumberOrString::String(
            code.to_string(),
        )),
        code_description: None,
        source: Some("lex".to_string()),
        message,
        related_information: None,
        tags: None,
        data: None,
    }
}

/// Synthesize a document-head diagnostic when registry registration
/// fails (e.g., another path of the LSP already registered the `lex`
/// namespace and we collided). This should never happen in practice
/// — we build a fresh `Registry` per resolve call — but the path is
/// here so a future regression surfaces an editor diagnostic rather
/// than a silent panic.
pub(crate) fn registry_setup_diagnostic(message: &str) -> Diagnostic {
    Diagnostic {
        range: head_range(),
        severity: Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR),
        code: Some(tower_lsp::lsp_types::NumberOrString::String(
            "include-registry-setup".to_string(),
        )),
        code_description: None,
        source: Some("lex".to_string()),
        message: format!("could not configure include resolver: {message}"),
        related_information: None,
        tags: None,
        data: None,
    }
}

pub(crate) fn to_lsp_diagnostic(diag: AnalysisDiagnostic) -> Diagnostic {
    use lex_analysis::diagnostics::DiagnosticSeverity as AS;
    let severity = match diag.severity {
        AS::Error => tower_lsp::lsp_types::DiagnosticSeverity::ERROR,
        AS::Warning => tower_lsp::lsp_types::DiagnosticSeverity::WARNING,
        AS::Info => tower_lsp::lsp_types::DiagnosticSeverity::INFORMATION,
        AS::Hint => tower_lsp::lsp_types::DiagnosticSeverity::HINT,
    };

    let code = diag.kind.code().into_owned();

    let source = match &diag.kind {
        DiagnosticKind::Handler { namespace, .. } => format!("lex:{namespace}"),
        _ => "lex".to_string(),
    };

    Diagnostic {
        range: to_lsp_range(&diag.range),
        severity: Some(severity),
        code: Some(tower_lsp::lsp_types::NumberOrString::String(code)),
        code_description: None,
        source: Some(source),
        message: diag.message,
        related_information: None,
        tags: None,
        data: None,
    }
}
