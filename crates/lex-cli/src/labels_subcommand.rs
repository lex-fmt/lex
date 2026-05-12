//! `lexd labels` subcommand handlers.
//!
//! - `lexd labels list` — print every registered namespace, its
//!   source, and its label count. Useful for "did the host see my
//!   `[labels]` block?" debugging.
//! - `lexd labels validate <doc>` — run the analysis pass against a
//!   document and print every diagnostic. Exits non-zero when at
//!   least one error-severity diagnostic fires.
//! - `lexd labels emit <doc> [--label X]... [--namespace N]...` —
//!   walk a document's labelled annotations / verbatims and write
//!   one NDJSON record per match to stdout. Pull-based export for
//!   downstream tools (static-site generators, indexers,
//!   pipelines).
//!
//! `lexd labels update` is deferred to the follow-up that ships the
//! real network resolvers — the subcommand only makes sense once
//! cached entries can grow stale (lex#562).

use std::io::Write;
use std::path::Path;

use lex_core::lex::ast::{ContentItem, Document, Session};
use lex_core::lex::wire::to_wire_node;
use lex_extension::wire::{NodeRef, WireNode};
use serde::Serialize;

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
                NamespaceSourceKind::Native => "native (embedder)".to_string(),
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

/// One NDJSON record emitted per labelled node. Shape pinned in
/// the [#564 contract decision]: positions use the wire `Range` /
/// `Position` types so the same parser that consumes hook wire AST
/// (LSP, handlers, etc.) consumes emit output unchanged.
///
/// [#564 contract decision]: https://github.com/lex-fmt/lex/issues/564
#[derive(Serialize)]
struct EmitRecord {
    label: String,
    namespace: String,
    node: NodeRef,
    params: serde_json::Value,
    body: BodyRecord,
}

/// Body shape for [`EmitRecord`]. Tagged-by-`kind` so consumers can
/// match on the variant without inspecting which other fields are
/// present.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BodyRecord {
    /// Marker-only annotation (schema `body.kind: none`).
    None,
    /// Opaque text body — verbatim contents, or annotations with
    /// schema `body.kind: text`. Without a registry, every parsed
    /// "non-Lex" body collapses to this variant.
    Text { text: String },
    /// Parsed Lex subtree — children are wire AST nodes the
    /// consumer can walk recursively.
    Lex { wire: Vec<WireNode> },
}

/// Walk `doc_path`'s labelled annotations and verbatims and write
/// one NDJSON record per match to stdout. Filters apply *before*
/// serialization: a record is emitted iff its label matches at
/// least one `--label` (when any are supplied) AND its namespace
/// matches at least one `--namespace` (when any are supplied).
/// Empty filter lists mean "no filter on that axis"; combining
/// both narrows the intersection.
///
/// Exit codes mirror [`validate`]: 0 on success (including zero
/// matches), 2 on parse failure. No registry is required —
/// `to_wire_node` produces the wire form without schema lookup, so
/// this command runs even against documents whose namespaces
/// aren't registered.
pub fn emit(doc_path: &Path, label_filter: &[String], namespace_filter: &[String]) -> i32 {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    emit_to_writer(doc_path, label_filter, namespace_filter, &mut out)
}

/// Internal entry point factored out for tests. Same semantics as
/// [`emit`] but writes to an arbitrary [`Write`] sink instead of
/// stdout — lets tests assert on the produced NDJSON without
/// shelling out or capturing the real stdout.
fn emit_to_writer<W: Write>(
    doc_path: &Path,
    label_filter: &[String],
    namespace_filter: &[String],
    out: &mut W,
) -> i32 {
    use lex_core::lex::loader::DocumentLoader;

    let document = match DocumentLoader::from_path(doc_path).and_then(|l| l.parse()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "lexd labels emit: cannot parse `{}`: {e}",
                doc_path.display()
            );
            return 2;
        }
    };

    let mut emit_one = |record: EmitRecord| -> std::io::Result<()> {
        let line = serde_json::to_string(&record).expect("EmitRecord serialises");
        writeln!(out, "{line}")
    };

    let mut walker = LabelWalker {
        label_filter,
        namespace_filter,
        emit: &mut emit_one,
    };
    if let Err(e) = walker.visit_document(&document) {
        eprintln!("lexd labels emit: write error: {e}");
        return 2;
    }
    0
}

/// Recursive walker that finds every labelled annotation and
/// verbatim in a parsed document and forwards each match to the
/// supplied closure as an [`EmitRecord`]. Filters are applied
/// early — a label that doesn't pass them is skipped before the
/// wire AST is built, so emitting from a large doc with a narrow
/// filter doesn't pay the wire-codec cost for every non-match.
struct LabelWalker<'a, F>
where
    F: FnMut(EmitRecord) -> std::io::Result<()>,
{
    label_filter: &'a [String],
    namespace_filter: &'a [String],
    emit: &'a mut F,
}

impl<'a, F> LabelWalker<'a, F>
where
    F: FnMut(EmitRecord) -> std::io::Result<()>,
{
    fn visit_document(&mut self, document: &Document) -> std::io::Result<()> {
        for ann in document.annotations() {
            self.visit_annotation_node(ann)?;
        }
        self.visit_session(&document.root)
    }

    fn visit_session(&mut self, session: &Session) -> std::io::Result<()> {
        for ann in session.annotations() {
            self.visit_annotation_node(ann)?;
        }
        for child in session.children.iter() {
            self.visit_content(child)?;
        }
        Ok(())
    }

    fn visit_content(&mut self, item: &ContentItem) -> std::io::Result<()> {
        match item {
            ContentItem::Paragraph(p) => {
                for ann in p.annotations() {
                    self.visit_annotation_node(ann)?;
                }
            }
            ContentItem::Session(s) => self.visit_session(s)?,
            ContentItem::Definition(d) => {
                for ann in d.annotations() {
                    self.visit_annotation_node(ann)?;
                }
                for child in d.children.iter() {
                    self.visit_content(child)?;
                }
            }
            ContentItem::List(list) => {
                for ann in list.annotations() {
                    self.visit_annotation_node(ann)?;
                }
                for entry in &list.items {
                    if let ContentItem::ListItem(li) = entry {
                        for ann in li.annotations() {
                            self.visit_annotation_node(ann)?;
                        }
                        for child in li.children.iter() {
                            self.visit_content(child)?;
                        }
                    }
                }
            }
            ContentItem::Annotation(a) => self.visit_annotation_node(a)?,
            ContentItem::VerbatimBlock(v) => {
                self.visit_verbatim_node(v)?;
                for ann in v.annotations() {
                    self.visit_annotation_node(ann)?;
                }
            }
            ContentItem::Table(_) | ContentItem::ListItem(_) => {
                // Table cells + list items have no label-carrying
                // syntax of their own; the parent list walker
                // already drained list-item annotations above, and
                // tables don't carry annotations in v1.
            }
            _ => {
                // Other content kinds (blank lines, raw text
                // fragments) don't carry labels.
            }
        }
        Ok(())
    }

    fn visit_annotation_node(
        &mut self,
        annotation: &lex_core::lex::ast::Annotation,
    ) -> std::io::Result<()> {
        let label = annotation.data.label.value.clone();
        if label.is_empty() {
            return Ok(());
        }
        if !self.passes_filters(&label) {
            return Ok(());
        }
        let wire = to_wire_node(&ContentItem::Annotation(annotation.clone()));
        if let WireNode::Annotation {
            label,
            params,
            body,
            range,
            origin,
        } = wire
        {
            let namespace = namespace_of(&label);
            let body_record = body_from_value(body);
            (self.emit)(EmitRecord {
                label,
                namespace,
                node: NodeRef {
                    kind: "annotation".into(),
                    range,
                    origin,
                },
                params,
                body: body_record,
            })?;
        }
        Ok(())
    }

    fn visit_verbatim_node(
        &mut self,
        verbatim: &lex_core::lex::ast::Verbatim,
    ) -> std::io::Result<()> {
        let label = verbatim.closing_data.label.value.clone();
        if label.is_empty() {
            return Ok(());
        }
        if !self.passes_filters(&label) {
            return Ok(());
        }
        let wire = to_wire_node(&ContentItem::VerbatimBlock(Box::new(verbatim.clone())));
        if let WireNode::Verbatim {
            params,
            body_text,
            range,
            origin,
            ..
        } = wire
        {
            let namespace = namespace_of(&label);
            (self.emit)(EmitRecord {
                label,
                namespace,
                node: NodeRef {
                    kind: "verbatim".into(),
                    range,
                    origin,
                },
                params,
                body: BodyRecord::Text { text: body_text },
            })?;
        }
        Ok(())
    }

    /// Check label against `--label` and `--namespace` filters.
    /// Empty filter list means "no filter on that axis."
    fn passes_filters(&self, label: &str) -> bool {
        let label_ok = self.label_filter.is_empty() || self.label_filter.iter().any(|l| l == label);
        let namespace_ok = self.namespace_filter.is_empty()
            || self
                .namespace_filter
                .iter()
                .any(|n| n == &namespace_of(label));
        label_ok && namespace_ok
    }
}

/// Derive the namespace prefix from a fully-qualified label.
/// `"acme.task"` → `"acme"`; `"lone-label"` (no dot) → the whole
/// label, treated as a single-segment namespace.
fn namespace_of(label: &str) -> String {
    label
        .split_once('.')
        .map(|(ns, _)| ns.to_string())
        .unwrap_or_else(|| label.to_string())
}

/// Convert the wire-codec's `body` JSON value (which uses the
/// untagged null/string/object form from
/// [`lex_extension::AnnotationBody`]) into the explicitly-tagged
/// [`BodyRecord`] shape this command's output contract uses.
fn body_from_value(body: serde_json::Value) -> BodyRecord {
    match body {
        serde_json::Value::Null => BodyRecord::None,
        serde_json::Value::String(text) => BodyRecord::Text { text },
        serde_json::Value::Object(map) => {
            // Wire form for parsed bodies: `{ "kind": "block", "children": [...] }`.
            // Extract children as Vec<WireNode>; on parse failure, fall
            // back to None so a malformed wire body doesn't drop the
            // record entirely.
            let children_value = map
                .get("children")
                .cloned()
                .unwrap_or(serde_json::Value::Array(Vec::new()));
            match serde_json::from_value::<Vec<WireNode>>(children_value) {
                Ok(wire) => BodyRecord::Lex { wire },
                Err(_) => BodyRecord::None,
            }
        }
        _ => BodyRecord::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_setup::{boot_registry, ExtensionSetup};
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

    // -- `lexd labels emit` tests. The helpers below build a doc on
    // disk, run `emit_to_writer` against an in-memory buffer, and
    // assert on the produced NDJSON. Each line is a separate record;
    // tests parse them back as JSON to compare structurally rather
    // than byte-equality-matching, since position offsets are
    // sensitive to exact whitespace in the test doc.

    fn write_doc(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let ws = tempfile::tempdir().unwrap();
        let path = ws.path().join("doc.lex");
        std::fs::write(&path, content).unwrap();
        (ws, path)
    }

    fn run_emit(
        doc_path: &Path,
        labels: &[&str],
        namespaces: &[&str],
    ) -> (i32, Vec<serde_json::Value>) {
        let labels: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
        let namespaces: Vec<String> = namespaces.iter().map(|s| s.to_string()).collect();
        let mut buf: Vec<u8> = Vec::new();
        let code = emit_to_writer(doc_path, &labels, &namespaces, &mut buf);
        let out = String::from_utf8(buf).unwrap();
        let records: Vec<serde_json::Value> = out
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).expect("emit output must be valid JSON"))
            .collect();
        (code, records)
    }

    #[test]
    fn emit_yields_one_record_per_labelled_annotation() {
        let (_ws, path) = write_doc(
            "\
:: acme.task priority=\"high\" ::
    body text here.

Plain paragraph with no label.

:: other.foo ::
    nested.
",
        );
        let (code, records) = run_emit(&path, &[], &[]);
        assert_eq!(code, 0);
        let labels: Vec<&str> = records
            .iter()
            .map(|r| r["label"].as_str().unwrap())
            .collect();
        assert!(
            labels.contains(&"acme.task"),
            "expected acme.task in {labels:?}"
        );
        assert!(
            labels.contains(&"other.foo"),
            "expected other.foo in {labels:?}"
        );
    }

    #[test]
    fn emit_label_filter_restricts_output() {
        let (_ws, path) = write_doc(
            "\
:: acme.task ::
    one.

:: acme.note ::
    two.

:: other.thing ::
    three.
",
        );
        let (_, records) = run_emit(&path, &["acme.task"], &[]);
        let labels: Vec<&str> = records
            .iter()
            .map(|r| r["label"].as_str().unwrap())
            .collect();
        assert_eq!(labels, vec!["acme.task"]);
    }

    #[test]
    fn emit_namespace_filter_restricts_output() {
        let (_ws, path) = write_doc(
            "\
:: acme.task ::
    one.

:: acme.note ::
    two.

:: other.thing ::
    three.
",
        );
        let (_, records) = run_emit(&path, &[], &["acme"]);
        let namespaces: Vec<&str> = records
            .iter()
            .map(|r| r["namespace"].as_str().unwrap())
            .collect();
        assert!(namespaces.iter().all(|n| *n == "acme"));
        assert_eq!(namespaces.len(), 2);
    }

    #[test]
    fn emit_label_and_namespace_filters_intersect() {
        let (_ws, path) = write_doc(
            "\
:: acme.task ::
    one.

:: acme.note ::
    two.

:: other.task ::
    three (same label, different namespace).
",
        );
        let (_, records) = run_emit(&path, &["acme.task"], &["acme"]);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["label"], "acme.task");
        assert_eq!(records[0]["namespace"], "acme");
    }

    #[test]
    fn emit_record_uses_wire_position_types() {
        // Contract check: the NDJSON shape uses the wire AST's
        // Position serialization (Position is a 2-tuple [line,
        // character]; Range is an object with start/end Positions).
        // Same shape LSP hover and extension hook payloads use, so
        // a single parser consumes both.
        let (_ws, path) = write_doc(":: acme.task ::\n    body.\n");
        let (_, records) = run_emit(&path, &[], &[]);
        assert_eq!(records.len(), 1);
        let range = &records[0]["node"]["range"];
        assert!(
            range["start"].is_array(),
            "start should be a Position tuple"
        );
        assert!(range["end"].is_array(), "end should be a Position tuple");
        assert_eq!(range["start"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn emit_empty_doc_yields_no_records() {
        let (_ws, path) = write_doc("Just a paragraph, nothing else.\n");
        let (code, records) = run_emit(&path, &[], &[]);
        assert_eq!(code, 0);
        assert!(records.is_empty());
    }

    #[test]
    fn emit_parse_error_returns_two_and_writes_nothing() {
        let workspace = tempfile::tempdir().unwrap();
        let missing = workspace.path().join("does-not-exist.lex");
        let mut buf: Vec<u8> = Vec::new();
        let code = emit_to_writer(&missing, &[], &[], &mut buf);
        assert_eq!(code, 2);
        assert!(buf.is_empty(), "no stdout output on parse failure");
    }
}
