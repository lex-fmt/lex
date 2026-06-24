//! Tests for the language server (`lex_lsp::server`).
//!
//! Split out of `server.rs` as a sibling module file (declared there as
//! `#[cfg(test)] mod tests;`). All setup routes through a small set of
//! harness types — [`NoopClient`] for the lightweight router/smoke tests,
//! and [`CapturingClient`] / [`open_in_tempdir`] for the include-resolution
//! integration tests that exercise the `FsLoader` end-to-end against a real
//! `TempDir`.

use super::*;
use crate::features::semantic_tokens::{LexSemanticToken, LexSemanticTokenKind};
use lex_analysis::test_support::sample_source;
use lex_core::lex::ast::{Position as AstPosition, Range as AstRange};
use serde::Deserialize;
use std::fs;
use std::sync::Mutex;
use tempfile::tempdir;
use tower_lsp::lsp_types::{
    DidOpenTextDocumentParams, DocumentSymbolParams, FormattingOptions, FormattingProperty,
    GotoDefinitionParams, HoverParams, Position, Range, SemanticTokensParams,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams,
};
use tower_lsp::LanguageServer;

#[derive(Clone, Default)]
struct NoopClient;
#[async_trait]
impl LspClient for NoopClient {
    async fn publish_diagnostics(&self, _: Url, _: Vec<Diagnostic>, _: Option<i32>) {}
    async fn show_message(&self, _: MessageType, _: String) {}
}
#[async_trait]
impl crate::trust_prompt::LspTrustRequester for NoopClient {
    async fn send_trust_request(
        &self,
        _: crate::trust_prompt::TrustRequestParams,
    ) -> tower_lsp::jsonrpc::Result<crate::trust_prompt::TrustResponse> {
        // Tests don't exercise the trust prompt path; deny so a
        // boot path that does reach the prompt has predictable
        // behavior.
        Ok(crate::trust_prompt::TrustResponse {
            decision: "denied".into(),
            reason: Some("test client".into()),
        })
    }
}

fn sample_uri() -> Url {
    Url::parse("file:///sample.lex").unwrap()
}

fn sample_text() -> String {
    sample_source().to_string()
}

fn offset_to_position(source: &str, offset: usize) -> AstPosition {
    let mut line = 0;
    let mut line_start = 0;
    for (idx, ch) in source.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = idx + ch.len_utf8();
        }
    }
    AstPosition::new(line, offset - line_start)
}

fn range_for_snippet(snippet: &str) -> AstRange {
    let source = sample_source();
    let start = source
        .find(snippet)
        .unwrap_or_else(|| panic!("snippet not found: {snippet}"));
    let end = start + snippet.len();
    let start_pos = offset_to_position(source, start);
    let end_pos = offset_to_position(source, end);
    AstRange::new(start..end, start_pos, end_pos)
}

async fn open_sample_document(server: &LexLanguageServer<NoopClient>) {
    let uri = sample_uri();
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "lex".into(),
                version: 1,
                text: sample_text(),
            },
        })
        .await;
}

// The router methods are thin: look up the document, call the feature
// free-function (densely tested in `lex-analysis` / `lex-lsp-core`), convert,
// return. These smoke tests exercise the wiring + conversion over a real parse
// of the sample document; per-feature behavior lives in the feature crates.

#[tokio::test]
async fn semantic_tokens_full_over_sample_returns_tokens() {
    let server = LexLanguageServer::new(NoopClient);
    open_sample_document(&server).await;

    let result = server
        .semantic_tokens_full(SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: sample_uri() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();

    let data_len = match result {
        SemanticTokensResult::Tokens(tokens) => tokens.data.len(),
        SemanticTokensResult::Partial(partial) => partial.data.len(),
    };
    assert!(data_len > 0, "sample document should yield semantic tokens");
}

#[tokio::test]
async fn document_symbol_over_sample_returns_outline() {
    let server = LexLanguageServer::new(NoopClient);
    open_sample_document(&server).await;

    let response = server
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: sample_uri() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();

    match response {
        DocumentSymbolResponse::Nested(symbols) => assert!(!symbols.is_empty()),
        _ => panic!("unexpected symbol response"),
    }
}

#[tokio::test]
async fn execute_command_unknown_is_invalid_request() {
    let server = LexLanguageServer::new(NoopClient);

    // Commands not matched by the router fall through to
    // `features::commands::execute_command`, whose default arm rejects
    // unknown commands with `invalid_request`.
    let err = server
        .execute_command(ExecuteCommandParams {
            command: "does.not.exist".into(),
            arguments: vec![],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap_err();

    assert_eq!(err.code, tower_lsp::jsonrpc::ErrorCode::InvalidRequest);
}

#[test]
fn encode_semantic_tokens_splits_multi_line_ranges() {
    let snippet = "    CLI Example:\n        lex build\n        lex serve";
    let range = range_for_snippet(snippet);
    let tokens = vec![LexSemanticToken {
        kind: LexSemanticTokenKind::DocumentTitle,
        range,
    }];
    let source = sample_source();
    let encoded = encode_semantic_tokens(&tokens, source);
    assert_eq!(encoded.len(), 3);
    let snippet_offset = source
        .find(snippet)
        .expect("snippet not found in sample document");
    let mut cursor = 0;
    let lines: Vec<&str> = snippet.split('\n').collect();
    let mut expected_positions = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let offset = snippet_offset + cursor;
        expected_positions.push(offset_to_position(source, offset));
        cursor += line.len();
        if idx < lines.len() - 1 {
            cursor += 1; // account for newline
        }
    }
    let mut absolute_positions = Vec::new();
    let mut line = 0u32;
    let mut column = 0u32;
    for token in &encoded {
        line += token.delta_line;
        let start = if token.delta_line == 0 {
            column + token.delta_start
        } else {
            token.delta_start
        };
        column = start;
        absolute_positions.push((line, start));
    }
    for (actual, expected) in absolute_positions.iter().zip(expected_positions.iter()) {
        assert_eq!(actual.0, expected.line as u32);
        assert_eq!(actual.1, expected.column as u32);
    }
    let expected_len: usize = snippet.lines().map(|line| line.len()).sum();
    let actual_len: usize = encoded.iter().map(|token| token.length as usize).sum();
    assert_eq!(actual_len, expected_len);
}

#[derive(Deserialize)]
struct SnippetResponse {
    text: String,
    #[serde(rename = "cursorOffset")]
    cursor_offset: usize,
}

#[tokio::test]
async fn execute_insert_commands() {
    let server = LexLanguageServer::new(NoopClient);
    open_sample_document(&server).await;

    let temp_dir = tempdir().unwrap();
    let asset_file = temp_dir.path().join("diagram.png");
    fs::write(&asset_file, [0u8, 159u8, 146u8, 150u8]).unwrap();

    let params = ExecuteCommandParams {
        command: commands::COMMAND_INSERT_ASSET.to_string(),
        arguments: vec![
            serde_json::to_value(sample_uri().to_string()).unwrap(),
            serde_json::to_value(Position::new(0, 0)).unwrap(),
            serde_json::to_value(asset_file.to_string_lossy()).unwrap(),
        ],
        work_done_progress_params: Default::default(),
    };
    let result = server.execute_command(params).await.unwrap();
    let snippet: SnippetResponse = serde_json::from_value(result.unwrap()).unwrap();
    assert!(snippet.text.contains(":: image"));
    assert!(snippet.text.contains(asset_file.to_string_lossy().as_ref()));

    let verbatim_file = temp_dir.path().join("example.py");
    fs::write(&verbatim_file, "print('hi')\n").unwrap();

    let params = ExecuteCommandParams {
        command: commands::COMMAND_INSERT_VERBATIM.to_string(),
        arguments: vec![
            serde_json::to_value(sample_uri().to_string()).unwrap(),
            serde_json::to_value(Position::new(0, 0)).unwrap(),
            serde_json::to_value(verbatim_file.to_string_lossy()).unwrap(),
        ],
        work_done_progress_params: Default::default(),
    };
    let result = server.execute_command(params).await.unwrap();
    let snippet: SnippetResponse = serde_json::from_value(result.unwrap()).unwrap();
    assert!(snippet.text.contains(":: python"));
    assert!(snippet.text.contains("print('hi')"));
    assert_eq!(snippet.cursor_offset, 0);
}

#[tokio::test]
async fn execute_annotation_navigation_commands() {
    let server = LexLanguageServer::new(NoopClient);
    let uri = Url::parse("file:///annotations.lex").unwrap();
    let text = ":: note ::\n    First\n::\n\n:: note ::\n    Second\n::\n";
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "lex".into(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    let next_params = ExecuteCommandParams {
        command: commands::COMMAND_NEXT_ANNOTATION.to_string(),
        arguments: vec![
            serde_json::to_value(uri.to_string()).unwrap(),
            serde_json::to_value(Position::new(0, 0)).unwrap(),
        ],
        work_done_progress_params: Default::default(),
    };
    let next_location: Location =
        serde_json::from_value(server.execute_command(next_params).await.unwrap().unwrap())
            .unwrap();
    assert_eq!(next_location.range.start.line, 0);

    let previous_params = ExecuteCommandParams {
        command: commands::COMMAND_PREVIOUS_ANNOTATION.to_string(),
        arguments: vec![
            serde_json::to_value(uri.to_string()).unwrap(),
            serde_json::to_value(Position::new(0, 0)).unwrap(),
        ],
        work_done_progress_params: Default::default(),
    };
    let previous_location: Location = serde_json::from_value(
        server
            .execute_command(previous_params)
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(previous_location.range.start.line, 4);

    let resolve_params = ExecuteCommandParams {
        command: commands::COMMAND_RESOLVE_ANNOTATION.to_string(),
        arguments: vec![
            serde_json::to_value(uri.to_string()).unwrap(),
            serde_json::to_value(Position::new(0, 0)).unwrap(),
        ],
        work_done_progress_params: Default::default(),
    };
    let edit_value = server
        .execute_command(resolve_params)
        .await
        .unwrap()
        .unwrap();
    let workspace_edit: tower_lsp::lsp_types::WorkspaceEdit =
        serde_json::from_value(edit_value).unwrap();
    let changes = workspace_edit.changes.expect("workspace edit changes");
    let edits = changes.get(&uri).expect("edits for document");
    assert_eq!(edits[0].new_text, ":: note status=resolved ::");
}

#[tokio::test]
async fn semantic_tokens_returns_none_when_document_missing() {
    let server = LexLanguageServer::new(NoopClient);

    let result = server
        .semantic_tokens_full(SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: sample_uri() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn hover_returns_none_without_document_entry() {
    let server = LexLanguageServer::new(NoopClient);

    let hover = server
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: sample_uri() },
                position: Position::new(0, 0),
            },
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap();

    assert!(hover.is_none());
}

#[test]
fn apply_formatting_overrides_noop_without_lex_properties() {
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        properties: Default::default(),
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let mut rules = FormattingRules::default();
    let original = rules.clone();
    apply_formatting_overrides(&mut rules, &options);
    assert_eq!(rules.indent_string, original.indent_string);
    assert_eq!(rules.max_blank_lines, original.max_blank_lines);
}

#[test]
fn apply_formatting_overrides_applies_lex_properties() {
    use std::collections::HashMap;

    let mut properties = HashMap::new();
    properties.insert(
        "lex.indent_string".to_string(),
        FormattingProperty::String("  ".to_string()),
    );
    properties.insert(
        "lex.max_blank_lines".to_string(),
        FormattingProperty::Number(3),
    );
    properties.insert(
        "lex.normalize_seq_markers".to_string(),
        FormattingProperty::Bool(false),
    );
    properties.insert(
        "lex.unordered_seq_marker".to_string(),
        FormattingProperty::String("*".to_string()),
    );

    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        properties,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };

    let mut rules = FormattingRules::default();
    apply_formatting_overrides(&mut rules, &options);
    assert_eq!(rules.indent_string, "  ");
    assert_eq!(rules.max_blank_lines, 3);
    assert!(!rules.normalize_seq_markers);
    assert_eq!(rules.unordered_seq_marker, '*');
}

#[tokio::test]
async fn did_change_workspace_folders_adds_roots() {
    let server = LexLanguageServer::new(NoopClient);

    // Start with one root via initialize
    server
        .initialize(InitializeParams {
            root_uri: Some(Url::from_file_path("/initial").unwrap()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(server.workspace_roots.read().await.len(), 1);

    // Add a workspace folder
    server
        .did_change_workspace_folders(DidChangeWorkspaceFoldersParams {
            event: lsp_types::WorkspaceFoldersChangeEvent {
                added: vec![lsp_types::WorkspaceFolder {
                    uri: Url::from_file_path("/added").unwrap(),
                    name: "added".to_string(),
                }],
                removed: vec![],
            },
        })
        .await;

    let roots = server.workspace_roots.read().await;
    assert_eq!(roots.len(), 2);
    assert_eq!(roots[1], PathBuf::from("/added"));
}

#[tokio::test]
async fn did_change_workspace_folders_removes_roots() {
    let server = LexLanguageServer::new(NoopClient);

    server
        .initialize(InitializeParams {
            root_uri: Some(Url::from_file_path("/initial").unwrap()),
            ..Default::default()
        })
        .await
        .unwrap();

    // Add a folder then remove the initial one
    server
        .did_change_workspace_folders(DidChangeWorkspaceFoldersParams {
            event: lsp_types::WorkspaceFoldersChangeEvent {
                added: vec![lsp_types::WorkspaceFolder {
                    uri: Url::from_file_path("/new-root").unwrap(),
                    name: "new-root".to_string(),
                }],
                removed: vec![lsp_types::WorkspaceFolder {
                    uri: Url::from_file_path("/initial").unwrap(),
                    name: "initial".to_string(),
                }],
            },
        })
        .await;

    let roots = server.workspace_roots.read().await;
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0], PathBuf::from("/new-root"));
}

#[tokio::test]
async fn did_change_workspace_folders_does_not_duplicate() {
    let server = LexLanguageServer::new(NoopClient);

    server
        .initialize(InitializeParams {
            root_uri: Some(Url::from_file_path("/root").unwrap()),
            ..Default::default()
        })
        .await
        .unwrap();

    // Try to add the same folder that already exists
    server
        .did_change_workspace_folders(DidChangeWorkspaceFoldersParams {
            event: lsp_types::WorkspaceFoldersChangeEvent {
                added: vec![lsp_types::WorkspaceFolder {
                    uri: Url::from_file_path("/root").unwrap(),
                    name: "root".to_string(),
                }],
                removed: vec![],
            },
        })
        .await;

    assert_eq!(server.workspace_roots.read().await.len(), 1);
}

#[tokio::test]
async fn initialize_advertises_workspace_folder_support() {
    let server = LexLanguageServer::new(NoopClient);

    let result = server
        .initialize(InitializeParams::default())
        .await
        .unwrap();

    let workspace = result
        .capabilities
        .workspace
        .expect("workspace capabilities");
    let folders = workspace
        .workspace_folders
        .expect("workspace folder support");
    assert_eq!(folders.supported, Some(true));
    assert_eq!(folders.change_notifications, Some(OneOf::Left(true)));
}

// ========================================================================
// Include resolution integration (PR 8)
// ========================================================================
//
// These tests use a CapturingClient that records every
// publish_diagnostics call so assertions can inspect the diagnostic
// payload directly. Test sources are written to a TempDir so the
// FsLoader is exercised end-to-end (no MemoryLoader bypass).

type DiagnosticLog = Arc<Mutex<Vec<(Url, Vec<Diagnostic>)>>>;

#[derive(Clone, Default)]
struct CapturingClient {
    last_diagnostics: DiagnosticLog,
}

#[async_trait]
impl LspClient for CapturingClient {
    async fn publish_diagnostics(&self, uri: Url, diags: Vec<Diagnostic>, _: Option<i32>) {
        self.last_diagnostics.lock().unwrap().push((uri, diags));
    }
    async fn show_message(&self, _: MessageType, _: String) {}
}
#[async_trait]
impl crate::trust_prompt::LspTrustRequester for CapturingClient {
    async fn send_trust_request(
        &self,
        _: crate::trust_prompt::TrustRequestParams,
    ) -> tower_lsp::jsonrpc::Result<crate::trust_prompt::TrustResponse> {
        // Tests run with no `[labels]` block; the prompt path is
        // not exercised.
        Ok(crate::trust_prompt::TrustResponse {
            decision: "denied".into(),
            reason: Some("test client".into()),
        })
    }
}

impl CapturingClient {
    fn diagnostics_for(&self, uri: &Url) -> Vec<Diagnostic> {
        self.last_diagnostics
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(u, _)| u == uri)
            .map(|(_, d)| d.clone())
            .unwrap_or_default()
    }
}

/// Build a temp directory with the given `(relpath, contents)` files,
/// open the entry file via the LSP, and return (server, capturing client,
/// entry uri, temp dir). The TempDir is returned so the caller keeps it
/// alive for the duration of the test (drop = cleanup).
async fn open_in_tempdir(
    files: &[(&str, &str)],
    entry: &str,
) -> (
    LexLanguageServer<CapturingClient>,
    CapturingClient,
    Url,
    tempfile::TempDir,
) {
    let dir = tempdir().expect("tempdir");
    for (rel, contents) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir -p");
        }
        std::fs::write(&path, contents).expect("write fixture");
    }
    let entry_path = dir.path().join(entry);
    let entry_text = std::fs::read_to_string(&entry_path).expect("read entry");
    let uri = Url::from_file_path(&entry_path).expect("file uri");

    let client = CapturingClient::default();
    let server = LexLanguageServer::new(client.clone());

    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "lex".into(),
                version: 1,
                text: entry_text,
            },
        })
        .await;

    (server, client, uri, dir)
}

fn has_diag_with_code(diags: &[Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| {
        matches!(
            &d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == code
        )
    })
}

#[tokio::test]
async fn includes_did_open_resolves_and_publishes_no_include_diagnostic() {
    let (_server, client, uri, _dir) = open_in_tempdir(
        &[
            (
                "main.lex",
                "1. Host\n\n    :: lex.include src=\"chapter.lex\" ::\n",
            ),
            ("chapter.lex", "1.1 Chapter\n\n    Body of chapter.\n"),
        ],
        "main.lex",
    )
    .await;

    let diags = client.diagnostics_for(&uri);
    assert!(
        !diags.iter().any(|d| matches!(
            &d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c.starts_with("include-")
        )),
        "successful include resolution should produce no include-* diagnostics, got {diags:?}"
    );
}

#[tokio::test]
async fn includes_missing_target_emits_diagnostic_with_path() {
    // The include sits on line 0, column 0 — flat fixture so the
    // diagnostic should pin to that exact location, not the
    // document head fallback (which would also be (0,0)–(0,0); the
    // distinction the test cares about is "did the resolver wire
    // annotation.location through to the diagnostic at all").
    let (_server, client, uri, _dir) = open_in_tempdir(
        &[("main.lex", ":: lex.include src=\"missing.lex\" ::\n")],
        "main.lex",
    )
    .await;

    let diags = client.diagnostics_for(&uri);
    assert!(
        has_diag_with_code(&diags, "include-not-found"),
        "missing include should surface include-not-found, got {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("missing.lex")),
        "diagnostic should name the missing file, got {diags:?}"
    );
    // The diagnostic must span more than a single point at (0,0).
    // The default `head_range()` fallback was (0,0)–(0,0), a
    // zero-width point — vscode renders nothing useful for that.
    // After wiring annotation.location through, the range covers
    // the annotation text.
    let not_found = diags
        .iter()
        .find(|d| {
            matches!(
                &d.code,
                Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "include-not-found"
            )
        })
        .expect("not-found diag");
    let r = &not_found.range;
    assert!(
        r.end.line > r.start.line || r.end.character > r.start.character,
        "include-not-found should span the annotation, not collapse to a point; got {r:?}",
    );
}

#[tokio::test]
async fn includes_cycle_emits_diagnostic_pointing_at_include_site() {
    let (_server, client, uri, _dir) = open_in_tempdir(
        &[
            ("main.lex", ":: lex.include src=\"a.lex\" ::\n"),
            ("a.lex", ":: lex.include src=\"b.lex\" ::\n"),
            ("b.lex", ":: lex.include src=\"a.lex\" ::\n"),
        ],
        "main.lex",
    )
    .await;

    let diags = client.diagnostics_for(&uri);
    assert!(
        has_diag_with_code(&diags, "include-cycle"),
        "cycle should surface include-cycle, got {diags:?}"
    );
    // The Cycle variant carries an include_site Range — the
    // diagnostic should point at it (not at the document head).
    let cycle = diags
        .iter()
        .find(|d| {
            matches!(
                &d.code,
                Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "include-cycle"
            )
        })
        .expect("cycle diag");
    // The site is in main.lex line 0 (the only include there).
    assert_eq!(cycle.range.start.line, 0);
}

#[tokio::test]
async fn includes_root_escape_emits_diagnostic() {
    let (_server, client, uri, _dir) = open_in_tempdir(
        &[(
            "main.lex",
            "1. Host\n\n    :: lex.include src=\"../../etc/passwd\" ::\n",
        )],
        "main.lex",
    )
    .await;

    let diags = client.diagnostics_for(&uri);
    assert!(
        has_diag_with_code(&diags, "include-root-escape"),
        "root escape should surface include-root-escape, got {diags:?}"
    );
}

#[tokio::test]
async fn includes_stored_tree_remains_unresolved_so_positions_match_host_buffer() {
    // The stored Document MUST be the unresolved parse of the host
    // buffer. Storing the merged tree would mix in nodes whose
    // Range.{start,end,span} reference the *included file's*
    // coordinate space, so semantic-token / hover / goto positions
    // served back to the editor would point at the wrong text.
    // (The merged tree is computed for diagnostic purposes only —
    // resolver errors get surfaced — and then dropped.)
    let (server, _client, uri, _dir) = open_in_tempdir(
        &[
            ("main.lex", ":: lex.include src=\"chapter.lex\" ::\n"),
            (
                "chapter.lex",
                "1. Spliced Chapter\n\n    Body content here.\n",
            ),
        ],
        "main.lex",
    )
    .await;

    let entry = server.document_entry(&uri).await.expect("entry stored");
    // Walk to find the session title — "1. Spliced Chapter" should
    // NOT be present in the host buffer's parse (it lives in the
    // included file).
    use lex_core::lex::ast::elements::content_item::ContentItem;
    let titles: Vec<String> = entry
        .document
        .root
        .children
        .iter()
        .filter_map(|i| match i {
            ContentItem::Session(s) => Some(s.title.as_string().to_string()),
            _ => None,
        })
        .collect();
    assert!(
        !titles.iter().any(|t| t == "1. Spliced Chapter"),
        "spliced chapter must NOT be in the stored host tree (its Ranges \
         would point at the wrong buffer); got titles {titles:?}"
    );
}

// ------------------------------------------------------------------
// Goto-def + hover for `lex.include` annotations (PR 9)
// ------------------------------------------------------------------

/// Build a `GotoDefinitionParams` pointing at a given (line, char)
/// inside `uri` — small helper to keep tests short.
fn goto_at(uri: &Url, line: u32, character: u32) -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn hover_at(uri: &Url, line: u32, character: u32) -> HoverParams {
    HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: Default::default(),
    }
}

#[tokio::test]
async fn goto_definition_on_include_returns_target_file_location() {
    let (server, _client, uri, dir) = open_in_tempdir(
        &[
            ("main.lex", ":: lex.include src=\"chapter.lex\" ::\n"),
            ("chapter.lex", "1. Chapter\n\n    Body.\n"),
        ],
        "main.lex",
    )
    .await;

    // Cursor on the `lex.include` annotation header (line 0).
    let response = server.goto_definition(goto_at(&uri, 0, 5)).await.unwrap();
    let location = match response {
        Some(GotoDefinitionResponse::Scalar(loc)) => loc,
        other => panic!("expected scalar Location, got {other:?}"),
    };

    // Target URI must point at chapter.lex (canonicalized via the
    // same absolutize_path the resolver uses).
    let expected =
        Url::from_file_path(absolutize_path(&dir.path().join("chapter.lex"))).expect("file uri");
    assert_eq!(location.uri, expected);
    // Range is the file head — cross-file goto-def lands at top-of-file.
    assert_eq!(location.range.start.line, 0);
    assert_eq!(location.range.start.character, 0);
}

#[tokio::test]
async fn goto_definition_off_include_falls_through_to_normal_logic() {
    // Cursor on a paragraph (NOT an include) — the include-aware
    // short-circuit must not fire, so the response comes from the
    // normal in-document goto path. With no references at this
    // position, that's None.
    let (server, _client, uri, _dir) = open_in_tempdir(
        &[("main.lex", "1. Chapter\n\n    Just a paragraph.\n")],
        "main.lex",
    )
    .await;
    let response = server.goto_definition(goto_at(&uri, 2, 8)).await.unwrap();
    assert!(
        response.is_none(),
        "non-include cursor should fall through, got {response:?}"
    );
}

#[tokio::test]
async fn hover_on_include_returns_preview_of_target_file() {
    let (server, _client, uri, _dir) = open_in_tempdir(
        &[
            ("main.lex", ":: lex.include src=\"chapter.lex\" ::\n"),
            ("chapter.lex", "1. Chapter\n\n    Body line.\n"),
        ],
        "main.lex",
    )
    .await;

    let hover = server
        .hover(hover_at(&uri, 0, 5))
        .await
        .unwrap()
        .expect("hover");
    let body = match hover.contents {
        HoverContents::Markup(m) => m.value,
        other => panic!("expected markup hover, got {other:?}"),
    };
    // Mentions the src parameter and the resolved path.
    assert!(
        body.contains("chapter.lex"),
        "hover should name target: {body}"
    );
    // Includes a preview chunk from the file content.
    assert!(
        body.contains("1. Chapter"),
        "hover should preview content: {body}"
    );
}

#[tokio::test]
async fn hover_off_include_falls_through_to_normal_hover() {
    // The default feature provider's hover is a no-op for plain
    // text positions, so we just check that the include-specific
    // path didn't fire and produce a phantom hover.
    let (server, _client, uri, _dir) = open_in_tempdir(
        &[("main.lex", "1. Chapter\n\n    Just text.\n")],
        "main.lex",
    )
    .await;
    let hover = server.hover(hover_at(&uri, 2, 8)).await.unwrap();
    if let Some(h) = hover {
        // If something does come back, it must NOT be the include
        // preview (which always mentions "lex.include").
        let body = match h.contents {
            HoverContents::Markup(m) => m.value,
            _ => String::new(),
        };
        assert!(
            !body.contains("lex.include"),
            "non-include cursor must not get include preview, got {body}"
        );
    }
}

#[tokio::test]
async fn goto_definition_on_include_with_missing_target_returns_none() {
    // A broken include (target file doesn't exist on disk) — goto-def
    // returns None so the editor renders its native "no definition
    // found" UX. The user already gets the missing-target signal via
    // the PR 8 `include-not-found` diagnostic; we don't want to also
    // navigate them to a phantom buffer.
    let (server, _client, uri, _dir) = open_in_tempdir(
        &[("main.lex", ":: lex.include src=\"missing.lex\" ::\n")],
        "main.lex",
    )
    .await;
    let response = server.goto_definition(goto_at(&uri, 0, 5)).await.unwrap();
    assert!(
        response.is_none(),
        "goto-def must return None for missing targets, got {response:?}"
    );
}

// ========================================================================
// lex.extractToInclude — end-to-end via executeCommand (lex#497)
// ========================================================================

#[tokio::test]
async fn extract_to_include_returns_workspace_edit_with_create_and_replace() {
    let host_text = "Doc\n===\n\n1. Section\n\n    Some content.\n    More content.\n";
    let (server, _client, uri, dir) = open_in_tempdir(&[("main.lex", host_text)], "main.lex").await;

    // Select the indented body of the section — lines 5–6, column 4–end.
    let range = Range::new(Position::new(5, 4), Position::new(7, 0));
    let result = server
        .execute_command(ExecuteCommandParams {
            command: commands::COMMAND_EXTRACT_TO_INCLUDE.to_string(),
            arguments: vec![
                Value::String(uri.to_string()),
                serde_json::to_value(range).unwrap(),
                Value::String("section-body.lex".to_string()),
            ],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .expect("extract command should return WorkspaceEdit");

    let edit: tower_lsp::lsp_types::WorkspaceEdit = serde_json::from_value(result).unwrap();
    let ops = match edit.document_changes.unwrap() {
        tower_lsp::lsp_types::DocumentChanges::Operations(ops) => ops,
        _ => panic!("expected operations"),
    };
    assert_eq!(ops.len(), 3, "create + target-content + host-replace");

    // First op: create target.
    match &ops[0] {
        tower_lsp::lsp_types::DocumentChangeOperation::Op(
            tower_lsp::lsp_types::ResourceOp::Create(c),
        ) => {
            assert!(c.uri.path().ends_with("section-body.lex"));
        }
        other => panic!("expected CreateFile, got {other:?}"),
    }

    // Second op writes the indent-shifted selection into the target.
    let target_text = match &ops[1] {
        tower_lsp::lsp_types::DocumentChangeOperation::Edit(e) => match &e.edits[0] {
            OneOf::Left(t) => t.new_text.clone(),
            _ => panic!("unexpected edit shape"),
        },
        _ => panic!("expected TextDocumentEdit for target"),
    };
    assert!(
        target_text.contains("Some content.") && target_text.contains("More content."),
        "target should hold the extracted body, got: {target_text:?}"
    );
    // Indent-shifted: should start at column 0.
    assert!(
        target_text.starts_with("Some content."),
        "expected indent shift to drop leading 4 spaces, got: {target_text:?}"
    );

    // Third op replaces the host range with `:: lex.include ::` at indent 4.
    let host_replace = match &ops[2] {
        tower_lsp::lsp_types::DocumentChangeOperation::Edit(e) => match &e.edits[0] {
            OneOf::Left(t) => t.new_text.clone(),
            _ => panic!("unexpected edit shape"),
        },
        _ => panic!("expected TextDocumentEdit for host"),
    };
    assert_eq!(
        host_replace,
        "    :: lex.include src=\"section-body.lex\" ::"
    );

    // Keep dir alive until end of test.
    drop(dir);
}

#[tokio::test]
async fn extract_to_include_surfaces_validation_errors_as_invalid_params() {
    let host_text = "Doc\n===\n\n    Body text.\n";
    let (server, _client, uri, _dir) =
        open_in_tempdir(&[("main.lex", host_text)], "main.lex").await;

    let range = Range::new(Position::new(3, 4), Position::new(4, 0));
    let err = server
        .execute_command(ExecuteCommandParams {
            command: commands::COMMAND_EXTRACT_TO_INCLUDE.to_string(),
            arguments: vec![
                Value::String(uri.to_string()),
                serde_json::to_value(range).unwrap(),
                Value::String("https://elsewhere/foo.lex".to_string()),
            ],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap_err();
    assert!(
        err.message.contains("URL"),
        "expected URL-scheme error message, got: {}",
        err.message
    );
}

#[tokio::test]
async fn extract_to_include_capability_advertises_command() {
    let server = LexLanguageServer::new(NoopClient);
    let init = server
        .initialize(InitializeParams::default())
        .await
        .unwrap();
    let advertised = init
        .capabilities
        .execute_command_provider
        .expect("execute_command_provider")
        .commands;
    assert!(
        advertised.contains(&commands::COMMAND_EXTRACT_TO_INCLUDE.to_string()),
        "extractToInclude must be in advertised commands, got: {advertised:?}"
    );
}

/// `slice_text_by_range` treats each endpoint's `Position.character`
/// (`range.start.character` / `range.end.character`) as a UTF-8 byte
/// offset, matching lex-core's `SourceLocation::byte_to_position`
/// (which sets `column = byte_offset - line_start`). Char-based
/// slicing would mis-slice selections containing multi-byte chars;
/// this test pins the byte semantics.
#[test]
fn slice_text_by_range_uses_utf8_byte_offsets() {
    let text = "café\nrestaurant\n";
    // The é is 2 UTF-8 bytes, so "café" occupies bytes 0..5.
    let range = Range::new(Position::new(0, 0), Position::new(0, 5));
    assert_eq!(slice_text_by_range(text, range).as_deref(), Some("café"));

    // Mid-character byte offset (between the two bytes of é) is rejected.
    let bad = Range::new(Position::new(0, 0), Position::new(0, 4));
    assert!(slice_text_by_range(text, bad).is_none());

    // Multi-line slice with non-ASCII in the source.
    let multi = Range::new(Position::new(0, 0), Position::new(1, 10));
    assert_eq!(
        slice_text_by_range(text, multi).as_deref(),
        Some("café\nrestaurant")
    );
}

#[tokio::test]
async fn includes_untitled_uri_skips_resolution_without_error() {
    // Untitled URIs (no on-disk anchor) can't drive include
    // resolution. The server must handle these gracefully — no
    // panics, no spurious include diagnostics.
    let client = CapturingClient::default();
    let server = LexLanguageServer::new(client.clone());
    let uri: Url = "untitled:Untitled-1".parse().unwrap();
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "lex".into(),
                version: 1,
                text: "1. Host\n\n    Some content.\n".to_string(),
            },
        })
        .await;

    let diags = client.diagnostics_for(&uri);
    assert!(
        !diags.iter().any(|d| matches!(
            &d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c.starts_with("include-")
        )),
        "untitled URIs should produce no include-* diagnostics, got {diags:?}"
    );
}

// ========================================================================
// Smart paste — `lex/preparePaste` request wiring (comms#73).
//
// The transform itself is exhaustively table-tested in
// `lex_lsp_core::prepare_paste`; these tests cover the server-side wiring:
// capability advertisement, document-store lookup, and the missing-buffer
// fallback.
// ========================================================================

#[tokio::test]
async fn initialize_advertises_prepare_paste_capability() {
    let server = LexLanguageServer::new(NoopClient);

    let result = server
        .initialize(InitializeParams::default())
        .await
        .unwrap();

    let experimental = result
        .capabilities
        .experimental
        .expect("experimental capabilities advertised");
    assert_eq!(experimental["lexPreparePaste"], serde_json::json!(true));
}

#[tokio::test]
async fn prepare_paste_reanchors_against_open_buffer() {
    let server = LexLanguageServer::new(NoopClient);
    let uri = Url::from_file_path("/tmp/paste.lex").unwrap();

    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "lex".into(),
                version: 1,
                text: "Top:\n\n    existing\n\n".to_string(),
            },
        })
        .await;

    // Fresh blank line 3, inside the session (content indent 4). Paste a
    // column-zero two-line block; both lines re-anchor to indent 4.
    let result = server
        .prepare_paste(PreparePasteParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range {
                start: Position::new(3, 0),
                end: Position::new(3, 0),
            },
            pasted_text: "first\n    second\n".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(result.mode, PasteMode::Reanchor);
    assert_eq!(result.text, "    first\n        second\n");
}

#[tokio::test]
async fn prepare_paste_passes_through_verbatim() {
    let server = LexLanguageServer::new(NoopClient);
    let uri = Url::from_file_path("/tmp/verb.lex").unwrap();

    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "lex".into(),
                version: 1,
                text: "Code:\n    line one\n    line two\n:: text ::\n".to_string(),
            },
        })
        .await;

    let pasted = "  weird\n      indent\n".to_string();
    let result = server
        .prepare_paste(PreparePasteParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range {
                start: Position::new(1, 8),
                end: Position::new(1, 8),
            },
            pasted_text: pasted.clone(),
        })
        .await
        .unwrap();

    assert_eq!(result.mode, PasteMode::PassthroughVerbatim);
    assert_eq!(result.text, pasted);
}

#[tokio::test]
async fn prepare_paste_unopened_buffer_echoes_clipboard() {
    let server = LexLanguageServer::new(NoopClient);
    let uri = Url::from_file_path("/tmp/never-opened.lex").unwrap();

    let pasted = "anything\n    here\n".to_string();
    let result = server
        .prepare_paste(PreparePasteParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range {
                start: Position::new(0, 0),
                end: Position::new(0, 0),
            },
            pasted_text: pasted.clone(),
        })
        .await
        .unwrap();

    // No parse to consult — clipboard echoed back unchanged so the editor
    // still completes the paste.
    assert_eq!(result.text, pasted);
}
