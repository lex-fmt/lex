use lex_analysis::utils::collect_footnote_definitions;
use lex_core::lex::ast::Document;
use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionParams, Position, Range, TextEdit, WorkspaceEdit,
};

pub fn compute_actions(
    document: &Document,
    source: &str,
    params: &CodeActionParams,
) -> Vec<CodeAction> {
    let mut actions = Vec::new();

    // 1. Diagnostic-based actions.
    //
    // Group missing-footnote diagnostics by label: applying the quickfix
    // resolves *all* references to that label, so the resulting CodeAction
    // attaches every matching diagnostic. Preserve first-encountered order
    // so the quickfix list is stable across runs.
    let mut label_order: Vec<String> = Vec::new();
    let mut diagnostics_by_label: HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>> =
        HashMap::new();
    for diagnostic in &params.context.diagnostics {
        let Some(tower_lsp::lsp_types::NumberOrString::String(code)) = &diagnostic.code else {
            continue;
        };
        if code.as_str() != "missing-footnote" {
            continue;
        }
        // The diagnostic range points at the reference (e.g. `[1]`). Read the
        // source at that range and strip brackets to get the label.
        let Some(label) = label_from_diagnostic_range(source, &diagnostic.range) else {
            continue;
        };
        if !diagnostics_by_label.contains_key(&label) {
            label_order.push(label.clone());
        }
        diagnostics_by_label
            .entry(label)
            .or_default()
            .push(diagnostic.clone());
    }

    for label in &label_order {
        let matching = diagnostics_by_label
            .remove(label)
            .expect("label registered in diagnostics_by_label");
        let edit = build_missing_footnote_edit(document, source, label);
        actions.push(CodeAction {
            title: format!("Add definition for footnote [{label}]"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(matching),
            edit: Some(WorkspaceEdit {
                changes: Some(HashMap::from([(
                    params.text_document.uri.clone(),
                    vec![edit],
                )])),
                ..Default::default()
            }),
            command: None,
            is_preferred: Some(true),
            disabled: None,
            data: None,
        });
    }

    // 2. Global actions (Refactor)
    let requested_kind = params.context.only.as_ref().and_then(|k| k.first());
    let wants_refactor = requested_kind
        .is_none_or(|k| k.as_str().starts_with("source") || k.as_str().starts_with("refactor"));

    if wants_refactor {
        // Compute reordered content
        let new_content = crate::features::footnotes::reorder_footnotes(document, source);

        if new_content != source {
            let line_count = source.lines().count().max(1) as u32;
            let last_line_idx = line_count - 1;
            let last_char = source
                .lines()
                .last()
                .map(|l| l.chars().count())
                .unwrap_or(0) as u32;

            let end_pos = Position {
                line: last_line_idx,
                character: last_char,
            };

            actions.push(CodeAction {
                title: "Reorder footnotes".to_string(),
                kind: Some(CodeActionKind::SOURCE),
                diagnostics: None,
                edit: Some(WorkspaceEdit {
                    changes: Some(HashMap::from([(
                        params.text_document.uri.clone(),
                        vec![TextEdit {
                            range: Range {
                                start: Position {
                                    line: 0,
                                    character: 0,
                                },
                                end: end_pos,
                            },
                            new_text: new_content,
                        }],
                    )])),
                    ..Default::default()
                }),
                command: None,
                is_preferred: None,
                disabled: None,
                data: None,
            });
        }
    }

    actions
}

/// Reads the source text spanned by the diagnostic range and extracts the
/// footnote label. The range typically points at `[1]`; we strip the brackets
/// and return the inside. Returns None if the range is degenerate, crosses
/// lines (a footnote reference never does), or the byte offsets land on
/// non-UTF-8 boundaries.
///
/// `Position.character` is a byte offset in this codebase (see
/// `SourceLocation::byte_to_position` in lex-core), not a UTF-16 code unit
/// count, so we slice the line by bytes.
fn label_from_diagnostic_range(source: &str, range: &Range) -> Option<String> {
    if range.start.line != range.end.line {
        return None;
    }
    let line = source.lines().nth(range.start.line as usize)?;
    let start_byte = range.start.character as usize;
    let end_byte = range.end.character as usize;
    let slice = line.get(start_byte..end_byte)?;
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
        let inside = &trimmed[1..trimmed.len() - 1];
        if inside.is_empty() {
            return None;
        }
        return Some(inside.to_string());
    }
    Some(trimmed.to_string())
}

/// Decides where to insert a new footnote definition and produces the edit.
///
/// Strategy:
/// - If the document already has a `:: notes ::`-scoped list with definitions
///   anywhere (root or session), append a new list item after the textually
///   last one, preserving its indent.
/// - Otherwise, append a fresh `:: notes ::` block at the end of the document.
///
/// The placeholder content after `{label}.` is intentionally empty — the edit
/// leaves the cursor at a natural point for the user to type the definition.
fn build_missing_footnote_edit(document: &Document, source: &str, label: &str) -> TextEdit {
    let defs = collect_footnote_definitions(document);

    // Pick the textually-last definition by *end* position. List-item ranges
    // span the full item (marker line + any nested content), so `end` is the
    // right anchor: selecting by start.line can place the new item in the
    // middle of a multi-line definition.
    let last_def_range = defs
        .iter()
        .max_by_key(|(_, r)| (r.end.line, r.end.column))
        .map(|(_, r)| r);

    if let Some(r) = last_def_range {
        // Indent comes from the marker line (start). Preserve it verbatim so
        // session-scoped notes (e.g. 4-space-indented items) keep their shape.
        let marker_line = source.lines().nth(r.start.line).unwrap_or("");
        let indent: String = marker_line
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();

        // Insert at the end of the item. If end.column == 0 the anchor is at
        // the start of the next line; the new item slots in verbatim without a
        // leading newline. Otherwise prepend one.
        let insert_pos = Position {
            line: r.end.line as u32,
            character: r.end.column as u32,
        };
        let prefix = if r.end.column == 0 { "" } else { "\n" };
        return TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: format!("{prefix}{indent}{label}. "),
        };
    }

    // No existing notes block — append a new one at EOF.
    let eof = end_of_document_position(source);
    let separator = separator_before_new_block(source);
    TextEdit {
        range: Range {
            start: eof,
            end: eof,
        },
        new_text: format!("{separator}:: notes ::\n\n{label}. "),
    }
}

/// End-of-document position as an LSP Position. If the document ends with a
/// newline, points to `(line_count, 0)` — i.e. the (empty) line after the
/// last content. Otherwise points at the end of the final line.
///
/// `Position.character` is a byte offset in this codebase, so use `line.len()`
/// (bytes) rather than `chars().count()` for multi-byte safety.
fn end_of_document_position(source: &str) -> Position {
    if source.is_empty() {
        return Position {
            line: 0,
            character: 0,
        };
    }
    if source.ends_with('\n') {
        let line_count = source.lines().count() as u32;
        return Position {
            line: line_count,
            character: 0,
        };
    }
    // No trailing newline — sit at the end of the last line.
    let line_count = source.lines().count() as u32;
    let last_line_len = source.lines().last().map(|l| l.len()).unwrap_or(0) as u32;
    Position {
        line: line_count.saturating_sub(1),
        character: last_line_len,
    }
}

/// Whitespace to prepend before a new `:: notes ::` block so it is separated
/// from the existing content by a blank line. The rules mirror how a human
/// would type it: nothing if the file is empty, nothing extra if it already
/// ends with a blank line, one `\n` if it ends with a newline, two otherwise.
fn separator_before_new_block(source: &str) -> &'static str {
    if source.is_empty() || source.ends_with("\n\n") {
        ""
    } else if source.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::parsing::parse_document;
    use lex_core::lex::testing::lexplore::Lexplore;
    use tower_lsp::lsp_types::{
        CodeActionContext, Diagnostic, DiagnosticSeverity, NumberOrString, PartialResultParams,
        TextDocumentIdentifier, Url, WorkDoneProgressParams,
    };

    fn parse(source: &str) -> Document {
        parse_document(source).expect("parse fixture")
    }

    /// Loads a footnote spec fixture (source + parsed Document).
    fn footnote_fixture(n: usize) -> (String, Document) {
        let loader = Lexplore::footnotes(n);
        let source = loader.source();
        let doc = loader.parse().expect("parse spec fixture");
        (source, doc)
    }

    fn missing_footnote_diag(line: u32, start: u32, end: u32, message: &str) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: start,
                },
                end: Position {
                    line,
                    character: end,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("missing-footnote".into())),
            code_description: None,
            source: Some("lex".into()),
            message: message.into(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    fn make_params(source: &str, diags: Vec<Diagnostic>) -> CodeActionParams {
        CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::parse("file:///test.lex").unwrap(),
            },
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: source.lines().next().map(|l| l.len()).unwrap_or(0) as u32,
                },
            },
            context: CodeActionContext {
                diagnostics: diags,
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        }
    }

    fn quickfix_edit(action: &CodeAction) -> &TextEdit {
        let edit = action.edit.as_ref().expect("action has no edit");
        let changes = edit.changes.as_ref().expect("edit has no changes");
        let edits = changes.values().next().expect("no file in changes");
        assert_eq!(edits.len(), 1, "expected exactly one TextEdit");
        &edits[0]
    }

    fn apply_edit(source: &str, edit: &TextEdit) -> String {
        let mut line_offsets = vec![0usize];
        for (i, ch) in source.char_indices() {
            if ch == '\n' {
                line_offsets.push(i + 1);
            }
        }
        let to_byte = |pos: Position| -> usize {
            let line_start = *line_offsets
                .get(pos.line as usize)
                .unwrap_or(line_offsets.last().unwrap_or(&source.len()));
            let mut byte = line_start;
            for (chars_seen, ch) in source[line_start..].chars().enumerate() {
                if chars_seen >= pos.character as usize {
                    break;
                }
                if ch == '\n' {
                    break;
                }
                byte += ch.len_utf8();
            }
            byte.min(source.len())
        };
        let start = to_byte(edit.range.start);
        let end = to_byte(edit.range.end);
        let mut out = String::with_capacity(source.len() + edit.new_text.len());
        out.push_str(&source[..start]);
        out.push_str(&edit.new_text);
        out.push_str(&source[end..]);
        out
    }

    // --- label extraction ---

    #[test]
    fn label_extracted_from_bracketed_range() {
        let src = "See [1] here.\n";
        let range = Range {
            start: Position {
                line: 0,
                character: 4,
            },
            end: Position {
                line: 0,
                character: 7,
            },
        };
        assert_eq!(label_from_diagnostic_range(src, &range).unwrap(), "1");
    }

    #[test]
    fn label_extraction_rejects_cross_line_range() {
        let src = "a\nb\n";
        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 1,
                character: 1,
            },
        };
        assert_eq!(label_from_diagnostic_range(src, &range), None);
    }

    #[test]
    fn label_extraction_handles_unbracketed_text() {
        // Defensive path: if the range somehow covers an unbracketed token.
        let src = "word\n";
        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 4,
            },
        };
        assert_eq!(label_from_diagnostic_range(src, &range).unwrap(), "word");
    }

    #[test]
    fn label_extraction_handles_multibyte_prefix() {
        // `Position.character` is a byte offset in this codebase (not UTF-16
        // code units, not chars). If the label extractor slices by chars, a
        // multi-byte char before the reference will make it read the wrong
        // bytes. Exercise a line with a 2-byte UTF-8 character before `[1]`.
        let src = "Café [1] here.\n";
        // "Café " is 6 bytes (C=1, a=1, f=1, é=2, space=1), so `[` is at
        // byte 6. Exercising with byte offsets simulates what the analyzer
        // produces downstream of `SourceLocation::byte_to_position`.
        let bracket_start = src.find("[1]").unwrap() as u32;
        let range = Range {
            start: Position {
                line: 0,
                character: bracket_start,
            },
            end: Position {
                line: 0,
                character: bracket_start + 3,
            },
        };
        assert_eq!(label_from_diagnostic_range(src, &range).unwrap(), "1");
    }

    #[test]
    fn label_extraction_rejects_range_on_non_utf8_boundary() {
        // "Café" byte layout: C=0, a=1, f=2, é=[3,4] (2 bytes), .=5. Byte 4 is
        // the trailing byte of the `é` char — not a valid UTF-8 boundary.
        // `line.get(4..5)` returns None; the extractor should surface that
        // rather than panicking.
        let src = "Café.\n";
        let range = Range {
            start: Position {
                line: 0,
                character: 4,
            },
            end: Position {
                line: 0,
                character: 5,
            },
        };
        assert_eq!(label_from_diagnostic_range(src, &range), None);
    }

    #[test]
    fn label_extraction_rejects_empty_brackets() {
        let src = "[]";
        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 2,
            },
        };
        assert_eq!(label_from_diagnostic_range(src, &range), None);
    }

    // --- quickfix: no existing notes block ---

    #[test]
    fn creates_new_notes_block_when_none_exists() {
        // footnotes-01: `Text with [1] reference.\n` — reference but no definitions.
        let (src, doc) = footnote_fixture(1);
        // Find the [1] occurrence for an accurate range.
        let line0 = src.lines().next().unwrap();
        let bracket_start = line0.find("[1]").expect("fixture should contain [1]") as u32;
        let diag = missing_footnote_diag(
            0,
            bracket_start,
            bracket_start + 3,
            "Footnote [1] has no matching definition",
        );
        let params = make_params(&src, vec![diag]);
        let actions = compute_actions(&doc, &src, &params);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Add definition for footnote [1]");
        let after = apply_edit(&src, quickfix_edit(&actions[0]));
        assert!(
            after.contains(":: notes ::"),
            "expected notes block; got:\n{after}"
        );
        assert!(
            after.contains("\n1. "),
            "expected new list item; got:\n{after}"
        );
        // Note: a `:: notes ::` block with a single item does not re-parse as a
        // footnote definition list — Lex requires 2+ items to form a list (see
        // lex-primer, "List (2+ items)"). The quickfix still produces the right
        // scaffolding; resolving `[1]` requires the user to add a second item
        // or accept the parser's view that a single `1.` is a paragraph. This
        // is a known limitation of the MVP, not a bug in the edit shape.
    }

    #[test]
    fn new_notes_block_separated_from_trailing_content_without_newline() {
        let src = "Text [2] more.\nSecond paragraph.";
        let doc = parse(src);
        let diag = missing_footnote_diag(0, 5, 8, "Footnote [2] has no matching definition");
        let params = make_params(src, vec![diag]);
        let actions = compute_actions(&doc, src, &params);
        let after = apply_edit(src, quickfix_edit(&actions[0]));
        // File did not end with newline → edit should add blank-line separation.
        assert!(
            after.ends_with(":: notes ::\n\n2. "),
            "edit should end with a ready-to-type list item; got end: {:?}",
            &after[after.len().saturating_sub(30)..]
        );
        // The original content should still be present once, unmodified.
        assert!(after.starts_with("Text [2] more.\nSecond paragraph."));
    }

    // --- quickfix: existing notes block ---

    #[test]
    fn appends_to_existing_root_notes_block() {
        // footnotes-02: `:: notes ::` at root with definitions 1 and 2. We
        // simulate a missing-footnote diagnostic for a new label [3] so the
        // quickfix has to append rather than create.
        let (src, doc) = footnote_fixture(2);
        let defs_before = collect_footnote_definitions(&doc);
        assert!(
            !defs_before.is_empty(),
            "fixture must have at least one existing def; got {defs_before:?}"
        );
        // The fixture doesn't have a [3] reference. `label_from_diagnostic_range`
        // reads source at the diagnostic range, so we prepend `[3]\n` and point
        // the diagnostic at it. The quickfix logic itself is unaware of the
        // prepended line — it just sees the label and the existing notes block.
        let patched = format!("[3]\n{src}");
        let doc_patched = parse(&patched);
        let diag = missing_footnote_diag(0, 0, 3, "Footnote [3] has no matching definition");
        let params = make_params(&patched, vec![diag]);
        let actions = compute_actions(&doc_patched, &patched, &params);
        let missing_fixes: Vec<_> = actions
            .iter()
            .filter(|a| a.title.starts_with("Add definition"))
            .collect();
        assert_eq!(missing_fixes.len(), 1);
        assert_eq!(missing_fixes[0].title, "Add definition for footnote [3]");
        let after = apply_edit(&patched, quickfix_edit(missing_fixes[0]));

        let defs_after = collect_footnote_definitions(&parse(&after));
        let labels: Vec<&str> = defs_after.iter().map(|(l, _)| l.as_str()).collect();
        assert!(
            labels.contains(&"3"),
            "appended [3] should be a recognized def; labels={labels:?} after=\n{after}"
        );
        // Existing defs should still be present.
        assert!(labels.contains(&"1"));
        assert!(labels.contains(&"2"));
    }

    #[test]
    fn append_preserves_indent_of_existing_item() {
        // footnotes-05 uses session-scoped `:: notes ::` with 4-space-indented
        // items. Verify the quickfix preserves the indent.
        let (src, doc) = footnote_fixture(5);
        let defs_before = collect_footnote_definitions(&doc);
        assert!(
            !defs_before.is_empty(),
            "fixture 5 must have at least one def; got {defs_before:?}"
        );

        // Construct a synthetic diagnostic for label [9] using a patched source
        // that starts with `[9]` so label extraction succeeds.
        let patched = format!("[9]\n{src}");
        let doc_patched = parse(&patched);
        let diag = missing_footnote_diag(0, 0, 3, "Footnote [9] has no matching definition");
        let params = make_params(&patched, vec![diag]);
        let actions = compute_actions(&doc_patched, &patched, &params);
        let action = actions
            .iter()
            .find(|a| a.title.starts_with("Add definition"))
            .unwrap();
        let after = apply_edit(&patched, quickfix_edit(action));
        // Figure out the indent the last existing def used in the fixture.
        let last_def_line = defs_before
            .iter()
            .map(|(_, r)| r.start.line + 1) // +1 because we prepended one line to the source
            .max()
            .unwrap();
        let last_indent: String = after
            .lines()
            .nth(last_def_line)
            .unwrap_or("")
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        assert!(
            !last_indent.is_empty(),
            "fixture 5 items should be indented; check fixture"
        );
        let expected_new_line = format!("{last_indent}9. ");
        assert!(
            after.contains(&expected_new_line),
            "should preserve indent {last_indent:?}; expected line `{expected_new_line}` in:\n{after}"
        );
    }

    // --- dedup ---

    #[test]
    fn dedupes_multiple_diagnostics_for_same_label() {
        let src = "See [1] and again [1].\n";
        let doc = parse(src);
        let d1 = missing_footnote_diag(0, 4, 7, "Footnote [1] has no matching definition");
        let d2 = missing_footnote_diag(0, 18, 21, "Footnote [1] has no matching definition");
        let params = make_params(src, vec![d1.clone(), d2.clone()]);
        let actions = compute_actions(&doc, src, &params);
        // Expect exactly one missing-footnote quickfix, not two.
        let missing_footnote_actions: Vec<_> = actions
            .iter()
            .filter(|a| a.title.starts_with("Add definition"))
            .collect();
        assert_eq!(missing_footnote_actions.len(), 1);
        // Applying the fix resolves *both* occurrences, so the CodeAction
        // should attach both diagnostics. A client that filters actions by
        // diagnostic identity wouldn't find the fix for the second occurrence
        // if we only attached the first.
        let attached = missing_footnote_actions[0]
            .diagnostics
            .as_ref()
            .expect("action should have diagnostics attached");
        assert_eq!(attached.len(), 2, "both diagnostics should be attached");
        let attached_ranges: Vec<_> = attached.iter().map(|d| d.range).collect();
        assert!(attached_ranges.contains(&d1.range));
        assert!(attached_ranges.contains(&d2.range));
    }

    #[test]
    fn produces_quickfix_per_distinct_label() {
        let src = "See [1] and [2].\n";
        let doc = parse(src);
        let d1 = missing_footnote_diag(0, 4, 7, "Footnote [1] has no matching definition");
        let d2 = missing_footnote_diag(0, 12, 15, "Footnote [2] has no matching definition");
        let params = make_params(src, vec![d1, d2]);
        let actions = compute_actions(&doc, src, &params);
        let missing_footnote_actions: Vec<_> = actions
            .iter()
            .filter(|a| a.title.starts_with("Add definition"))
            .collect();
        assert_eq!(missing_footnote_actions.len(), 2);
        let titles: Vec<&str> = missing_footnote_actions
            .iter()
            .map(|a| a.title.as_str())
            .collect();
        assert!(titles.contains(&"Add definition for footnote [1]"));
        assert!(titles.contains(&"Add definition for footnote [2]"));
    }

    // --- gating ---

    #[test]
    fn no_quickfix_for_non_missing_footnote_code() {
        let src = "Refs [1].\n";
        let doc = parse(src);
        let diag = Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 5,
                },
                end: Position {
                    line: 0,
                    character: 8,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("unused-footnote".into())),
            code_description: None,
            source: Some("lex".into()),
            message: "unused".into(),
            related_information: None,
            tags: None,
            data: None,
        };
        let params = make_params(src, vec![diag]);
        let actions = compute_actions(&doc, src, &params);
        let any_missing = actions
            .iter()
            .any(|a| a.title.starts_with("Add definition"));
        assert!(!any_missing);
    }

    #[test]
    fn quickfix_is_preferred_and_quickfix_kind() {
        let src = "Ref [1].\n";
        let doc = parse(src);
        let diag = missing_footnote_diag(0, 4, 7, "Footnote [1] has no matching definition");
        let params = make_params(src, vec![diag]);
        let actions = compute_actions(&doc, src, &params);
        let action = actions
            .iter()
            .find(|a| a.title.starts_with("Add definition"))
            .unwrap();
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        assert_eq!(action.is_preferred, Some(true));
        // Diagnostic is attached so clients can resolve which diag the fix addresses.
        assert_eq!(action.diagnostics.as_ref().map(|v| v.len()), Some(1));
    }
}
