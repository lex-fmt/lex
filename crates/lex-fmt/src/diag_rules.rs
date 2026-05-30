//! Validate extension-emitted `[diagnostics.rules]` entries against the
//! resolved registry.
//!
//! `[diagnostics.rules]` accepts two key spaces. Built-in codes
//! (`missing_footnote`, `spellcheck`, …) are typed fields on
//! [`lex_config::DiagnosticsRulesConfig`] — clapfig validates them at
//! load and rejects typos. Extension codes (`<namespace>.<code>`, e.g.
//! `acme.task-due-date-missing`) are open-ended, so they land in
//! [`lex_config::LoadedLexConfig::extension_diagnostic_rules`]
//! unchecked: any dotted key the loader doesn't recognise is accepted.
//!
//! That leniency hides a class of mistakes — a rule whose `<code>` is
//! misspelled, or names a code the namespace never declares, silently
//! matches nothing. The diagnostic it was meant to retune is never
//! retuned and nothing says so. Once a namespace's schema declares its
//! diagnostic codes (`lex_extension::schema::DiagnosticDecl`), the host
//! can cross-check each rule against the registry and surface the dead
//! letters.
//!
//! Classification of each `<namespace>.<code>` entry:
//!
//! - **Namespace not registered** — pass silently. Matches the labels
//!   system's bounded-extensibility rule: users may stage rules ahead of
//!   installing the extension that provides them.
//! - **Namespace registered, `<code>` declared** — pass. The happy path.
//! - **Namespace registered, `<code>` not declared** — a finding, with a
//!   closest-match suggestion and the list of codes the namespace does
//!   declare.

use std::collections::BTreeMap;

use lex_config::RuleConfig;
use lex_extension::DiagnosticDecl;
use lex_extension_host::Registry;

/// A dead-letter `[diagnostics.rules]` entry: a `<namespace>.<code>`
/// rule whose namespace is registered but doesn't declare the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRuleFinding {
    /// The offending on-the-wire key (`<namespace>.<code>`).
    pub key: String,
    /// Human-readable explanation, including a suggestion when a declared
    /// code is a near miss and the list of declared codes.
    pub message: String,
}

/// Cross-check every extension diagnostic-rule key against the registry.
///
/// `extension_rules` is
/// [`LoadedLexConfig::extension_diagnostic_rules`](lex_config::LoadedLexConfig::extension_diagnostic_rules)
/// — the side-channel map of `<namespace>.<code>` rules the loader
/// collected. Returns one [`DiagnosticRuleFinding`] per dead-letter
/// entry, in the input's (sorted) key order.
pub fn validate_extension_diagnostic_rules(
    extension_rules: &BTreeMap<String, RuleConfig>,
    registry: &Registry,
) -> Vec<DiagnosticRuleFinding> {
    let mut findings = Vec::new();
    for key in extension_rules.keys() {
        // Split on the FIRST dot: `acme.task-due-date-missing` →
        // namespace `acme`, code `task-due-date-missing`. Bare keys
        // (no dot) can't reach this map — the config loader rejects
        // unknown un-dotted keys under `[diagnostics.rules]` at load —
        // so a missing dot is defensive and skipped.
        let Some((namespace, code)) = key.split_once('.') else {
            continue;
        };
        // `None` → namespace not registered → forward-compatible
        // pass-through (the user may install the extension later).
        let Some(declared) = registry.declared_diagnostic_codes(namespace) else {
            continue;
        };
        if declared.iter().any(|d| d.code == code) {
            continue;
        }
        findings.push(DiagnosticRuleFinding {
            key: key.clone(),
            message: format_finding(namespace, code, &declared),
        });
    }
    findings
}

fn format_finding(namespace: &str, code: &str, declared: &[DiagnosticDecl]) -> String {
    let mut msg = format!(
        "`[diagnostics.rules]` entry `{namespace}.{code}`: namespace \
         `{namespace}` declares no diagnostic code `{code}`"
    );
    if let Some(suggestion) = closest_code(code, declared) {
        msg.push_str(&format!(" — did you mean `{namespace}.{suggestion}`?"));
    }
    if declared.is_empty() {
        msg.push_str(&format!(
            " (namespace `{namespace}` declares no diagnostic codes)"
        ));
    } else {
        let list = declared
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        msg.push_str(&format!(" (declared codes: {list})"));
    }
    msg
}

/// The declared code closest to `code` by edit distance, when it's a
/// near miss. Threshold scales with the typo'd code's length so short
/// codes need an exact-ish match while longer ones tolerate a couple of
/// transpositions.
fn closest_code<'a>(code: &str, declared: &'a [DiagnosticDecl]) -> Option<&'a str> {
    let threshold = (code.len() / 3).max(2);
    declared
        .iter()
        .map(|d| (levenshtein(code, &d.code), d.code.as_str()))
        .filter(|(dist, _)| *dist <= threshold)
        .min_by_key(|(dist, _)| *dist)
        .map(|(_, c)| c)
}

/// Classic two-row Levenshtein edit distance over Unicode scalar values.
fn levenshtein(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0usize; b_chars.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b_chars.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_config::Severity;
    use lex_extension::schema::{BodyShape, Capabilities, HookSet, Schema};
    use lex_extension::DiagnosticSeverity;

    fn schema_with_diagnostics(label: &str, codes: &[&str]) -> Schema {
        Schema {
            schema_version: 1,
            label: label.into(),
            description: None,
            params: Default::default(),
            attaches_to: vec!["annotation".into()],
            body: BodyShape::default(),
            verbatim_label: false,
            capabilities: Capabilities::default(),
            hooks: HookSet::default(),
            handler: None,
            diagnostics: codes
                .iter()
                .map(|c| DiagnosticDecl {
                    code: (*c).into(),
                    description: None,
                    default_severity: DiagnosticSeverity::Warning,
                })
                .collect(),
        }
    }

    fn registry_with_acme(codes: &[&str]) -> Registry {
        let r = Registry::new();
        r.register_namespace(
            "acme",
            vec![schema_with_diagnostics("acme.task", codes)],
            Box::new(NoOp),
        )
        .unwrap();
        r
    }

    struct NoOp;
    impl lex_extension::LexHandler for NoOp {}

    fn rules(keys: &[&str]) -> BTreeMap<String, RuleConfig> {
        keys.iter()
            .map(|k| ((*k).into(), RuleConfig::Bare(Severity::Warn)))
            .collect()
    }

    #[test]
    fn declared_code_passes() {
        let reg = registry_with_acme(&["task-due-date-missing"]);
        let found =
            validate_extension_diagnostic_rules(&rules(&["acme.task-due-date-missing"]), &reg);
        assert!(found.is_empty());
    }

    #[test]
    fn unregistered_namespace_passes() {
        // `other` was never registered — staged ahead of install,
        // pass-through per bounded-extensibility.
        let reg = registry_with_acme(&["task-due-date-missing"]);
        let found = validate_extension_diagnostic_rules(&rules(&["other.whatever"]), &reg);
        assert!(found.is_empty());
    }

    #[test]
    fn undeclared_code_is_a_finding_with_suggestion() {
        let reg = registry_with_acme(&["task-due-date-missing"]);
        // Transposed `tsak`.
        let found =
            validate_extension_diagnostic_rules(&rules(&["acme.tsak-due-date-missing"]), &reg);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "acme.tsak-due-date-missing");
        assert!(
            found[0]
                .message
                .contains("did you mean `acme.task-due-date-missing`?"),
            "missing suggestion: {}",
            found[0].message
        );
        assert!(found[0]
            .message
            .contains("declared codes: task-due-date-missing"));
    }

    #[test]
    fn undeclared_code_far_from_any_declared_has_no_suggestion() {
        let reg = registry_with_acme(&["task-due-date-missing"]);
        let found =
            validate_extension_diagnostic_rules(&rules(&["acme.completely-different"]), &reg);
        assert_eq!(found.len(), 1);
        assert!(!found[0].message.contains("did you mean"));
        assert!(found[0]
            .message
            .contains("declared codes: task-due-date-missing"));
    }

    #[test]
    fn registered_namespace_with_no_declared_codes_reports_so() {
        let reg = registry_with_acme(&[]);
        let found = validate_extension_diagnostic_rules(&rules(&["acme.anything"]), &reg);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].message.contains("declares no diagnostic codes"),
            "{}",
            found[0].message
        );
        assert!(!found[0].message.contains("did you mean"));
    }

    #[test]
    fn multiple_keys_each_classified_independently() {
        let reg = registry_with_acme(&["overdue"]);
        let found = validate_extension_diagnostic_rules(
            &rules(&["acme.overdue", "acme.overdew", "other.x"]),
            &reg,
        );
        // `acme.overdue` ok, `other.x` unregistered → pass; only
        // `acme.overdew` is a dead letter.
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "acme.overdew");
        assert!(found[0].message.contains("did you mean `acme.overdue`?"));
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("tsak", "task"), 2);
        assert_eq!(levenshtein("", "abc"), 3);
    }
}
