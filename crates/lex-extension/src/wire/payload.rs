//! Hook response payloads — diagnostics, render output, hover, completions,
//! code actions.

use serde::{Deserialize, Serialize};

use super::ast::WireNode;
use super::range::Range;

/// A diagnostic returned by `on_validate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedDiagnostic>,
}

/// A diagnostic linked to another location (e.g., the definition the
/// diagnostic is about).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelatedDiagnostic {
    pub message: String,
    pub range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

/// Diagnostic severity.
///
/// Forward compatibility: unknown wire values deserialise as
/// [`DiagnosticSeverity::Info`]. Adding new variants is non-breaking
/// (`#[non_exhaustive]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

impl<'de> Deserialize<'de> for DiagnosticSeverity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "error" => Self::Error,
            "warning" => Self::Warning,
            "info" => Self::Info,
            "hint" => Self::Hint,
            _ => Self::Info,
        })
    }
}

/// The result of `on_render`. Either a target-format string snippet or a
/// wire AST in a tree-shaped target's vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenderOut {
    /// String-shaped target (HTML, LaTeX, Markdown).
    String { string: String },
    /// Tree-shaped target (intermediate AST, namespace-defined format).
    WireAst { ast: WireNode },
}

/// Hover content returned by `on_hover`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hover {
    pub contents: String,
    pub format: HoverFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

/// Hover content format.
///
/// Forward compatibility: unknown wire values deserialise as
/// [`HoverFormat::Plaintext`]. Adding new variants is non-breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HoverFormat {
    Plaintext,
    Markdown,
}

impl<'de> Deserialize<'de> for HoverFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "plaintext" => Self::Plaintext,
            "markdown" => Self::Markdown,
            _ => Self::Plaintext,
        })
    }
}

/// One completion item returned by `on_completion`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    pub insert: String,
    pub kind: CompletionKind,
}

/// Completion item kind.
///
/// Forward compatibility: unknown wire values deserialise as
/// [`CompletionKind::Value`]. Adding new variants is non-breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CompletionKind {
    Value,
    Param,
    Namespace,
    Snippet,
}

impl<'de> Deserialize<'de> for CompletionKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "value" => Self::Value,
            "param" => Self::Param,
            "namespace" => Self::Namespace,
            "snippet" => Self::Snippet,
            _ => Self::Value,
        })
    }
}

/// One code action returned by `on_code_action`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeAction {
    pub title: String,
    pub kind: CodeActionKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<TextEdit>,
}

/// Code-action kind.
///
/// Forward compatibility: unknown wire values deserialise as
/// [`CodeActionKind::Refactor`]. Adding new variants is non-breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CodeActionKind {
    Quickfix,
    Refactor,
    Source,
}

impl<'de> Deserialize<'de> for CodeActionKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "quickfix" => Self::Quickfix,
            "refactor" => Self::Refactor,
            "source" => Self::Source,
            _ => Self::Refactor,
        })
    }
}

/// A textual edit applied as part of a code action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::range::Position;

    fn r(s_l: u32, s_c: u32, e_l: u32, e_c: u32) -> Range {
        Range::new(Position::new(s_l, s_c), Position::new(e_l, e_c))
    }

    #[test]
    fn diagnostic_round_trips() {
        let d = Diagnostic {
            severity: DiagnosticSeverity::Error,
            message: "oops".into(),
            range: r(0, 0, 0, 5),
            code: Some("E001".into()),
            related: vec![],
        };
        let s = serde_json::to_string(&d).unwrap();
        let back: Diagnostic = serde_json::from_str(&s).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn render_out_string_round_trips() {
        let r0 = RenderOut::String {
            string: "<p>hi</p>".into(),
        };
        let s = serde_json::to_string(&r0).unwrap();
        assert!(s.contains(r#""kind":"string""#));
        let back: RenderOut = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r0);
    }

    #[test]
    fn render_out_wire_ast_round_trips() {
        let r0 = RenderOut::WireAst {
            ast: WireNode::Paragraph {
                range: r(0, 0, 0, 5),
                origin: None,
                inlines: vec![],
            },
        };
        let s = serde_json::to_string(&r0).unwrap();
        assert!(s.contains(r#""kind":"wire_ast""#));
        let back: RenderOut = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r0);
    }

    #[test]
    fn hover_round_trips() {
        let h = Hover {
            contents: "**bold**".into(),
            format: HoverFormat::Markdown,
            range: Some(r(0, 0, 0, 5)),
        };
        let s = serde_json::to_string(&h).unwrap();
        let back: Hover = serde_json::from_str(&s).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn completion_round_trips() {
        let c = Completion {
            label: "foo".into(),
            detail: Some("Foo the bar".into()),
            doc: None,
            insert: "foo".into(),
            kind: CompletionKind::Param,
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: Completion = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn code_action_round_trips() {
        let a = CodeAction {
            title: "Add missing footnote".into(),
            kind: CodeActionKind::Quickfix,
            edits: vec![TextEdit {
                range: r(10, 0, 10, 0),
                new_text: "[^1]: ...\n".into(),
                uri: None,
            }],
        };
        let s = serde_json::to_string(&a).unwrap();
        let back: CodeAction = serde_json::from_str(&s).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn unknown_severity_falls_back_to_info() {
        let s: DiagnosticSeverity = serde_json::from_str(r#""trace""#).unwrap();
        assert_eq!(s, DiagnosticSeverity::Info);
    }

    #[test]
    fn unknown_completion_kind_falls_back_to_value() {
        let k: CompletionKind = serde_json::from_str(r#""template""#).unwrap();
        assert_eq!(k, CompletionKind::Value);
    }

    #[test]
    fn unknown_code_action_kind_falls_back_to_refactor() {
        let k: CodeActionKind = serde_json::from_str(r#""rename""#).unwrap();
        assert_eq!(k, CodeActionKind::Refactor);
    }

    #[test]
    fn unknown_hover_format_falls_back_to_plaintext() {
        let f: HoverFormat = serde_json::from_str(r#""asciidoc""#).unwrap();
        assert_eq!(f, HoverFormat::Plaintext);
    }
}
