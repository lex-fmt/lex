//! `lexd check` — document linter over the include-expanded AST.
//!
//! `check` is the CLI consumer of `lex-analysis` diagnostics (the LSP is
//! the other). It runs `analyze_with_rules` over the **merged** document
//! (includes expanded by default), maps include-assembly failures into
//! diagnostics, applies `.lex.toml [diagnostics.rules]` severities, and
//! reports findings with a CI-friendly exit-code contract.
//!
//! ## Pipeline (per file)
//!
//! 1. Read source. An unreadable file is an *operational* error (exit 2),
//!    kept distinct from "the document has findings" (exit 1).
//! 2. Parse + (by default) expand `lex.include` — same resolver branch
//!    `convert`/`inspect` take. Include-assembly errors
//!    ([`IncludeError`]) surface here, blamed on the include site.
//! 3. Boot the extension registry so schema/handler diagnostics fire.
//! 4. `analyze_with_rules(doc, registry, rules)` — built-in + extension
//!    diagnostics, with `[diagnostics.rules]` severities applied.
//!
//! ## Origin-faithful reporting
//!
//! Every finding prints its true source via [`Range::origin`], not the
//! entry file: a footnote/table problem that originates inside an
//! included file is blamed on that file. The include resolver stamps
//! `origin_path` on every node of the merged tree (including the entry
//! file's own nodes), so the origin is authoritative; when it is absent
//! (e.g. `--no-includes`, which never runs the resolver) we fall back to
//! the entry path.
//!
//! ## Extension seam
//!
//! [`collect_file_diagnostics`] returns the full finding set for one
//! file as a flat `Vec<CheckFinding>`. A future `--references` pass
//! (epic #758, issues #760–#762) appends its post-merge reference
//! diagnostics to that same vector before reporting — the reporting and
//! exit-code layers operate on `CheckFinding`, not on any analysis-
//! specific type, so they need no change to absorb new finding sources.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lex_analysis::diagnostics::{
    analyze_with_rules, AnalysisDiagnostic, DiagnosticSeverity as AnalysisSeverity,
};
use lex_config::{DiagnosticsRulesConfig, LabelsConfig, CONFIG_FILE_NAME};
use lex_core::lex::ast::{Position, Range};
use lex_core::lex::builtins;
use lex_core::lex::includes::{resolve_from_source, FsLoader, IncludeError, ResolveConfig};
use lex_core::lex::parsing::parse_document_permissive;
use lex_extension_host::registry::Registry;
use serde::Serialize;

use crate::extension_setup::{boot_registry, ExtensionSetup};

/// Severity threshold the `--fail-on` flag selects. Ordered so a
/// finding "meets" the threshold when its severity is *at least* as
/// strong (Error is strongest). The default is `Warning`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

impl Severity {
    /// Rank with Error highest. A finding fails the gate when its rank
    /// is `>=` the threshold's rank.
    fn rank(self) -> u8 {
        match self {
            Severity::Error => 3,
            Severity::Warning => 2,
            Severity::Info => 1,
            Severity::Hint => 0,
        }
    }

    /// Lower-case wire/CLI spelling (used in human output and `--fail-on`).
    fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
            Severity::Hint => "hint",
        }
    }

    /// Parse the `--fail-on` argument value. Returns `None` for an
    /// unrecognised token so the caller can raise an operational error.
    pub fn parse(s: &str) -> Option<Severity> {
        match s {
            "error" => Some(Severity::Error),
            "warning" => Some(Severity::Warning),
            "info" => Some(Severity::Info),
            "hint" => Some(Severity::Hint),
            _ => None,
        }
    }

    fn from_analysis(s: AnalysisSeverity) -> Severity {
        match s {
            AnalysisSeverity::Error => Severity::Error,
            AnalysisSeverity::Warning => Severity::Warning,
            AnalysisSeverity::Info => Severity::Info,
            AnalysisSeverity::Hint => Severity::Hint,
        }
    }
}

/// One reported finding, decoupled from any analysis-specific type so
/// the reporting/exit-code layers absorb new finding sources (e.g. the
/// future `--references` pass) without change. `path` is the
/// origin-faithful source file (an included file's path when the
/// finding originates inside it), already resolved from
/// [`Range::origin`] with the entry path as fallback.
#[derive(Debug, Clone)]
pub struct CheckFinding {
    pub path: PathBuf,
    pub range: Range,
    pub severity: Severity,
    pub code: String,
    pub message: String,
}

impl CheckFinding {
    /// 1-based line for human output (analysis ranges are 0-based).
    fn line(&self) -> usize {
        self.range.start.line + 1
    }

    /// 1-based column for human output.
    fn column(&self) -> usize {
        self.range.start.column + 1
    }
}

/// JSON shape for `--format json`: an array of these. `range` mirrors
/// the analysis `Range` (0-based positions); all other fields are
/// flattened to the strings the contract names.
#[derive(Serialize)]
struct JsonFinding<'a> {
    path: String,
    range: JsonRange,
    severity: &'a str,
    code: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct JsonRange {
    start: JsonPosition,
    end: JsonPosition,
}

#[derive(Serialize)]
struct JsonPosition {
    line: usize,
    column: usize,
}

impl JsonRange {
    fn from_range(range: &Range) -> JsonRange {
        JsonRange {
            start: JsonPosition {
                line: range.start.line,
                column: range.start.column,
            },
            end: JsonPosition {
                line: range.end.line,
                column: range.end.column,
            },
        }
    }
}

/// Output format selector for `--format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

/// Knobs for one `check` run, assembled by the CLI dispatch from parsed
/// args + loaded config. `rules` is borrowed for the lifetime of the run.
///
/// Note `[labels]` is NOT carried here: it is loaded *per entry* inside
/// [`collect_file_diagnostics`] from the same workspace that anchors the
/// registry boot ([`workspace_for`] of the entry). Deriving both from
/// one workspace keeps them consistent when linting files that live in
/// different workspaces — a CWD-loaded `[labels]` would otherwise be
/// applied against a per-entry `workspace_root`, mis-resolving namespaces.
pub struct CheckOptions<'a> {
    /// Expand `lex.include` before analysing (default true; `--no-includes`
    /// clears it).
    pub expand_includes: bool,
    /// Explicit include-root override (`--includes-root` / `[includes].root`).
    pub includes_root: Option<PathBuf>,
    pub max_depth: usize,
    pub max_total_includes: usize,
    pub max_file_size: u64,
    /// Severity at/above which a finding fails the run (exit 1).
    pub fail_on: Severity,
    pub format: OutputFormat,
    /// `[diagnostics.rules]` from the resolved `.lex.toml`.
    pub rules: &'a DiagnosticsRulesConfig,
    /// Extra `--ext-schema` namespace directories/files.
    pub ext_schemas: &'a [PathBuf],
    /// Whether subprocess handlers are permitted (`--enable-handlers`).
    pub enable_handlers: bool,
}

/// Load the workspace `[labels]` block for an entry from `workspace`'s
/// `.lex.toml`. A missing file yields the default config; a malformed
/// one is an operational error (mapped to exit 2 by the caller). Cached
/// per workspace so a batch of files sharing a workspace pays the load
/// (and the TOML parse) once.
fn labels_for_workspace<'c>(
    workspace: &Path,
    cache: &'c mut std::collections::HashMap<PathBuf, LabelsConfig>,
) -> Result<&'c LabelsConfig, String> {
    let key = workspace.to_path_buf();
    if !cache.contains_key(&key) {
        let labels_path = workspace.join(CONFIG_FILE_NAME);
        let config = match lex_config::load_labels_from_toml(&labels_path) {
            Ok(c) => c,
            Err(lex_config::LabelsConfigError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                LabelsConfig::default()
            }
            Err(e) => return Err(format!("{e}")),
        };
        cache.insert(key.clone(), config);
    }
    Ok(cache.get(&key).expect("just inserted"))
}

/// Per-file outcome: either the collected findings, or an operational
/// failure (unreadable file etc.) that maps to exit 2.
enum FileOutcome {
    Findings(Vec<CheckFinding>),
    Operational(String),
}

/// Run `check` over every input path and return the process exit code:
///
/// - `0` — clean (no finding at/above `fail_on` and no operational error).
/// - `1` — at least one finding met the `fail_on` threshold.
/// - `2` — an operational error on any file (unreadable, etc.).
///
/// The aggregate code across files is the max, so one unreadable file in
/// a batch yields 2 even if every other file was clean.
pub fn run(paths: &[PathBuf], opts: &CheckOptions<'_>) -> i32 {
    let mut all_findings: Vec<(PathBuf, Vec<CheckFinding>)> = Vec::new();
    let mut had_operational = false;
    let mut operational_messages: Vec<String> = Vec::new();
    // `[labels]` cache, keyed by each entry's resolved workspace, so a
    // batch sharing one workspace loads it once.
    let mut labels_cache: std::collections::HashMap<PathBuf, LabelsConfig> =
        std::collections::HashMap::new();

    for entry in paths {
        match collect_file_outcome(entry, opts, &mut labels_cache) {
            FileOutcome::Findings(findings) => {
                all_findings.push((entry.clone(), findings));
            }
            FileOutcome::Operational(msg) => {
                had_operational = true;
                operational_messages.push(msg);
            }
        }
    }

    // Operational errors always go to stderr regardless of format so a
    // JSON consumer's stdout stays a clean findings array.
    for msg in &operational_messages {
        eprintln!("lexd check: {msg}");
    }

    let meets_threshold = report(&all_findings, opts);

    if had_operational {
        2
    } else if meets_threshold {
        1
    } else {
        0
    }
}

/// Collect findings for a single entry file, or report an operational
/// failure. Factored so tests can drive one file without the
/// aggregate/exit-code wrapper.
fn collect_file_outcome(
    entry: &Path,
    opts: &CheckOptions<'_>,
    labels_cache: &mut std::collections::HashMap<PathBuf, LabelsConfig>,
) -> FileOutcome {
    let source = match std::fs::read_to_string(entry) {
        Ok(s) => s,
        Err(e) => {
            return FileOutcome::Operational(format!("cannot read {}: {e}", entry.display()));
        }
    };
    match collect_file_diagnostics(entry, &source, opts, labels_cache) {
        Ok(findings) => FileOutcome::Findings(findings),
        Err(msg) => FileOutcome::Operational(msg),
    }
}

/// The extension seam. Parse + expand + analyse one file's `source` and
/// return its findings. A future `--references` pass appends its
/// post-merge reference diagnostics to the returned vector before it
/// reaches the reporting layer.
///
/// `Err(String)` is an operational failure (registry boot, fatal parse)
/// that the caller maps to exit 2. Include-assembly failures are *not*
/// errors here — they are mapped into findings, since "this include
/// won't load" is a document problem the linter reports, not a CLI
/// failure.
pub fn collect_file_diagnostics(
    entry: &Path,
    source: &str,
    opts: &CheckOptions<'_>,
    labels_cache: &mut std::collections::HashMap<PathBuf, LabelsConfig>,
) -> Result<Vec<CheckFinding>, String> {
    // Boot the extension registry from the workspace `[labels]` block so
    // schema/handler diagnostics fire — same boot the LSP and
    // `labels validate` perform. The `[labels]` config is loaded from the
    // SAME workspace that anchors the boot (`workspace_for(entry)`), so a
    // file outside the CWD's workspace gets its own workspace's labels,
    // not a mismatched CWD config. Surface boot diagnostics (unresolvable
    // namespaces, trust denials, …) to stderr so a silently un-booted
    // namespace — whose schema/handler diagnostics then never run — is
    // visible; stdout stays reserved for findings (important under
    // `--format json`), matching `lexd config gen` / `labels list`.
    let workspace = workspace_for(entry);
    let labels_config = labels_for_workspace(&workspace, labels_cache)?;
    let outcome = boot_registry(ExtensionSetup {
        workspace_root: &workspace,
        labels_config,
        ext_schemas: opts.ext_schemas,
        enable_handlers: opts.enable_handlers,
        surface_override: Some(lex_extension_host::Surface::CliOneShot),
    });
    for diag in &outcome.diagnostics {
        match &diag.namespace {
            Some(ns) => eprintln!("lexd check: [{ns}] {}", diag.message),
            None => eprintln!("lexd check: {}", diag.message),
        }
    }

    let mut findings: Vec<CheckFinding> = Vec::new();

    // Resolve the document we analyse. With includes on (and the source
    // actually using the feature) we run the resolver; assembly failures
    // become findings against the include site. Otherwise we parse
    // permissively so label-policy diagnostics still surface.
    let document = if opts.expand_includes && source.contains("lex.include") {
        let entry_abs = absolutize(entry);
        // Default include root mirrors `convert`/`inspect`
        // (`IncludeOptions::resolved_root`): the nearest ancestor
        // containing `.lex.toml`, falling back to the entry file's own
        // directory — NOT the entry directory unconditionally, which
        // would spuriously trip `include-root-escape` for valid
        // workspace-relative includes when the entry lives in a subdir.
        let root = opts
            .includes_root
            .clone()
            .map(|r| absolutize(&r))
            .unwrap_or_else(|| workspace_for(&entry_abs));
        let resolve_config = ResolveConfig {
            root: root.clone(),
            max_depth: opts.max_depth,
            max_total_includes: opts.max_total_includes,
        };
        let loader = FsLoader::new(root).with_max_file_size(opts.max_file_size);
        let registry = Registry::new();
        if let Err(e) = builtins::register_into(&registry, Arc::new(loader), resolve_config.clone())
        {
            return Err(format!(
                "could not configure include resolver for {}: {e}",
                entry.display()
            ));
        }
        match resolve_from_source(source, Some(entry_abs.clone()), &resolve_config, &registry) {
            Ok(doc) => doc,
            Err(err) => {
                // The document did not assemble. Report the assembly
                // failure as a finding blamed on its include site and
                // stop here: analysing the *un*-expanded fallback tree
                // would surface misleading diagnostics about the very
                // `lex.include` annotation that failed to resolve (e.g.
                // a `schema.bad-attachment` on the unspliced annotation),
                // which is noise, not a separate document problem.
                findings.push(include_error_finding(&err, entry));
                return Ok(findings);
            }
        }
    } else {
        parse_document_permissive(source)
            .map_err(|e| format!("{} could not be parsed: {e}", entry.display()))?
    };

    let diagnostics: Vec<AnalysisDiagnostic> =
        analyze_with_rules(&document, &outcome.registry, opts.rules);

    for diag in diagnostics {
        findings.push(analysis_finding(diag, entry));
    }

    Ok(findings)
}

/// Map an analyser diagnostic into a [`CheckFinding`], resolving the
/// origin-faithful source path: the diagnostic's range `origin` when
/// the resolver stamped one, otherwise the entry file.
fn analysis_finding(diag: AnalysisDiagnostic, entry: &Path) -> CheckFinding {
    let path = diag
        .range
        .origin()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| entry.to_path_buf());
    CheckFinding {
        path,
        severity: Severity::from_analysis(diag.severity),
        code: diag.kind.code().into_owned(),
        message: diag.message,
        range: diag.range,
    }
}

/// Map an [`IncludeError`] into a [`CheckFinding`]. Variants that carry
/// an `include_site` are blamed on that site (origin-faithful: the
/// site's own range origin when set, else the entry file); the four
/// site-less variants (`RootEscape`, `AbsolutePath`, `ParseFailed`,
/// `LoaderIo`) are blamed on the document head of the entry file. Codes
/// mirror the LSP's `include-*` family at `server.rs`.
fn include_error_finding(err: &IncludeError, entry: &Path) -> CheckFinding {
    let (site, code): (Option<&Range>, &str) = match err {
        IncludeError::Cycle { include_site, .. } => (Some(include_site), "include-cycle"),
        IncludeError::DepthExceeded { include_site, .. } => {
            (Some(include_site), "include-depth-exceeded")
        }
        IncludeError::TotalIncludesExceeded { include_site, .. } => {
            (Some(include_site), "include-total-exceeded")
        }
        IncludeError::FileTooLarge { include_site, .. } => {
            (Some(include_site), "include-file-too-large")
        }
        IncludeError::NotFound { include_site, .. } => (Some(include_site), "include-not-found"),
        IncludeError::ContainerPolicy { include_site, .. } => {
            (Some(include_site), "include-container-policy")
        }
        IncludeError::MissingSrc { include_site } => (Some(include_site), "include-missing-src"),
        IncludeError::HandlerFailed { include_site, .. } => {
            (Some(include_site), "include-handler-failed")
        }
        IncludeError::RootEscape { .. } => (None, "include-root-escape"),
        IncludeError::AbsolutePath { .. } => (None, "include-absolute-path"),
        IncludeError::ParseFailed { .. } => (None, "include-parse-failed"),
        IncludeError::LoaderIo { .. } => (None, "include-loader-io"),
    };

    let range = site.cloned().unwrap_or_else(head_range);
    let path = range
        .origin()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| entry.to_path_buf());

    CheckFinding {
        path,
        range,
        severity: Severity::Error,
        code: code.to_string(),
        message: err.to_string(),
    }
}

/// A zero-width range at the document head — fallback for include
/// errors with no anchorable site.
fn head_range() -> Range {
    Range::new(0..0, Position::new(0, 0), Position::new(0, 0))
}

/// Best-effort absolutize for resolver paths (canonicalize, falling back
/// to cwd-join). Mirrors `main.rs::absolutize_path` so include resolution
/// behaves identically to `convert`/`inspect`.
fn absolutize(p: &Path) -> PathBuf {
    if let Ok(canon) = p.canonicalize() {
        return canon;
    }
    if p.is_absolute() {
        return p.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(p))
        .unwrap_or_else(|_| p.to_path_buf())
}

/// Emit the report and return `true` when any finding met the `fail_on`
/// threshold. Human format groups per file with a trailing summary;
/// JSON emits a single flat array across all files.
fn report(files: &[(PathBuf, Vec<CheckFinding>)], opts: &CheckOptions<'_>) -> bool {
    let mut any_meets = false;
    for (_, findings) in files {
        for f in findings {
            if f.severity.rank() >= opts.fail_on.rank() {
                any_meets = true;
            }
        }
    }

    match opts.format {
        OutputFormat::Json => print_json(files),
        OutputFormat::Human => print_human(files),
    }

    any_meets
}

/// Human report: `path:line:col: severity: message [code]`, grouped per
/// file, with a trailing one-line summary. Silent when there are no
/// findings at all (clean docs print nothing — exit 0).
fn print_human(files: &[(PathBuf, Vec<CheckFinding>)]) {
    let total: usize = files.iter().map(|(_, f)| f.len()).sum();
    if total == 0 {
        return;
    }

    let mut first = true;
    for (entry, findings) in files {
        if findings.is_empty() {
            continue;
        }
        if !first {
            println!();
        }
        first = false;
        // Header names the entry file; individual lines carry the
        // origin-faithful path (which may be an included file).
        println!("{}:", entry.display());
        for f in findings {
            println!(
                "  {}:{}:{}: {}: {} [{}]",
                f.path.display(),
                f.line(),
                f.column(),
                f.severity.as_str(),
                f.message,
                f.code,
            );
        }
    }

    let files_with_findings = files.iter().filter(|(_, f)| !f.is_empty()).count();
    println!();
    println!(
        "{total} finding{} across {files_with_findings} file{}",
        if total == 1 { "" } else { "s" },
        if files_with_findings == 1 { "" } else { "s" },
    );
}

/// JSON report: a single flat array of findings across all files, in the
/// `{path, range, severity, code, message}` shape. Always valid JSON,
/// including `[]` for a clean run.
fn print_json(files: &[(PathBuf, Vec<CheckFinding>)]) {
    let records: Vec<JsonFinding> = files
        .iter()
        .flat_map(|(_, findings)| findings.iter())
        .map(|f| JsonFinding {
            path: f.path.display().to_string(),
            range: JsonRange::from_range(&f.range),
            severity: f.severity.as_str(),
            code: &f.code,
            message: &f.message,
        })
        .collect();
    // Serialization of this owned, string-only shape cannot fail.
    let json = serde_json::to_string_pretty(&records).expect("check findings serialise");
    println!("{json}");
}

/// Resolve the `.lex.toml` workspace directory for the given entry file
/// — nearest ancestor containing the config file, else the entry's own
/// directory. Used to boot the extension registry relative to the
/// document.
pub fn workspace_for(entry: &Path) -> PathBuf {
    // `Path::parent` of a bare filename (`file.lex`) is `Some("")`, and
    // an empty path canonicalizes to an error and `pop`s to `false`
    // immediately — which would skip the ancestor walk entirely. Treat
    // an empty parent as "." (the current directory) so the walk runs.
    let fallback = entry
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut cur = fallback.canonicalize().unwrap_or_else(|_| fallback.clone());
    loop {
        if cur.join(CONFIG_FILE_NAME).is_file() {
            return cur;
        }
        if !cur.pop() {
            return fallback;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_threshold_ranks_error_highest() {
        assert!(Severity::Error.rank() > Severity::Warning.rank());
        assert!(Severity::Warning.rank() > Severity::Info.rank());
        assert!(Severity::Info.rank() > Severity::Hint.rank());
    }

    #[test]
    fn severity_parse_round_trips_the_legal_set() {
        for s in ["error", "warning", "info", "hint"] {
            assert_eq!(Severity::parse(s).expect("legal").as_str(), s);
        }
        assert!(Severity::parse("bogus").is_none());
    }

    #[test]
    fn workspace_for_bare_filename_walks_from_cwd() {
        // A bare filename has an empty `parent()`; `workspace_for` must
        // treat that as "." and still return a real directory (not the
        // empty path, which would skip the ancestor walk). We don't
        // assert *which* dir — only that it is non-empty and absolute-ish
        // (canonicalized "." is absolute), proving the empty-path guard.
        let ws = workspace_for(Path::new("file.lex"));
        assert!(!ws.as_os_str().is_empty(), "must not return the empty path");
    }
}
