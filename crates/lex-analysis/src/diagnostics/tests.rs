use super::annotations::looks_like_unclosed_annotation;
use super::references::{is_url_like, url_is_malformed};
use super::*;
use lex_core::lex::parsing::parse_document_permissive;
use lex_core::lex::testing::lexplore::Lexplore;

fn unclosed_annotation_diags(source: &str) -> Vec<AnalysisDiagnostic> {
    let doc = parse_document_permissive(source).expect("permissive parse");
    analyze(&doc)
        .into_iter()
        .filter(|d| d.kind == DiagnosticKind::UnclosedAnnotation)
        .collect()
}

#[test]
fn unclosed_annotation_warns_on_open_form() {
    // `:: note severity=high` (no closing `::`) parses as a paragraph; the
    // analyser flags it so the author knows it isn't an annotation (lex#700).
    let diags = unclosed_annotation_diags("Open form:\n\t:: note severity=high\n");
    assert_eq!(diags.len(), 1, "expected one unclosed-annotation warning");
    assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
    assert_eq!(diags[0].kind.code(), "unclosed-annotation");
}

#[test]
fn unclosed_annotation_silent_on_closed_form_and_prose() {
    // A properly closed annotation is its own node, not a flagged paragraph.
    assert!(unclosed_annotation_diags(":: note severity=high ::\n\nBody.\n").is_empty());
    // Prose that merely mentions `::` is not flagged.
    assert!(unclosed_annotation_diags("Use :: to start a marker.\n").is_empty());
}

#[test]
fn looks_like_unclosed_annotation_heuristic() {
    assert!(looks_like_unclosed_annotation(":: note"));
    assert!(looks_like_unclosed_annotation("    :: note severity=high"));
    // A `::` inside a quoted value is not a structural close, so this is still
    // an unclosed annotation (lex#704 review).
    assert!(looks_like_unclosed_annotation(":: note foo=\":: value\""));
    assert!(!looks_like_unclosed_annotation(":: note ::"));
    assert!(!looks_like_unclosed_annotation(
        ":: note foo=\":: value\" ::"
    )); // real close
    assert!(!looks_like_unclosed_annotation("::note")); // no whitespace after marker
    assert!(!looks_like_unclosed_annotation("::")); // no label
    assert!(!looks_like_unclosed_annotation("just prose"));
}

fn footnote_diags(doc: &Document) -> Vec<AnalysisDiagnostic> {
    analyze(doc)
        .into_iter()
        .filter(|d| d.kind == DiagnosticKind::MissingFootnoteDefinition)
        .collect()
}

fn label_diags(source: &str) -> Vec<AnalysisDiagnostic> {
    let doc = parse_document_permissive(source).expect("permissive parse");
    analyze(&doc)
        .into_iter()
        .filter(|d| {
            matches!(
                d.kind,
                DiagnosticKind::ForbiddenLabelPrefix | DiagnosticKind::UnknownLexCanonical
            )
        })
        .collect()
}

#[test]
fn check_labels_emits_for_doc_prefix() {
    let diags = label_diags(":: doc.table :: x\n\nBody.\n");
    assert_eq!(diags.len(), 1, "expected 1 forbidden-prefix diagnostic");
    assert_eq!(diags[0].kind, DiagnosticKind::ForbiddenLabelPrefix);
    assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
    assert!(
        diags[0].message.contains("doc.table") && diags[0].message.contains("reserved"),
        "message names the offending prefix; got: {}",
        diags[0].message
    );
}

#[test]
fn check_labels_emits_for_unknown_lex_canonical() {
    let diags = label_diags(":: lex.foobar :: x\n\nBody.\n");
    assert_eq!(diags.len(), 1, "expected 1 unknown-canonical diagnostic");
    assert_eq!(diags[0].kind, DiagnosticKind::UnknownLexCanonical);
    assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
    assert!(
        diags[0].message.contains("lex.foobar"),
        "message names the offending label; got: {}",
        diags[0].message
    );
}

#[test]
fn check_labels_silent_on_accepted_forms() {
    // Shortcut, prefix-stripped, canonical, and community labels
    // all accept silently — analysis only flags the two reject
    // categories from `classify_label`.
    let sources = [
        ":: author :: Alice\n\nBody.\n",
        ":: metadata.author :: Alice\n\nBody.\n",
        ":: lex.metadata.author :: Alice\n\nBody.\n",
        ":: acme.task :: x\n\nBody.\n",
    ];
    for src in sources {
        let diags = label_diags(src);
        assert!(
            diags.is_empty(),
            "no label diagnostics expected for {src:?}; got {diags:?}"
        );
    }
}

#[test]
fn check_labels_finds_verbatim_closer_violations() {
    let diags =
        label_diags("Table:\n    | a | b |\n    |---|---|\n    | 1 | 2 |\n:: doc.table ::\n");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, DiagnosticKind::ForbiddenLabelPrefix);
}

#[test]
fn check_labels_emits_each_offending_site_exactly_once() {
    // Regression for Copilot's PR 589 callout: the earlier
    // walker shape descended into a node's children twice (once
    // via the type-specific helper, once via the generic
    // `walk_item` fallback), which produced duplicate
    // diagnostics for any forbidden label nested inside another
    // label-bearing site. Three nested + adjacent forbidden
    // labels should produce exactly three diagnostics, not six.
    let src = ":: doc.outer ::\n    :: doc.inner :: nested body\n\n:: doc.sibling :: x\n";
    let diags = label_diags(src);
    assert_eq!(
        diags.len(),
        3,
        "exactly one diagnostic per offending site: {diags:?}"
    );
    for d in &diags {
        assert_eq!(d.kind, DiagnosticKind::ForbiddenLabelPrefix);
    }
}

#[test]
fn detects_missing_footnote_definition() {
    let doc = Lexplore::footnotes(1).parse().unwrap();
    let diags = analyze(&doc);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, DiagnosticKind::MissingFootnoteDefinition);
}

#[test]
fn ignores_valid_footnote_with_notes_annotation() {
    // :: notes :: annotated list at the document root provides the definitions
    let doc = Lexplore::footnotes(2).parse().unwrap();
    assert!(footnote_diags(&doc).is_empty());
}

#[test]
fn ignores_valid_list_footnote_in_session() {
    // :: notes :: inside a session
    let doc = Lexplore::footnotes(3).parse().unwrap();
    assert!(footnote_diags(&doc).is_empty());
}

#[test]
fn list_without_notes_annotation_is_not_footnotes() {
    // A "Notes" session without :: notes :: does NOT define footnotes
    let doc = Lexplore::footnotes(4).parse().unwrap();
    assert_eq!(footnote_diags(&doc).len(), 1);
}

fn table_diags(doc: &Document) -> Vec<AnalysisDiagnostic> {
    analyze(doc)
        .into_iter()
        .filter(|d| d.kind == DiagnosticKind::TableInconsistentColumns)
        .collect()
}

#[test]
fn detects_inconsistent_table_columns() {
    // table-13: 3-col header, 2-col row, 3-col row — middle row is short.
    let doc = Lexplore::table(13).parse().unwrap();
    let diags = table_diags(&doc);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("2 columns"));
    assert!(diags[0].message.contains("expected 3"));
}

#[test]
fn consistent_table_no_diagnostic() {
    // table-01: minimal 2-column table, all rows consistent.
    let doc = Lexplore::table(1).parse().unwrap();
    assert!(table_diags(&doc).is_empty());
}

#[test]
fn table_with_rowspan_counts_carry_over() {
    // table-17: rowspan via ^^ — effective widths remain consistent across rows.
    let doc = Lexplore::table(17).parse().unwrap();
    let diags = table_diags(&doc);
    assert!(
        diags.is_empty(),
        "rowspan carry-over should not trigger inconsistent-columns, got: {diags:?}"
    );
}

#[test]
fn table_with_colspan_and_rowspan_mixed() {
    // table-18: combined >> colspan and ^^ rowspan; effective widths stay consistent.
    let doc = Lexplore::table(18).parse().unwrap();
    let diags = table_diags(&doc);
    assert!(
        diags.is_empty(),
        "mixed colspan/rowspan should not trigger inconsistent-columns, got: {diags:?}"
    );
}

#[test]
fn table_with_colspan_counts_effective_width() {
    // table-04: colspan via >> contributes to effective width; all rows consistent.
    let doc = Lexplore::table(4).parse().unwrap();
    assert!(table_diags(&doc).is_empty());
}

#[test]
fn footnote_ref_in_table_cell_is_checked() {
    // footnotes-09: table cell contains [1] but no footnote definition
    // anywhere in scope — document, session, or table-local.
    let doc = Lexplore::footnotes(9).parse().unwrap();
    let diags = footnote_diags(&doc);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("[1]"));
}

#[test]
fn table_scoped_footnotes_resolve_cell_refs() {
    // footnotes-11: cell refs [1] and [2] resolve to the table's own
    // positional footnote list (no :: notes :: annotation needed).
    let doc = Lexplore::footnotes(11).parse().unwrap();
    let diags = footnote_diags(&doc);
    assert!(
        diags.is_empty(),
        "table-scoped cell refs should resolve to table.footnotes, got: {diags:?}"
    );
}

#[test]
fn table_scoped_footnotes_do_not_leak_out() {
    // footnotes-12: a [1] ref in body text outside the table must NOT
    // resolve to the table's own positional footnote list even when the
    // numbers happen to match. The table's list is table-local.
    let doc = Lexplore::footnotes(12).parse().unwrap();
    let diags = footnote_diags(&doc);
    assert_eq!(
        diags.len(),
        1,
        "only the paragraph ref [1] should be unresolved, got: {diags:?}"
    );
    assert!(diags[0].message.contains("[1]"));
}

// ─────────────── apply_rules / DiagnosticKind::code ───────────────

fn dummy_diag(kind: DiagnosticKind, severity: DiagnosticSeverity) -> AnalysisDiagnostic {
    AnalysisDiagnostic {
        range: Range::default(),
        severity,
        kind,
        message: "test".into(),
    }
}

#[test]
fn diagnostic_kind_code_matches_lookup_for_every_builtin() {
    // Drift test: every built-in DiagnosticKind variant must have a
    // matching entry in DiagnosticsRulesConfig::lookup_by_code so
    // configuration overrides reach every rule.
    let rules = DiagnosticsRulesConfig::default();
    for kind in [
        DiagnosticKind::MissingFootnoteDefinition,
        DiagnosticKind::UnusedFootnoteDefinition,
        DiagnosticKind::TableInconsistentColumns,
        DiagnosticKind::ForbiddenLabelPrefix,
        DiagnosticKind::UnknownLexCanonical,
        DiagnosticKind::SchemaValidation(SchemaValidationKind::UnknownLabel),
        DiagnosticKind::SchemaValidation(SchemaValidationKind::MissingParam),
        DiagnosticKind::SchemaValidation(SchemaValidationKind::ParamTypeMismatch),
        DiagnosticKind::SchemaValidation(SchemaValidationKind::BadAttachment),
        DiagnosticKind::SchemaValidation(SchemaValidationKind::BodyShapeMismatch),
    ] {
        let code = kind.code();
        assert!(
            rules.lookup_by_code(&code).is_some(),
            "DiagnosticsRulesConfig is missing a field for built-in code {code:?} \
             — add it to lookup_by_code (and likely as a struct field too)"
        );
    }
}

#[test]
fn handler_code_carries_namespace_prefix() {
    // Wire-shape contract (spec §9): the wire `code` is the
    // namespace-prefixed form so a `.lex.toml` rule like
    // `"acme.task-stuck" = "deny"` actually matches what the
    // handler emitted. The handler supplies the bare leaf (`code`
    // field on `Diagnostic`); the analyser glues on the namespace.
    let with_code = DiagnosticKind::Handler {
        namespace: "acme".into(),
        code: Some("task-stuck".into()),
    };
    assert_eq!(with_code.code(), "acme.task-stuck");
    // Code-less handler diagnostic gets a per-namespace fallback
    // — users can target it as `"acme.diagnostic" = "warn"` rather
    // than a single global literal.
    let without_code = DiagnosticKind::Handler {
        namespace: "acme".into(),
        code: None,
    };
    assert_eq!(without_code.code(), "acme.diagnostic");
}

#[test]
fn apply_rules_matches_extension_code_via_side_channel() {
    // End-to-end: handler emits `acme.foo`, user configured
    // `"acme.foo" = "allow"` in `[diagnostics.rules]` (now
    // captured into the LSP's `extension_diagnostic_rules`
    // side-channel by the `on_unknown_key` callback rather than
    // landing in a `#[serde(flatten)] extra` map); diagnostic
    // gets dropped.
    use std::collections::BTreeMap;
    // The closure mirrors `LoadedLexConfig::lookup_diagnostic_rule`:
    // built-in first, side-channel second.
    let lookup = |code: &str, side: &BTreeMap<String, lex_config::RuleConfig>| {
        DiagnosticsRulesConfig::default()
            .lookup_by_code(code)
            .cloned()
            .or_else(|| side.get(code).cloned())
    };

    let side: BTreeMap<String, lex_config::RuleConfig> = [(
        "acme.foo".to_string(),
        lex_config::RuleConfig::Bare(Severity::Allow),
    )]
    .into_iter()
    .collect();
    let mut diags = vec![dummy_diag(
        DiagnosticKind::Handler {
            namespace: "acme".into(),
            code: Some("foo".into()),
        },
        DiagnosticSeverity::Error,
    )];
    apply_rules(&mut diags, |code| lookup(code, &side));
    assert!(diags.is_empty(), "allow drops the extension diagnostic");

    // `warn` keeps the intrinsic severity (Error stays Error).
    let side: BTreeMap<String, lex_config::RuleConfig> = [(
        "acme.foo".to_string(),
        lex_config::RuleConfig::Bare(Severity::Warn),
    )]
    .into_iter()
    .collect();
    let mut diags = vec![dummy_diag(
        DiagnosticKind::Handler {
            namespace: "acme".into(),
            code: Some("foo".into()),
        },
        DiagnosticSeverity::Error,
    )];
    apply_rules(&mut diags, |code| lookup(code, &side));
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].severity,
        DiagnosticSeverity::Error,
        "warn preserves the handler's intrinsic severity"
    );

    // `deny` is a no-op when the intrinsic is already Error, but
    // still keeps the diagnostic — symmetry with built-ins.
    let side: BTreeMap<String, lex_config::RuleConfig> = [(
        "acme.foo".to_string(),
        lex_config::RuleConfig::Bare(Severity::Deny),
    )]
    .into_iter()
    .collect();
    let mut diags = vec![dummy_diag(
        DiagnosticKind::Handler {
            namespace: "acme".into(),
            code: Some("foo".into()),
        },
        DiagnosticSeverity::Error,
    )];
    apply_rules(&mut diags, |code| lookup(code, &side));
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, DiagnosticSeverity::Error);

    // A configured rule whose code doesn't match the emitted one
    // passes the diagnostic through untouched.
    let side: BTreeMap<String, lex_config::RuleConfig> = [(
        "acme.other".to_string(),
        lex_config::RuleConfig::Bare(Severity::Allow),
    )]
    .into_iter()
    .collect();
    let mut diags = vec![dummy_diag(
        DiagnosticKind::Handler {
            namespace: "acme".into(),
            code: Some("foo".into()),
        },
        DiagnosticSeverity::Warning,
    )];
    apply_rules(&mut diags, |code| lookup(code, &side));
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
}

#[test]
fn apply_rules_allow_drops_diagnostic() {
    let mut diags = vec![dummy_diag(
        DiagnosticKind::MissingFootnoteDefinition,
        DiagnosticSeverity::Error,
    )];
    let rules = DiagnosticsRulesConfig {
        missing_footnote: lex_config::RuleConfig::Bare(Severity::Allow),
        ..Default::default()
    };
    apply_rules(&mut diags, |code| rules.lookup_by_code(code).cloned());
    assert!(diags.is_empty(), "allow should drop the diagnostic");
}

#[test]
fn apply_rules_deny_upgrades_to_error() {
    let mut diags = vec![dummy_diag(
        DiagnosticKind::TableInconsistentColumns,
        DiagnosticSeverity::Warning,
    )];
    let rules = DiagnosticsRulesConfig {
        table_inconsistent_columns: lex_config::RuleConfig::Bare(Severity::Deny),
        ..Default::default()
    };
    apply_rules(&mut diags, |code| rules.lookup_by_code(code).cloned());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
}

#[test]
fn apply_rules_warn_keeps_intrinsic_severity() {
    let mut diags = vec![dummy_diag(
        DiagnosticKind::TableInconsistentColumns,
        DiagnosticSeverity::Warning,
    )];
    let rules = DiagnosticsRulesConfig {
        table_inconsistent_columns: lex_config::RuleConfig::Bare(Severity::Warn),
        ..Default::default()
    };
    apply_rules(&mut diags, |code| rules.lookup_by_code(code).cloned());
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].severity,
        DiagnosticSeverity::Warning,
        "warn should not change the intrinsic severity"
    );
}

#[test]
fn apply_rules_unknown_code_is_passthrough() {
    // An extension-emitted diagnostic with a code the registry
    // does not know about must pass through unmodified. The
    // handler's `code` is the bare leaf — the analyser glues on
    // `acme.` to produce wire `acme.unknown`.
    let mut diags = vec![dummy_diag(
        DiagnosticKind::Handler {
            namespace: "acme".into(),
            code: Some("unknown".into()),
        },
        DiagnosticSeverity::Warning,
    )];
    let rules = DiagnosticsRulesConfig::default();
    apply_rules(&mut diags, |code| rules.lookup_by_code(code).cloned());
    assert_eq!(diags.len(), 1, "unknown codes should pass through");
    assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
}

#[test]
fn apply_rules_preserves_order_of_kept_diagnostics() {
    // Mixed stream: one to drop, one to keep, one to upgrade.
    let mut diags = vec![
        dummy_diag(
            DiagnosticKind::MissingFootnoteDefinition,
            DiagnosticSeverity::Error,
        ),
        dummy_diag(
            DiagnosticKind::UnusedFootnoteDefinition,
            DiagnosticSeverity::Warning,
        ),
        dummy_diag(
            DiagnosticKind::TableInconsistentColumns,
            DiagnosticSeverity::Warning,
        ),
    ];
    let rules = DiagnosticsRulesConfig {
        missing_footnote: lex_config::RuleConfig::Bare(Severity::Allow),
        table_inconsistent_columns: lex_config::RuleConfig::Bare(Severity::Deny),
        ..Default::default()
    };
    apply_rules(&mut diags, |code| rules.lookup_by_code(code).cloned());
    assert_eq!(diags.len(), 2);
    assert_eq!(diags[0].kind, DiagnosticKind::UnusedFootnoteDefinition);
    assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
    assert_eq!(diags[1].kind, DiagnosticKind::TableInconsistentColumns);
    assert_eq!(diags[1].severity, DiagnosticSeverity::Error);
}

// ========================================================================
// analyze_references (opt-in `check --references`) unit tests
// ========================================================================

fn reference_diags(source: &str) -> Vec<AnalysisDiagnostic> {
    let doc = parse_document_permissive(source).expect("permissive parse");
    analyze_references(&doc)
}

fn ref_codes(source: &str) -> Vec<String> {
    let mut codes: Vec<String> = reference_diags(source)
        .into_iter()
        .map(|d| d.kind.code().into_owned())
        .collect();
    codes.sort();
    codes
}

#[test]
fn references_pass_is_not_run_by_the_always_on_analyser() {
    // A dangling definition reference produces nothing from `analyze`
    // (the always-on path) — only the opt-in pass flags it. This pins
    // the separation that keeps the LSP from emitting these unasked.
    let doc =
        parse_document_permissive("Body with a [Dangling] reference.\n").expect("permissive parse");
    let always_on = analyze(&doc);
    assert!(
        always_on
            .iter()
            .all(|d| !d.kind.code().starts_with("missing-")
                || d.kind == DiagnosticKind::MissingFootnoteDefinition),
        "always-on analyser must not emit reference-target diagnostics"
    );
}

#[test]
fn dangling_definition_reference_flagged() {
    let codes = ref_codes("1. Intro\n\n    See [Nope].\n");
    assert_eq!(codes, vec!["missing-definition-target"]);
}

#[test]
fn dangling_session_reference_flagged() {
    let codes = ref_codes("1. Intro\n\n    See [#9.9].\n");
    assert_eq!(codes, vec!["missing-session-target"]);
}

#[test]
fn dangling_annotation_reference_flagged() {
    let codes = ref_codes("1. Intro\n\n    See [::ghost].\n");
    assert_eq!(codes, vec!["missing-annotation-target"]);
}

#[test]
fn dangling_citation_flagged() {
    let codes = ref_codes("1. Intro\n\n    See [@missing2024].\n");
    assert_eq!(codes, vec!["missing-citation-target"]);
}

#[test]
fn resolved_references_are_clean() {
    // Definition + annotation + session all defined; references to
    // each resolve and produce no findings.
    let source = ":: mynote ::\n\
         \x20   Note body.\n\
         \n\
         Cache:\n\
         \x20   Definition body.\n\
         \n\
         2. Topic\n\
         \n\
         \x20   See [Cache] and [::mynote] and [#2].\n";
    assert!(
        reference_diags(source).is_empty(),
        "resolved references must be clean: {:?}",
        reference_diags(source)
    );
}

#[test]
fn citation_resolves_via_annotation_label() {
    // `[@spec]` resolves to a `:: spec ::` annotation (its label is a
    // citation key too).
    let source = ":: spec ::\n    Body.\n\n1. Intro\n\n    See [@spec].\n";
    assert!(reference_diags(source).is_empty());
}

#[test]
fn annotation_matching_is_case_insensitive() {
    // `[::MyNote]` resolves to `:: mynote ::` — resolution is
    // case-insensitive, mirroring `references::reference_matches`.
    let source = ":: mynote ::\n    Body.\n\n1. Intro\n\n    See [::MyNote].\n";
    assert!(reference_diags(source).is_empty());
}

#[test]
fn placeholders_never_flagged() {
    // `[TK]` / `[TK-id]` and an unclassifiable reference are
    // intentional placeholders — never flagged.
    assert!(reference_diags("1. Intro\n\n    A [TK] and [TK-later].\n").is_empty());
}

#[test]
fn each_unresolved_citation_key_is_flagged() {
    // A multi-key citation flags each unresolved key independently —
    // both `@a` and `@b`, not just the first. Exactly two pins the
    // per-key behaviour against a regression that reports only one.
    let diags = reference_diags("1. Intro\n\n    See [@a; @b].\n");
    let citation = diags
        .iter()
        .filter(|d| d.kind == DiagnosticKind::MissingCitationTarget)
        .count();
    assert_eq!(
        citation, 2,
        "both unresolved keys must be flagged: {diags:?}"
    );
}

#[test]
fn reference_findings_default_to_warning() {
    let diags = reference_diags("1. Intro\n\n    See [Nope].\n");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
}

// ========================================================================
// URL well-formedness (issue #762). Validated inside analyze_references;
// pure parse, no network.
// ========================================================================

#[test]
fn malformed_url_embedded_space_flagged() {
    // An embedded space makes the URL unparseable.
    let codes = ref_codes("1. Intro\n\n    See [https://exa mple.com].\n");
    assert_eq!(codes, vec!["malformed-url"]);
}

#[test]
fn malformed_url_empty_host_flagged() {
    // `https://` with no host is well-formed-prefix but empty-host.
    let codes = ref_codes("1. Intro\n\n    See [https:// ].\n");
    assert_eq!(codes, vec!["malformed-url"]);
}

#[test]
fn well_formed_https_url_not_flagged() {
    assert!(
        reference_diags("1. Intro\n\n    See [https://example.com/path?q=1].\n").is_empty(),
        "a well-formed https URL must not be flagged"
    );
}

#[test]
fn well_formed_http_url_not_flagged() {
    assert!(reference_diags("1. Intro\n\n    See [http://example.com].\n").is_empty());
}

#[test]
fn well_formed_mailto_not_flagged() {
    // `mailto:` has no host component — an empty host is expected and
    // must not be flagged.
    assert!(
        reference_diags("1. Intro\n\n    Write [mailto:hi@example.com].\n").is_empty(),
        "a well-formed mailto must not be flagged"
    );
}

#[test]
fn malformed_url_defaults_to_warning() {
    let diags = reference_diags("1. Intro\n\n    See [https://exa mple.com].\n");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, DiagnosticKind::MalformedUrl);
    assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
}

#[test]
fn url_check_makes_no_network_calls_by_construction() {
    // `url_is_malformed` is a pure parse over a string — it borrows no
    // socket, client, or runtime, so the default check path cannot
    // make a network call. Exercising both a well-formed and a
    // malformed URL here documents that the only work done is parsing.
    assert!(!url_is_malformed("https://example.com"));
    assert!(url_is_malformed("https://exa mple.com"));
    assert!(!url_is_malformed("mailto:a@b.com"));
}

// ========================================================================
// collect_file_references (file-path pass, #761) unit tests
// ========================================================================

fn file_ref_targets(source: &str) -> Vec<String> {
    let doc = parse_document_permissive(source).expect("permissive parse");
    let mut targets: Vec<String> = collect_file_references(&doc)
        .into_iter()
        .map(|r| r.target)
        .collect();
    targets.sort();
    targets
}

#[test]
fn collects_inline_file_references() {
    // The three inline file-reference shapes (`./`, `../`, `/`) are
    // all collected; a non-file `[General]` reference is not.
    let source = "1. Intro\n\n    See [./a.txt] and [../b] and [/c] but not [Nope].\n";
    assert_eq!(
        file_ref_targets(source),
        vec!["../b".to_string(), "./a.txt".to_string(), "/c".to_string()]
    );
}

#[test]
fn collects_verbatim_src_but_not_lex_include() {
    // An image verbatim `src=` is collected. `lex.include` is an
    // annotation (not a verbatim block) and is structurally excluded
    // — collecting it here would double-validate a path the base
    // command already checks via expansion.
    let source = "Photo:\n    Caption.\n:: image src=./diagram.png ::\n\n";
    assert_eq!(file_ref_targets(source), vec!["./diagram.png".to_string()]);
}

#[test]
fn verbatim_src_is_unquoted() {
    // A quoted `src="./x.png"` is collected as the bare path, not the
    // still-quoted raw value — otherwise the existence check looks for
    // a filename that literally includes the quotes.
    let source = "Photo:\n    Caption.\n:: image src=\"./diagram.png\" ::\n\n";
    assert_eq!(file_ref_targets(source), vec!["./diagram.png".to_string()]);
}

#[test]
fn ignores_url_references() {
    // URLs are out of scope for the file-path pass (#762 owns them).
    // Inline `[<url>]` is classified `Url` (not `File`); a verbatim
    // `src=<url>` is not pre-classified, so the collector filters it.
    assert!(file_ref_targets("1. Intro\n\n    See [https://example.com].\n").is_empty());
    assert!(file_ref_targets(
        "Photo:\n    Caption.\n:: image src=https://example.com/diagram.png ::\n\n"
    )
    .is_empty());
    // Quoted URL form, too.
    assert!(file_ref_targets(
        "Photo:\n    Caption.\n:: image src=\"https://example.com/diagram.png\" ::\n\n"
    )
    .is_empty());
}

#[test]
fn is_url_like_matches_real_schemes_not_windows_drives() {
    // A genuine `scheme://` URL is URL-like and filtered out.
    assert!(is_url_like("https://example.com"));
    assert!(is_url_like("http://example.com"));
    assert!(is_url_like("mailto:user@example.com"));
    // A length-≥2 custom scheme still matches.
    assert!(is_url_like("ftp://host/path"));
    // A Windows drive path is NOT a URL — its single-letter "scheme"
    // is exactly the ambiguity the length-≥2 floor disambiguates.
    assert!(!is_url_like("C://path"));
    assert!(!is_url_like("C:\\path"));
    // A plain relative path is not URL-like.
    assert!(!is_url_like("./rel/path"));
}
