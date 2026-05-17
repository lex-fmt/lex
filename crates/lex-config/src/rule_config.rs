//! User-facing severity for a diagnostic rule.
//!
//! [`RuleConfig`] is the value type for entries in `[diagnostics.rules]` in
//! `.lex.toml`. It accepts two shapes on disk: a bare severity string
//! (`"warn"`) or an array carrying severity plus rule-specific options
//! (`["warn", { max = 100 }]`). The options table is forwarded as-is to
//! the rule's emission code; no type-checking happens here.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// User-facing severity for a diagnostic rule.
///
/// Semantics (applied by the registry once that surface lands; this type
/// currently only carries the configuration value):
///
/// - [`Severity::Allow`] is intended to suppress emission entirely.
/// - [`Severity::Warn`] is intended to emit at the diagnostic's intrinsic
///   LSP severity.
/// - [`Severity::Deny`] is intended to emit at LSP `Error` severity
///   regardless of the intrinsic value, and to be the level CI tooling
///   treats as fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Allow,
    Warn,
    Deny,
}

impl Default for Severity {
    /// Tests and ad-hoc construction of [`RuleConfig`] default to
    /// [`Severity::Warn`]. The *real* per-rule intrinsic defaults are
    /// declared as `#[config(default = "...")]` on each
    /// [`crate::DiagnosticsRulesConfig`] field and applied by clapfig
    /// during config load.
    fn default() -> Self {
        Severity::Warn
    }
}

impl Default for RuleConfig {
    fn default() -> Self {
        RuleConfig::Bare(Severity::default())
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Allow => "allow",
            Severity::Warn => "warn",
            Severity::Deny => "deny",
        })
    }
}

/// Free-form options table forwarded to a rule's emission code.
///
/// Values are kept as raw `toml::Value` so each rule can deserialize the
/// keys it cares about without the registry having to know the shape.
pub type RuleOptions = BTreeMap<String, toml::Value>;

/// One entry in a `[diagnostics.rules]` block.
///
/// Two on-disk shapes parse into the same logical record:
///
/// - `"missing-footnote" = "warn"` — bare severity, no options.
/// - `"line-too-long" = ["warn", { max = 100 }]` — severity + options.
///
/// The single-line form is the common case; the array form exists so
/// rules with numeric thresholds (line length, nesting depth) can plug
/// in without changing the schema. No rule in lex today carries
/// options.
///
/// `Eq` is not derived because [`RuleOptions`] embeds `toml::Value`,
/// which contains `Float` and therefore is not `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RuleConfig {
    /// Bare severity, no options.
    Bare(Severity),
    /// Severity plus a free-form options table.
    WithOptions(Severity, RuleOptions),
}

impl RuleConfig {
    /// The configured severity, regardless of which form was used.
    pub fn severity(&self) -> Severity {
        match self {
            RuleConfig::Bare(s) | RuleConfig::WithOptions(s, _) => *s,
        }
    }

    /// Rule-specific options, or `None` if the bare form was used.
    pub fn options(&self) -> Option<&RuleOptions> {
        match self {
            RuleConfig::Bare(_) => None,
            RuleConfig::WithOptions(_, opts) => Some(opts),
        }
    }
}

impl From<Severity> for RuleConfig {
    fn from(s: Severity) -> Self {
        RuleConfig::Bare(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, Serialize)]
    struct Wrap {
        rule: RuleConfig,
    }

    fn parse(toml: &str) -> RuleConfig {
        toml::from_str::<Wrap>(toml).expect("parse").rule
    }

    #[test]
    fn bare_string_warn() {
        let r = parse(r#"rule = "warn""#);
        assert_eq!(r.severity(), Severity::Warn);
        assert!(r.options().is_none());
    }

    #[test]
    fn bare_string_allow() {
        let r = parse(r#"rule = "allow""#);
        assert_eq!(r.severity(), Severity::Allow);
        assert!(r.options().is_none());
    }

    #[test]
    fn bare_string_deny() {
        let r = parse(r#"rule = "deny""#);
        assert_eq!(r.severity(), Severity::Deny);
        assert!(r.options().is_none());
    }

    #[test]
    fn array_form_with_options() {
        let r = parse(r#"rule = ["warn", { max = 100 }]"#);
        assert_eq!(r.severity(), Severity::Warn);
        let opts = r.options().expect("options present");
        assert_eq!(opts.get("max"), Some(&toml::Value::Integer(100)));
    }

    #[test]
    fn array_form_multiple_options() {
        let r = parse(r#"rule = ["deny", { max = 80, indent = "tabs" }]"#);
        assert_eq!(r.severity(), Severity::Deny);
        let opts = r.options().unwrap();
        assert_eq!(opts.get("max"), Some(&toml::Value::Integer(80)));
        assert_eq!(
            opts.get("indent"),
            Some(&toml::Value::String("tabs".into()))
        );
    }

    #[test]
    fn array_form_empty_options() {
        let r = parse(r#"rule = ["warn", {}]"#);
        assert_eq!(r.severity(), Severity::Warn);
        assert!(r.options().unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_severity_string() {
        // Behaviour we own: an unrecognised severity must not deserialize.
        // Exact wording is owned by serde/toml and not asserted.
        assert!(toml::from_str::<Wrap>(r#"rule = "error""#).is_err());
    }

    #[test]
    fn rejects_invalid_array_severity() {
        assert!(toml::from_str::<Wrap>(r#"rule = ["error", {}]"#).is_err());
    }

    #[test]
    fn round_trip_bare() {
        let r = parse(r#"rule = "warn""#);
        let s = toml::to_string(&Wrap { rule: r.clone() }).unwrap();
        let back = toml::from_str::<Wrap>(&s).unwrap().rule;
        assert_eq!(back, r);
    }

    #[test]
    fn round_trip_with_options() {
        // The array form is part of the on-disk contract — round-trip
        // it through serialization to catch regressions in the
        // tuple-variant emit shape.
        let r = parse(r#"rule = ["warn", { max = 100, indent = "tabs" }]"#);
        let s = toml::to_string(&Wrap { rule: r.clone() }).unwrap();
        let back = toml::from_str::<Wrap>(&s).unwrap().rule;
        assert_eq!(back, r);
        assert_eq!(back.severity(), Severity::Warn);
        let opts = back.options().expect("options preserved");
        assert_eq!(opts.get("max"), Some(&toml::Value::Integer(100)));
        assert_eq!(
            opts.get("indent"),
            Some(&toml::Value::String("tabs".into()))
        );
    }

    #[test]
    fn severity_display() {
        assert_eq!(Severity::Allow.to_string(), "allow");
        assert_eq!(Severity::Warn.to_string(), "warn");
        assert_eq!(Severity::Deny.to_string(), "deny");
    }

    #[test]
    fn severity_into_rule_config() {
        let r: RuleConfig = Severity::Warn.into();
        assert_eq!(r.severity(), Severity::Warn);
        assert!(r.options().is_none());
    }
}
