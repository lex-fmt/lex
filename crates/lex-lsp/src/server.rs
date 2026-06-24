//! Main language server implementation

use std::path::PathBuf;
use std::sync::Arc;

use crate::extension_dispatch::{
    dispatch_code_action as ext_dispatch_code_action,
    dispatch_completion as ext_dispatch_completion, dispatch_hover as ext_dispatch_hover,
    LspExtensionState,
};
use crate::features::commands::{self};
use crate::features::document_links::collect_document_links;
use crate::features::document_symbols::collect_document_symbols;
use crate::features::folding_ranges::folding_ranges as collect_folding_ranges;
use crate::features::formatting::{self};
use crate::features::go_to_definition::goto_definition;
use crate::features::hover::hover as compute_hover;
use crate::features::references::find_references;
use crate::features::semantic_tokens::collect_semantic_tokens;
use lex_analysis::completion::{completion_items, CompletionWorkspace};
use lex_analysis::diagnostics::{analyze as analyze_diagnostics, apply_rules};
use lex_babel::formats::lex::formatting_rules::FormattingRules;
use lex_config::LoadedLexConfig;
use lex_core::lex::ast::Document;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tower_lsp::async_trait;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeActionParams, CodeActionProviderCapability, CodeActionResponse, CompletionItem,
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeConfigurationParams,
    DidChangeWorkspaceFoldersParams, DocumentFormattingParams, DocumentLink, DocumentLinkOptions,
    DocumentLinkParams, DocumentRangeFormattingParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, ExecuteCommandOptions, ExecuteCommandParams, FoldingRange,
    FoldingRangeParams, FoldingRangeProviderCapability, FormattingOptions, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, Location, MarkupContent, MarkupKind,
    OneOf, ReferenceParams, SemanticTokens, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensResult, ServerCapabilities, ServerInfo, TextDocumentItem,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url, WorkDoneProgressOptions,
    WorkspaceFoldersServerCapabilities,
};
use tower_lsp::Client;

use tower_lsp::lsp_types::Diagnostic;

use tower_lsp::lsp_types::MessageType;

mod command_dispatch;
mod config_loading;
mod convert;
mod diagnostics;
mod document_store;
mod extension_boot;
mod includes;
#[cfg(test)]
mod tests;

// Re-export the extracted submodule items at the `server::` path so the
// crate-internal call sites in this file (and the test module) keep
// referring to them by bare name, and any existing `crate::server::*`
// path stays valid. These were all crate-private free items before the
// split; the re-exports preserve their reachability without widening the
// crate's public API.
pub(crate) use config_loading::{absolutize_path, best_matching_root, inc_root_for, load_config};
pub(crate) use convert::{
    apply_formatting_overrides, build_document_link, encode_semantic_tokens, from_lsp_position,
    head_range, include_preview_markdown, indent_level_from_position, semantic_tokens_legend,
    slice_text_by_range, spans_to_text_edits, to_document_symbol, to_formatting_line_range,
    to_lsp_completion_item, to_lsp_folding_range, to_lsp_location, to_lsp_range,
};
pub(crate) use diagnostics::{
    include_error_to_diagnostic, registry_setup_diagnostic, to_lsp_diagnostic,
};
pub(crate) use document_store::{document_directory_from_uri, DocumentEntry, DocumentStore};

#[async_trait]
pub trait LspClient:
    crate::trust_prompt::LspTrustRequester + Send + Sync + Clone + 'static
{
    async fn publish_diagnostics(&self, uri: Url, diags: Vec<Diagnostic>, version: Option<i32>);
    async fn show_message(&self, typ: MessageType, message: String);
}

#[async_trait]
impl LspClient for Client {
    async fn publish_diagnostics(&self, uri: Url, diags: Vec<Diagnostic>, version: Option<i32>) {
        self.publish_diagnostics(uri, diags, version).await;
    }

    async fn show_message(&self, typ: MessageType, message: String) {
        self.show_message(typ, message).await;
    }
}

pub struct LexLanguageServer<C = Client> {
    pub(crate) client: C,
    pub(crate) documents: DocumentStore,
    pub(crate) workspace_roots: RwLock<Vec<PathBuf>>,
    pub(crate) config: RwLock<LoadedLexConfig>,
    /// Extension registry + boot diagnostics, lazily populated on first
    /// extension-aware request (hover/completion/code_action). Held for
    /// the lifetime of the workspace; rebuilt when workspace folders
    /// change. `None` when the LSP is running outside any workspace
    /// (e.g. a single untitled buffer) — extension dispatch is a no-op
    /// in that case, and built-in providers handle every request.
    pub(crate) extension: RwLock<Option<Arc<LspExtensionState>>>,
    /// Serializes concurrent calls to [`Self::extension_state`] so the
    /// first request to land at boot does the work and every other
    /// request waits for that single boot to finish — instead of all
    /// of them racing into `spawn_blocking` and producing N parallel
    /// schema reads, N parallel subprocess spawns, and N parallel
    /// `lex/trustRequest` prompts to the editor. Naturally happens
    /// when N requests arrive on file-open (semantic tokens + hover +
    /// document symbols + folding + …).
    pub(crate) extension_init: tokio::sync::Mutex<()>,
}

impl<C> LexLanguageServer<C>
where
    C: LspClient,
{
    pub fn new(client: C) -> Self {
        let config = load_config(None);
        Self {
            client,
            documents: DocumentStore::default(),
            workspace_roots: RwLock::new(Vec::new()),
            config: RwLock::new(config),
            extension: RwLock::new(None),
            extension_init: tokio::sync::Mutex::new(()),
        }
    }

    async fn parse_and_store(&self, uri: Url, text: String) {
        // Try include resolution first when the document has an on-disk
        // path. If resolution succeeds, the resolved (merged) tree is what
        // we store and analyze; downstream features (semantic tokens,
        // hover, goto) see the post-include AST. If resolution fails, we
        // fall back to a plain parse so the rest of the LSP keeps working,
        // and surface the include error as a diagnostic at the include
        // site.
        let include_diags = self.resolve_and_upsert(&uri, &text).await;

        let mut diagnostics: Vec<Diagnostic> = include_diags;
        if let Some(entry) = self.documents.get(&uri).await {
            let mut analysis_diags = analyze_diagnostics(&entry.document);
            let cfg = self.config.read().await;
            apply_rules(&mut analysis_diags, |code| {
                cfg.lookup_diagnostic_rule(code).cloned()
            });
            drop(cfg);
            diagnostics.extend(analysis_diags.into_iter().map(to_lsp_diagnostic));
        }

        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    pub(crate) async fn document_entry(&self, uri: &Url) -> Option<DocumentEntry> {
        self.documents.get(uri).await
    }

    pub(crate) async fn document(&self, uri: &Url) -> Option<Arc<Document>> {
        self.document_entry(uri).await.map(|entry| entry.document)
    }

    #[allow(deprecated)]
    async fn update_workspace_roots(&self, params: &InitializeParams) {
        let mut roots = Vec::new();

        if let Some(folders) = params.workspace_folders.as_ref() {
            for folder in folders {
                if let Ok(path) = folder.uri.to_file_path() {
                    roots.push(path);
                }
            }
        }

        if roots.is_empty() {
            if let Some(root_uri) = params.root_uri.as_ref() {
                if let Ok(path) = root_uri.to_file_path() {
                    roots.push(path);
                }
            } else if let Some(root_path) = params.root_path.as_ref() {
                roots.push(PathBuf::from(root_path));
            } else if let Ok(current_dir) = std::env::current_dir() {
                roots.push(current_dir);
            }
        }

        *self.workspace_roots.write().await = roots;
    }

    async fn workspace_context_for_uri(&self, uri: &Url) -> Option<CompletionWorkspace> {
        let document_path = uri.to_file_path().ok()?;
        let roots = self.workspace_roots.read().await;
        let project_root = best_matching_root(&roots, &document_path)
            .or_else(|| document_directory_from_uri(uri))
            .or_else(|| document_path.parent().map(|path| path.to_path_buf()))
            .unwrap_or_else(|| document_path.clone());

        Some(CompletionWorkspace {
            project_root,
            document_path,
        })
    }

    /// Build formatting rules from stored config, with per-request LSP overrides on top.
    async fn resolve_formatting_rules(&self, options: &FormattingOptions) -> FormattingRules {
        let config = self.config.read().await;
        let mut rules = FormattingRules::from(&config.config.formatting.rules);

        // Layer per-request LSP overrides (editors can send lex.* properties)
        apply_formatting_overrides(&mut rules, options);

        rules
    }
}

#[async_trait]
impl<C> tower_lsp::LanguageServer for LexLanguageServer<C>
where
    C: LspClient,
{
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        self.update_workspace_roots(&params).await;

        // Reload config now that we know the workspace root
        {
            let roots = self.workspace_roots.read().await;
            let root = roots.first().map(|p| p.as_path());
            *self.config.write().await = load_config(root);
        }

        let capabilities = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            document_symbol_provider: Some(OneOf::Left(true)),
            folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
            definition_provider: Some(OneOf::Left(true)),
            references_provider: Some(OneOf::Left(true)),
            document_link_provider: Some(DocumentLinkOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
                resolve_provider: Some(false),
            }),
            code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
            completion_provider: Some(CompletionOptions {
                resolve_provider: Some(false),
                trigger_characters: Some(vec![
                    "[".to_string(),
                    ":".to_string(),
                    "=".to_string(),
                    "@".to_string(),
                ]),
                work_done_progress_options: WorkDoneProgressOptions::default(),
                all_commit_characters: None,
                ..Default::default()
            }),
            document_formatting_provider: Some(OneOf::Left(true)),
            document_range_formatting_provider: Some(OneOf::Left(true)),
            semantic_tokens_provider: Some(
                lsp_types::SemanticTokensServerCapabilities::SemanticTokensOptions(
                    SemanticTokensOptions {
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                        legend: semantic_tokens_legend(),
                        range: None,
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                    },
                ),
            ),
            execute_command_provider: Some(ExecuteCommandOptions {
                commands: vec![
                    commands::COMMAND_ECHO.to_string(),
                    commands::COMMAND_IMPORT.to_string(),
                    commands::COMMAND_EXPORT.to_string(),
                    commands::COMMAND_NEXT_ANNOTATION.to_string(),
                    commands::COMMAND_PREVIOUS_ANNOTATION.to_string(),
                    commands::COMMAND_RESOLVE_ANNOTATION.to_string(),
                    commands::COMMAND_TOGGLE_ANNOTATIONS.to_string(),
                    commands::COMMAND_INSERT_ASSET.to_string(),
                    commands::COMMAND_INSERT_VERBATIM.to_string(),
                    commands::COMMAND_FOOTNOTES_REORDER.to_string(),
                    commands::COMMAND_TABLE_FORMAT.to_string(),
                    commands::COMMAND_TABLE_NEXT_CELL.to_string(),
                    commands::COMMAND_TABLE_PREVIOUS_CELL.to_string(),
                    commands::COMMAND_FORMATS_LIST.to_string(),
                    commands::COMMAND_EXTRACT_TO_INCLUDE.to_string(),
                ],
                work_done_progress_options: WorkDoneProgressOptions::default(),
            }),
            workspace: Some(lsp_types::WorkspaceServerCapabilities {
                workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                    supported: Some(true),
                    change_notifications: Some(OneOf::Left(true)),
                }),
                file_operations: None,
            }),
            // Advertise the custom `lex/preparePaste` request under
            // `experimental` so editors enable paste interception only against a
            // server that implements smart paste (comms#73 §5); a server without
            // this flag falls back to native paste.
            experimental: Some(json!({ "lexPreparePaste": true })),
            ..ServerCapabilities::default()
        };

        Ok(InitializeResult {
            capabilities,
            server_info: Some(ServerInfo {
                name: "lexd-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {}

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        let mut roots = self.workspace_roots.write().await;

        // Remove old folders
        for removed in &params.event.removed {
            if let Ok(path) = removed.uri.to_file_path() {
                roots.retain(|r| r != &path);
            }
        }

        // Add new folders
        for added in &params.event.added {
            if let Ok(path) = added.uri.to_file_path() {
                if !roots.contains(&path) {
                    roots.push(path);
                }
            }
        }

        // Reload config from the first (primary) root
        drop(roots);
        let roots = self.workspace_roots.read().await;
        let root = roots.first().map(|p| p.as_path());
        *self.config.write().await = load_config(root);

        // Workspace shape changed — drop the cached extension registry
        // so the next request rebuilds it against the new root + config.
        self.invalidate_extension_state().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: lsp_types::DidOpenTextDocumentParams) {
        let TextDocumentItem { uri, text, .. } = params.text_document;
        self.parse_and_store(uri, text).await;
    }

    async fn did_change_configuration(&self, _params: DidChangeConfigurationParams) {
        // Reload config from disk (e.g. .lex.toml changed)
        {
            let roots = self.workspace_roots.read().await;
            let root = roots.first().map(|p| p.as_path());
            *self.config.write().await = load_config(root);
        }

        // Config changed — `[labels]` may have grown / shrunk; drop the
        // cached extension registry so the next request rebuilds it.
        self.invalidate_extension_state().await;

        // Re-check all documents with new settings
        let uris: Vec<Url> = self
            .documents
            .entries
            .read()
            .await
            .keys()
            .cloned()
            .collect();

        for uri in uris {
            if let Some(entry) = self.documents.get(&uri).await {
                self.parse_and_store(uri, entry.text.to_string()).await;
            }
        }
    }
    async fn did_change(&self, params: lsp_types::DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.parse_and_store(params.text_document.uri, change.text)
                .await;
        }
    }

    async fn did_close(&self, params: lsp_types::DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri).await;
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        if let Some(entry) = self.document_entry(&params.text_document.uri).await {
            let DocumentEntry { document, text } = entry;
            let tokens = collect_semantic_tokens(&document);
            let data = encode_semantic_tokens(&tokens, text.as_str());
            Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data,
            })))
        } else {
            Ok(None)
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        if let Some(document) = self.document(&params.text_document.uri).await {
            let symbols = collect_document_symbols(&document);
            let converted: Vec<DocumentSymbol> = symbols.iter().map(to_document_symbol).collect();
            Ok(Some(DocumentSymbolResponse::Nested(converted)))
        } else {
            Ok(None)
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        if let Some(document) = self.document(uri).await {
            let position = from_lsp_position(params.text_document_position_params.position);

            // Include-aware short-circuit: if the cursor is on a
            // `lex.include` annotation, render a preview of the
            // target file's title + first paragraph instead of falling
            // through to the generic hover. This is the editor UX win
            // — author can peek the chapter without navigating away.
            if let Some(hover) = self.hover_for_include(uri, &document, position).await {
                return Ok(Some(hover));
            }

            // Extension dispatch: ask any registered third-party
            // namespace's handler for hover content at this position.
            // Takes precedence over the built-in hover when it returns
            // Some — the handler authored the label, it knows the most
            // about what to show.
            if let Some(state) = self.extension_state().await {
                if let Some(hover) =
                    ext_dispatch_hover(&document, position, state.registry.as_ref())
                {
                    return Ok(Some(hover));
                }
            }

            if let Some(result) = compute_hover(&document, position) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: result.contents,
                    }),
                    range: Some(to_lsp_range(&result.range)),
                }));
            }
        }
        Ok(None)
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        if let Some(document) = self.document(&params.text_document.uri).await {
            let ranges = collect_folding_ranges(&document);
            Ok(Some(ranges.iter().map(to_lsp_folding_range).collect()))
        } else {
            Ok(None)
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        if let Some(document) = self.document(&uri).await {
            let position = from_lsp_position(params.text_document_position_params.position);

            // Include-aware short-circuit: if cursor is on a
            // `lex.include` annotation, jump to the target file rather
            // than running the in-document goto logic (which only
            // returns Ranges, can't cross files).
            if let Some(loc) = self.goto_for_include(&uri, &document, position).await {
                return Ok(Some(GotoDefinitionResponse::Scalar(loc)));
            }

            let ranges = goto_definition(&document, position);
            if ranges.is_empty() {
                Ok(None)
            } else {
                let locations: Vec<Location> = ranges
                    .iter()
                    .map(|range| to_lsp_location(&uri, range))
                    .collect();
                Ok(Some(GotoDefinitionResponse::Array(locations)))
            }
        } else {
            Ok(None)
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        if let Some(document) = self.document(&uri).await {
            let position = from_lsp_position(params.text_document_position.position);
            let include_declaration = params.context.include_declaration;
            let ranges = find_references(&document, position, include_declaration);
            if ranges.is_empty() {
                Ok(None)
            } else {
                Ok(Some(
                    ranges
                        .iter()
                        .map(|range| to_lsp_location(&uri, range))
                        .collect(),
                ))
            }
        } else {
            Ok(None)
        }
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let uri = params.text_document.uri;
        if let Some(document) = self.document(&uri).await {
            let links = collect_document_links(&document);
            let resolved: Vec<DocumentLink> = links
                .iter()
                .filter_map(|link| build_document_link(&uri, link))
                .collect();
            Ok(Some(resolved))
        } else {
            Ok(None)
        }
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        if let Some(entry) = self.document_entry(&uri).await {
            let DocumentEntry { document, text } = entry;
            let rules = self.resolve_formatting_rules(&params.options).await;
            let edits = formatting::format_document(&document, text.as_str(), Some(rules));
            Ok(Some(spans_to_text_edits(text.as_str(), edits)))
        } else {
            Ok(None)
        }
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        if let Some(entry) = self.document_entry(&uri).await {
            let DocumentEntry { document, text } = entry;
            let line_range = to_formatting_line_range(&params.range);
            let rules = self.resolve_formatting_rules(&params.options).await;
            let edits = formatting::format_range(&document, text.as_str(), line_range, Some(rules));
            Ok(Some(spans_to_text_edits(text.as_str(), edits)))
        } else {
            Ok(None)
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        if let Some(entry) = self.document_entry(&uri).await {
            let DocumentEntry { document, text } = entry;
            let position = from_lsp_position(params.text_document_position.position);
            let workspace = self.workspace_context_for_uri(&uri).await;

            // Extract trigger character from context
            let trigger_char = params
                .context
                .as_ref()
                .and_then(|ctx| ctx.trigger_character.as_deref());

            // Extract current line text for resilient parsing (e.g. "::" without following newline)
            let current_line = text.lines().nth(position.line);

            let candidates = completion_items(
                &document,
                position,
                current_line,
                workspace.as_ref(),
                trigger_char,
            );
            let mut items: Vec<CompletionItem> =
                candidates.iter().map(to_lsp_completion_item).collect();

            // Extension dispatch: append handler-supplied completions.
            // Additive — the built-in items still appear so the user
            // doesn't lose access to footnote/reference/snippet
            // completions when an extension is loaded.
            if let Some(state) = self.extension_state().await {
                items.extend(ext_dispatch_completion(
                    &document,
                    position,
                    state.registry.as_ref(),
                ));
            }

            Ok(Some(CompletionResponse::Array(items)))
        } else {
            Ok(None)
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let mut actions = Vec::new();

        let document_uri = params.text_document.uri.clone();
        if let Some(entry) = self.documents.get(&document_uri).await {
            let lex_actions = crate::features::available_actions::compute_actions(
                &entry.document,
                &entry.text,
                &params,
            );
            for action in lex_actions {
                actions.push(tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(
                    action,
                ));
            }

            // Extension dispatch: append handler-supplied code actions
            // for the labelled node under the request's selection. The
            // request's range start is the position we use to locate
            // the labelled node — `compute_actions` already operates
            // off the same anchor.
            if let Some(state) = self.extension_state().await {
                let start = from_lsp_position(params.range.start);
                for action in ext_dispatch_code_action(
                    &entry.document,
                    start,
                    &document_uri,
                    state.registry.as_ref(),
                ) {
                    actions.push(tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(
                        action,
                    ));
                }
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        self.execute_command_dispatch(params).await
    }
}
