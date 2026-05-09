//! `lexd labels` subcommand handlers.
//!
//! Two operations land in this PR:
//!
//! - `lexd labels list` — print every registered namespace, its
//!   source, and its label count. Useful for "did the host see my
//!   `[labels]` block?" debugging.
//! - `lexd labels validate <doc>` — run the analysis pass against a
//!   document and print every diagnostic. Exits non-zero when at
//!   least one error-severity diagnostic fires.
//!
//! `lexd labels emit` and `lexd labels update` are deferred to the
//! follow-up that lands the network resolvers + cache (the `update`
//! subcommand only makes sense once cached entries can grow stale).

use std::path::Path;

use crate::extension_setup::{BootOutcome, NamespaceSourceKind};

/// Render `lexd labels list` output to stdout. Returns 0 on
/// success; this command never produces non-zero exits — bad
/// namespaces show up as warnings in the boot diagnostic stream
/// but the listing itself succeeds.
pub fn list(outcome: &BootOutcome) -> i32 {
    println!("Registered namespaces:");
    if outcome.registered.is_empty() {
        println!("  (none)");
    } else {
        for ns in &outcome.registered {
            let source = match &ns.source {
                NamespaceSourceKind::Builtin => "built-in".to_string(),
                NamespaceSourceKind::LexToml { uri } => format!("lex.toml: {uri}"),
                NamespaceSourceKind::ExtSchemaFlag { path } => {
                    format!("--ext-schema {}", path.display())
                }
            };
            println!(
                "  {} ({} schema{}) — {}",
                ns.name,
                ns.schema_count,
                if ns.schema_count == 1 { "" } else { "s" },
                source
            );
        }
    }
    if !outcome.diagnostics.is_empty() {
        println!();
        println!("Diagnostics:");
        for diag in &outcome.diagnostics {
            match &diag.namespace {
                Some(ns) => println!("  [{ns}] {}", diag.message),
                None => println!("  {}", diag.message),
            }
        }
    }
    0
}

/// Run the analysis pass against `doc_path` with the populated
/// registry and print every diagnostic. Returns the suggested
/// process exit code (0 = no errors, 1 = at least one
/// error-severity diagnostic).
pub fn validate(doc_path: &Path, outcome: &BootOutcome) -> i32 {
    use lex_analysis::diagnostics::{
        analyze_with_registry, AnalysisDiagnostic, DiagnosticSeverity,
    };
    use lex_core::lex::loader::DocumentLoader;

    let document = match DocumentLoader::from_path(doc_path).and_then(|l| l.parse()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "lexd labels validate: cannot parse `{}`: {e}",
                doc_path.display()
            );
            return 2;
        }
    };

    let diagnostics: Vec<AnalysisDiagnostic> = analyze_with_registry(&document, &outcome.registry);

    if diagnostics.is_empty() {
        println!("{}: no diagnostics", doc_path.display());
        return 0;
    }

    let mut had_error = false;
    for d in &diagnostics {
        let severity_str = match d.severity {
            DiagnosticSeverity::Error => {
                had_error = true;
                "error"
            }
            DiagnosticSeverity::Warning => "warning",
            DiagnosticSeverity::Info => "info",
            DiagnosticSeverity::Hint => "hint",
        };
        let line = d.range.start.line;
        let col = d.range.start.column;
        println!(
            "{}:{}:{}: {severity_str}: {}",
            doc_path.display(),
            line + 1,
            col + 1,
            d.message
        );
    }
    if had_error {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_setup::{boot_registry, ExtensionSetup, RegisteredNamespace};
    use lex_config::LabelsConfig;
    use lex_extension_host::Surface;

    #[test]
    fn list_prints_builtin_when_no_other_namespaces() {
        let workspace = tempfile::tempdir().unwrap();
        let labels = LabelsConfig::default();
        let outcome = boot_registry(ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[],
            enable_handlers: false,
            surface_override: Some(Surface::CliOneShot),
        });
        // We can't easily capture stdout without harnessing, but we
        // can confirm the function returns 0 and the outcome's
        // `registered` list contains the builtin.
        assert_eq!(list(&outcome), 0);
        assert!(outcome.registered.iter().any(|r| r.name == "lex"));
    }

    #[test]
    fn validate_returns_zero_for_clean_document() {
        let workspace = tempfile::tempdir().unwrap();
        let doc_path = workspace.path().join("hello.lex");
        std::fs::write(&doc_path, "Hello, world.\n").unwrap();
        let labels = LabelsConfig::default();
        let outcome = boot_registry(ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[],
            enable_handlers: false,
            surface_override: Some(Surface::CliOneShot),
        });
        assert_eq!(validate(&doc_path, &outcome), 0);
    }

    #[test]
    fn validate_returns_two_for_unparseable_path() {
        let workspace = tempfile::tempdir().unwrap();
        let labels = LabelsConfig::default();
        let outcome = boot_registry(ExtensionSetup {
            workspace_root: workspace.path(),
            labels_config: &labels,
            ext_schemas: &[],
            enable_handlers: false,
            surface_override: Some(Surface::CliOneShot),
        });
        // Pointing at a path that doesn't exist surfaces as exit 2.
        let missing = workspace.path().join("does-not-exist.lex");
        assert_eq!(validate(&missing, &outcome), 2);
    }

    /// Silence "unused" warning if RegisteredNamespace's fields go
    /// unused in test bodies above.
    #[test]
    fn registered_namespace_has_a_name() {
        let r = RegisteredNamespace {
            name: "x".into(),
            source: NamespaceSourceKind::Builtin,
            schema_count: 0,
        };
        assert_eq!(r.name, "x");
    }
}
