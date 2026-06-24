//! Main language server implementation

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::extension_dispatch::{
    dispatch_code_action as ext_dispatch_code_action,
    dispatch_completion as ext_dispatch_completion, dispatch_hover as ext_dispatch_hover,
    LspExtensionState,
};
use crate::features::commands::{self, execute_command};
use crate::features::document_links::collect_document_links;
use crate::features::document_symbols::collect_document_symbols;
use crate::features::extract::{self, ExtractError};
use crate::features::folding_ranges::folding_ranges as collect_folding_ranges;
use crate::features::formatting::{self};
use crate::features::go_to_definition::goto_definition;
use crate::features::hover::hover as compute_hover;
use crate::features::references::find_references;
use crate::features::semantic_tokens::collect_semantic_tokens;
use lex_analysis::completion::{completion_items, CompletionWorkspace};
use lex_analysis::diagnostics::{analyze as analyze_diagnostics, apply_rules};
use lex_babel::formats::lex::formatting_rules::FormattingRules;
use lex_babel::templates::{
    build_asset_snippet, build_verbatim_snippet, AssetSnippetRequest, VerbatimSnippetRequest,
};
use lex_config::{LabelsConfig, LoadedLexConfig};
use lex_core::lex::ast::{Document, Position as AstPosition};
use lex_core::lex::builtins as lex_builtins;
use lex_core::lex::includes::{FsLoader, ResolveConfig};
use lex_lsp_core::prepare_paste::{
    prepare_paste as prepare_paste_transform, PasteMode, PreparePasteParams, PreparePasteResult,
};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tower_lsp::async_trait;
use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::{
    CodeActionParams, CodeActionProviderCapability, CodeActionResponse, CompletionItem,
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeConfigurationParams,
    DidChangeWorkspaceFoldersParams, DocumentFormattingParams, DocumentLink, DocumentLinkOptions,
    DocumentLinkParams, DocumentRangeFormattingParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, ExecuteCommandOptions, ExecuteCommandParams, FoldingRange,
    FoldingRangeParams, FoldingRangeProviderCapability, FormattingOptions, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, Location, MarkupContent, MarkupKind,
    OneOf, Position, Range, ReferenceParams, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult, ServerCapabilities,
    ServerInfo, TextDocumentItem, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url,
    WorkDoneProgressOptions, WorkspaceFoldersServerCapabilities,
};
use tower_lsp::Client;

use tower_lsp::lsp_types::Diagnostic;

use tower_lsp::lsp_types::MessageType;

mod config_loading;
mod convert;
mod diagnostics;
mod document_store;
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
    client: C,
    documents: DocumentStore,
    workspace_roots: RwLock<Vec<PathBuf>>,
    config: RwLock<LoadedLexConfig>,
    /// Extension registry + boot diagnostics, lazily populated on first
    /// extension-aware request (hover/completion/code_action). Held for
    /// the lifetime of the workspace; rebuilt when workspace folders
    /// change. `None` when the LSP is running outside any workspace
    /// (e.g. a single untitled buffer) — extension dispatch is a no-op
    /// in that case, and built-in providers handle every request.
    extension: RwLock<Option<Arc<LspExtensionState>>>,
    /// Serializes concurrent calls to [`Self::extension_state`] so the
    /// first request to land at boot does the work and every other
    /// request waits for that single boot to finish — instead of all
    /// of them racing into `spawn_blocking` and producing N parallel
    /// schema reads, N parallel subprocess spawns, and N parallel
    /// `lex/trustRequest` prompts to the editor. Naturally happens
    /// when N requests arrive on file-open (semantic tokens + hover +
    /// document symbols + folding + …).
    extension_init: tokio::sync::Mutex<()>,
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

    /// Lazily boot the extension registry against the current workspace
    /// root + config. Idempotent: once built, returns the cached state.
    /// Returns `None` when no workspace is set (e.g. single-file mode);
    /// extension dispatch is a no-op without a workspace anchor.
    ///
    /// Concurrency: the first request to land on a fresh workspace
    /// takes the `extension_init` mutex and runs the boot; every
    /// other concurrent request blocks on the mutex, then re-checks
    /// the cache and reuses what the first one produced. Without
    /// this serialization, an open-file event that fires several
    /// providers at once (hover, completion, semantic tokens, folding,
    /// document-symbols, …) would launch N parallel boots, with N
    /// concurrent reads of the schema directory, N concurrent
    /// subprocess spawns, and N `lex/trustRequest` prompts to the
    /// editor. The mutex keeps the observable side effects to one
    /// prompt and one set of spawns.
    async fn extension_state(&self) -> Option<Arc<LspExtensionState>> {
        // Fast path: already booted, no lock needed.
        if let Some(s) = self.extension.read().await.clone() {
            return Some(s);
        }

        // Slow path: serialize boot. Hold the init lock for the whole
        // boot so the second-arriving request waits for the first.
        let _guard = self.extension_init.lock().await;

        // Re-check after acquiring the init lock — another task may
        // have completed boot while we were waiting on the mutex.
        if let Some(s) = self.extension.read().await.clone() {
            return Some(s);
        }

        let workspace_root = {
            let roots = self.workspace_roots.read().await;
            roots.first().cloned()?
        };
        let labels_config = LabelsConfig {
            namespaces: self.config.read().await.config.labels.clone(),
        };

        // boot_registry does synchronous filesystem IO (schema load,
        // trust store open) and may attempt to spawn subprocess
        // handlers — a few hundred milliseconds in the worst case. Run
        // it on the blocking thread pool so the tokio runtime keeps
        // serving other LSP requests while boot runs.
        //
        // The trust prompt handler bridges sync→async via
        // `Handle::block_on` — safe because we're on a blocking-pool
        // thread, not a runtime worker.
        let workspace_root_owned = workspace_root.clone();
        let trust_requester = std::sync::Arc::new(self.client.clone());
        let runtime_handle = tokio::runtime::Handle::current();
        let outcome = match tokio::task::spawn_blocking(move || {
            lex_fmt::boot_registry(lex_fmt::ExtensionSetup {
                workspace_root: workspace_root_owned.as_path(),
                labels_config: &labels_config,
                // The LSP server has no `--ext-schema` flag; only
                // `[labels]` entries from `lex.toml` register schemas
                // in this surface.
                ext_schemas: &[],
                // `enable_handlers` is irrelevant on the Lsp surface —
                // that flag is the CLI shortcut for the trust-prompt
                // path. The LSP consults the trust store + prompt
                // handler directly.
                enable_handlers: false,
                surface_override: Some(lex_extension_host::Surface::LspSession),
                // Forwards `lex/trustRequest` to the editor and awaits
                // the user's decision. Already-pinned decisions in
                // `<workspace>/.lex/trust.json` short-circuit before
                // the prompt fires.
                trust_prompt: Box::new(crate::trust_prompt::LspPromptHandler::new(
                    trust_requester,
                    runtime_handle,
                )),
                // Reports `lexd-lsp`'s version to subprocess handlers
                // in their initialize handshake — what handlers expect
                // to see, not the `lex-fmt` boot crate's version.
                host_version: env!("CARGO_PKG_VERSION"),
            })
        })
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => {
                // Blocking task panicked or was cancelled. Skip
                // extension boot for this session; the next request
                // will retry.
                return None;
            }
        };

        // Surface boot diagnostics to the editor before we cache the
        // state. Per-namespace failures (resolver errors, denied
        // subprocess handlers, schema load problems) are stored on
        // the outcome but the user has no way to see them otherwise
        // — pre-validation diagnostics are attached to documents,
        // but boot diagnostics aren't. `window/showMessage` is the
        // right surface for one-shot status that's not tied to a
        // specific document range.
        for diag in &outcome.diagnostics {
            let prefix = match &diag.namespace {
                Some(ns) => format!("lex extension `{ns}`: "),
                None => "lex extensions: ".to_string(),
            };
            self.client
                .show_message(MessageType::WARNING, format!("{prefix}{}", diag.message))
                .await;
        }

        // Cross-check `[diagnostics.rules]` extension entries against the
        // freshly-booted registry. A `<namespace>.<code>` rule whose
        // namespace is registered but doesn't declare the code is a dead
        // letter — it retunes nothing. Surface each as a warning so the
        // misspelling is visible; like boot diagnostics, it's session
        // status with no document range to attach to. Unregistered
        // namespaces pass silently (the user may install the extension
        // later), so this never fires for staged-ahead rules.
        // Collect findings under the lock, then drop it before awaiting
        // any `show_message` — holding the config read lock across the
        // network await could starve a concurrent config write.
        let rule_findings = {
            let cfg = self.config.read().await;
            lex_fmt::validate_extension_diagnostic_rules(
                &cfg.extension_diagnostic_rules,
                &outcome.registry,
            )
        };
        for finding in rule_findings {
            self.client
                .show_message(MessageType::WARNING, finding.message)
                .await;
        }

        let state = Arc::new(LspExtensionState::from(outcome));
        *self.extension.write().await = Some(state.clone());
        Some(state)
    }

    /// Discard the cached extension registry. Called when workspace
    /// folders change so the next request picks up the new root +
    /// config.
    async fn invalidate_extension_state(&self) {
        *self.extension.write().await = None;
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

    /// Drives include resolution (when the URI is a file path) for
    /// *diagnostic* purposes only. Always stores the **unresolved**
    /// parse under `uri`; that's what every LSP feature
    /// (semantic tokens, hover, goto-definition, document symbols) sees.
    ///
    /// Why not store the merged tree: nodes spliced in from included
    /// files carry Ranges in the *included file's* coordinate space —
    /// `range.start.line == 0` means "line 0 of chapter.lex", not
    /// "line 0 of the host buffer." Serving those ranges back as if
    /// they were positions in the host URI's text would highlight the
    /// wrong tokens, send goto-definition to the wrong spot, etc. Until
    /// we have an origin-path-aware location-mapping layer (PR 9+),
    /// the safe behavior is to use the merged tree only to decide
    /// whether resolution succeeded, and emit diagnostics if it didn't.
    ///
    /// Returns include-related diagnostics: empty on success or when
    /// the document doesn't use includes at all; one diagnostic
    /// pointing at the include site (or document head as fallback) on
    /// resolver failure.
    async fn resolve_and_upsert(&self, uri: &Url, text: &str) -> Vec<Diagnostic> {
        // Standard parse goes in regardless — this is the tree every
        // LSP feature works against.
        self.documents.upsert(uri.clone(), text.to_string()).await;

        // Fast path: no `lex.include` literal in source, nothing to
        // resolve, nothing to diagnose. Avoids per-keystroke resolver
        // work for documents that don't use the feature, and prevents
        // the resolver's `ParseFailed` from firing as a spurious
        // include diagnostic for ordinary parse errors.
        if !text.contains("lex.include") {
            return Vec::new();
        }

        let path = match uri.to_file_path() {
            Ok(p) => p,
            // Untitled / non-file URIs (e.g. `untitled:Untitled-1`)
            // can't anchor relative include paths.
            Err(_) => return Vec::new(),
        };

        // Canonicalize the entry path so it lives in the same absolute-
        // path space as `inc_root` (`absolutize_path` calls
        // `Path::canonicalize` which follows symlinks — important on
        // macOS where /var → /private/var). Without this, host_dir
        // (path.parent()) and inc_root differ by symlink resolution and
        // every lookup fails the root-escape prefix check.
        let path = absolutize_path(&path);

        let cfg = self.config.read().await;
        let inc_root = inc_root_for(&path, &cfg.config);
        let max_depth = cfg.config.includes.max_depth;
        let max_total_includes = cfg.config.includes.max_total_includes;
        let max_file_size = cfg.config.includes.max_file_size;
        drop(cfg);

        let resolve_config = ResolveConfig {
            root: inc_root,
            max_depth,
            max_total_includes,
        };

        match lex_builtins::resolve_buffer(text, Some(path), &resolve_config, max_file_size) {
            // Resolution succeeded. We *don't* store the merged tree —
            // see fn-level docstring. The resolver was run only to
            // surface errors; the tree itself is dropped.
            Ok(_doc) => Vec::new(),
            Err(lex_builtins::ResolveBufferError::Registry(e)) => {
                vec![registry_setup_diagnostic(&e.to_string())]
            }
            Err(lex_builtins::ResolveBufferError::Resolve(err)) => {
                vec![include_error_to_diagnostic(&err)]
            }
        }
    }

    async fn document_entry(&self, uri: &Url) -> Option<DocumentEntry> {
        self.documents.get(uri).await
    }

    /// Resolve a `lex.include` annotation at `position` to a Location
    /// pointing at the target file. Returns `None` when the cursor isn't
    /// on a `lex.include`, when the URI has no on-disk anchor (untitled
    /// buffers), when the include has no `src=` parameter, when the
    /// path resolves outside the include root, **or when the target
    /// file does not exist on disk**. The last guard avoids navigating
    /// the editor to a non-existent path — the user gets the
    /// `include-not-found` diagnostic from PR 8 instead, which surfaces
    /// the underlying problem clearly. The Location range is the file
    /// head (line 0, column 0) — cross-file goto-def lands the user at
    /// the top of the target.
    async fn goto_for_include(
        &self,
        uri: &Url,
        document: &Document,
        position: AstPosition,
    ) -> Option<Location> {
        let annotation = lex_analysis::utils::find_annotation_at_position(document, position)?;
        if !annotation.is_include() {
            return None;
        }
        let src = annotation.include_src()?;

        let host_path = absolutize_path(&uri.to_file_path().ok()?);
        let cfg = self.config.read().await;
        let inc_root = inc_root_for(&host_path, &cfg.config);
        drop(cfg);

        let target = lex_core::lex::includes::resolve_file_reference(
            &src,
            Some(host_path.as_path()),
            inc_root.as_path(),
        )
        .ok()?;
        // Existence check: don't send the editor to nowhere.
        // `resolve_file_reference` is filesystem-free (lexical only),
        // so the path it returns may not exist. The PR 8 diagnostic
        // already surfaces missing-target errors; goto-def returning
        // None here lets the editor render its native "no definition
        // found" UX instead of opening a phantom buffer.
        if !target.is_file() {
            return None;
        }
        let target_uri = Url::from_file_path(&target).ok()?;
        Some(Location {
            uri: target_uri,
            range: head_range(),
        })
    }

    /// Build a hover preview for a `lex.include` annotation at `position`.
    /// The preview shows the target file's first two non-blank lines
    /// (no AST parsing — just the raw text) — enough to confirm the
    /// include points where the author thinks. Returns `None` when the
    /// cursor isn't on a `lex.include`, the URI has no on-disk anchor,
    /// or the target can't be loaded.
    async fn hover_for_include(
        &self,
        uri: &Url,
        document: &Document,
        position: AstPosition,
    ) -> Option<Hover> {
        let annotation = lex_analysis::utils::find_annotation_at_position(document, position)?;
        if !annotation.is_include() {
            return None;
        }
        let src = annotation.include_src()?;

        let host_path = absolutize_path(&uri.to_file_path().ok()?);
        let cfg = self.config.read().await;
        let inc_root = inc_root_for(&host_path, &cfg.config);
        drop(cfg);

        let target = lex_core::lex::includes::resolve_file_reference(
            &src,
            Some(host_path.as_path()),
            inc_root.as_path(),
        )
        .ok()?;

        let loader = FsLoader::new(inc_root.clone());
        let loaded = lex_core::lex::includes::Loader::load(&loader, target.as_path()).ok()?;
        let preview = include_preview_markdown(&src, &target, &loaded.source);

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: preview,
            }),
            range: Some(to_lsp_range(annotation.header_location())),
        })
    }

    async fn document(&self, uri: &Url) -> Option<Arc<Document>> {
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
        let command = params.command.as_str();
        match command {
            commands::COMMAND_NEXT_ANNOTATION | commands::COMMAND_PREVIOUS_ANNOTATION => {
                let uri_str = params.arguments.first().and_then(|v| v.as_str());
                let pos_val = params.arguments.get(1);

                if let (Some(uri_str), Some(pos_val)) = (uri_str, pos_val) {
                    if let Ok(uri) = Url::parse(uri_str) {
                        if let Ok(position) = serde_json::from_value::<Position>(pos_val.clone()) {
                            if let Some(document) = self.document(&uri).await {
                                let ast_pos = from_lsp_position(position);
                                let navigation = if command == commands::COMMAND_NEXT_ANNOTATION {
                                    lex_analysis::annotations::next_annotation(&document, ast_pos)
                                } else {
                                    lex_analysis::annotations::previous_annotation(
                                        &document, ast_pos,
                                    )
                                };

                                if let Some(result) = navigation {
                                    let location = to_lsp_location(&uri, &result.header);
                                    return Ok(Some(
                                        serde_json::to_value(location)
                                            .map_err(|_| Error::internal_error())?,
                                    ));
                                }
                            }
                        }
                    }
                }
                Ok(None)
            }
            commands::COMMAND_RESOLVE_ANNOTATION | commands::COMMAND_TOGGLE_ANNOTATIONS => {
                let uri_str = params.arguments.first().and_then(|v| v.as_str());
                let pos_val = params.arguments.get(1);

                if let (Some(uri_str), Some(pos_val)) = (uri_str, pos_val) {
                    if let Ok(uri) = Url::parse(uri_str) {
                        if let Ok(position) = serde_json::from_value::<Position>(pos_val.clone()) {
                            if let Some(document) = self.document(&uri).await {
                                let ast_pos = from_lsp_position(position);
                                let _resolved = command == commands::COMMAND_RESOLVE_ANNOTATION;

                                // For toggle, we need to check current status, but lex-analysis toggle takes a boolean "resolved".
                                // Wait, lex-analysis toggle_annotation_resolution takes "resolved: bool".
                                // If we want to toggle, we need to know current state.
                                // But the command name "toggle_annotations" implies switching.
                                // Let's check lex-analysis signature again.
                                // toggle_annotation_resolution(doc, pos, resolved) -> Option<Edit>
                                // It sets status=resolved if resolved=true, removes it if false.
                                // So "resolve" command should pass true.
                                // "toggle" command needs to check if it's currently resolved and flip it.

                                let target_state =
                                    if command == commands::COMMAND_RESOLVE_ANNOTATION {
                                        true
                                    } else {
                                        // Check if currently resolved
                                        if let Some(annotation) =
                                            lex_analysis::utils::find_annotation_at_position(
                                                &document, ast_pos,
                                            )
                                        {
                                            let is_resolved =
                                                annotation.data.parameters.iter().any(|p| {
                                                    p.key == "status" && p.value == "resolved"
                                                });
                                            !is_resolved
                                        } else {
                                            return Ok(None);
                                        }
                                    };

                                if let Some(edit) =
                                    lex_analysis::annotations::toggle_annotation_resolution(
                                        &document,
                                        ast_pos,
                                        target_state,
                                    )
                                {
                                    let text_edit = TextEdit {
                                        range: to_lsp_range(&edit.range),
                                        new_text: edit.new_text,
                                    };
                                    let mut changes = HashMap::new();
                                    changes.insert(uri, vec![text_edit]);
                                    let workspace_edit = tower_lsp::lsp_types::WorkspaceEdit {
                                        changes: Some(changes),
                                        ..Default::default()
                                    };
                                    return Ok(Some(
                                        serde_json::to_value(workspace_edit)
                                            .map_err(|_| Error::internal_error())?,
                                    ));
                                }
                            }
                        }
                    }
                }
                Ok(None)
            }
            commands::COMMAND_INSERT_ASSET => {
                let uri_str = params.arguments.first().and_then(|v| v.as_str());
                let pos_val = params.arguments.get(1);
                let path_val = params.arguments.get(2).and_then(|v| v.as_str());

                if let (Some(uri_str), Some(pos_val), Some(path)) = (uri_str, pos_val, path_val) {
                    if let Ok(uri) = Url::parse(uri_str) {
                        if let Ok(position) = serde_json::from_value::<Position>(pos_val.clone()) {
                            let file_path = PathBuf::from(path);
                            let rules = FormattingRules::default();
                            let entry = self.document_entry(&uri).await;
                            let indent_level = entry
                                .as_ref()
                                .map(|entry| indent_level_from_position(entry, &position, &rules))
                                .unwrap_or(0);
                            let document_directory = document_directory_from_uri(&uri);
                            let snippet = {
                                let request = AssetSnippetRequest {
                                    asset_path: file_path.as_path(),
                                    document_directory: document_directory.as_deref(),
                                    formatting: &rules,
                                    indent_level,
                                };
                                build_asset_snippet(&request)
                            };

                            return Ok(Some(json!({
                                "text": snippet.text,
                                "cursorOffset": snippet.cursor_offset,
                            })));
                        }
                    }
                }
                Ok(None)
            }
            commands::COMMAND_INSERT_VERBATIM => {
                let uri_str = params.arguments.first().and_then(|v| v.as_str());
                let pos_val = params.arguments.get(1);
                let path_val = params.arguments.get(2).and_then(|v| v.as_str());

                if let (Some(uri_str), Some(pos_val), Some(path)) = (uri_str, pos_val, path_val) {
                    if let Ok(uri) = Url::parse(uri_str) {
                        if let Ok(position) = serde_json::from_value::<Position>(pos_val.clone()) {
                            let file_path = PathBuf::from(path);
                            let rules = FormattingRules::default();
                            let entry = self.document_entry(&uri).await;
                            let indent_level = entry
                                .as_ref()
                                .map(|entry| indent_level_from_position(entry, &position, &rules))
                                .unwrap_or(0);
                            let document_directory = document_directory_from_uri(&uri);
                            let snippet_result = {
                                let mut request =
                                    VerbatimSnippetRequest::new(file_path.as_path(), &rules);
                                request.document_directory = document_directory.as_deref();
                                request.indent_level = indent_level;
                                build_verbatim_snippet(&request)
                            };

                            match snippet_result {
                                Ok(snippet) => {
                                    return Ok(Some(json!({
                                        "text": snippet.text,
                                        "cursorOffset": snippet.cursor_offset,
                                    })));
                                }
                                Err(err) => {
                                    return Err(Error::invalid_params(format!(
                                        "Failed to insert verbatim block: {err}"
                                    )));
                                }
                            }
                        }
                    }
                }
                Ok(None)
            }
            commands::COMMAND_EXTRACT_TO_INCLUDE => {
                self.handle_extract_to_include(&params.arguments).await
            }
            _ => execute_command(&params.command, &params.arguments),
        }
    }
}

impl<C> LexLanguageServer<C>
where
    C: LspClient,
{
    /// Handler for `lex.extractToInclude`. Args are **positional**:
    /// `[uri: string, range: lsp.Range, src: string]`.
    ///
    /// Returns a `WorkspaceEdit` JSON value the editor applies atomically
    /// (file creation + selection replacement). All validation failures
    /// surface as `invalid_params` errors carrying the
    /// [`ExtractError::message`] string for direct display.
    async fn handle_extract_to_include(&self, arguments: &[Value]) -> Result<Option<Value>> {
        let uri_str = arguments
            .first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::invalid_params("Missing 'uri' argument"))?;
        let range_val = arguments
            .get(1)
            .ok_or_else(|| Error::invalid_params("Missing 'range' argument"))?;
        let src = arguments
            .get(2)
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::invalid_params("Missing 'src' argument"))?;

        let uri = Url::parse(uri_str)
            .map_err(|_| Error::invalid_params(format!("Invalid uri: {uri_str}")))?;
        let range: Range = serde_json::from_value(range_val.clone())
            .map_err(|e| Error::invalid_params(format!("Invalid range: {e}")))?;

        let host_path = uri
            .to_file_path()
            .map_err(|_| Error::invalid_params(ExtractError::InvalidHostUri.message()))?;
        let host_path = absolutize_path(&host_path);

        let entry = self
            .document_entry(&uri)
            .await
            .ok_or_else(|| Error::invalid_params("Document not open in the server"))?;

        let selection_text = slice_text_by_range(&entry.text, range)
            .ok_or_else(|| Error::invalid_params("Selection range out of bounds"))?;

        let host_indent = range.start.character as usize;

        let cfg = self.config.read().await;
        let inc_root = inc_root_for(&host_path, &cfg.config);
        drop(cfg);

        let edit = extract::build_extract_workspace_edit(
            &uri,
            &host_path,
            range,
            &selection_text,
            host_indent,
            src,
            &inc_root,
        )
        .map_err(|e| Error::invalid_params(e.message()))?;

        Ok(Some(
            serde_json::to_value(edit).map_err(|_| Error::internal_error())?,
        ))
    }

    /// Handler for the custom `lex/preparePaste` request (smart paste,
    /// comms#73). The editor sends the document identity, the range the paste
    /// replaces, and the raw clipboard text; the server reuses the
    /// already-parsed buffer state to classify the paste and re-anchor the
    /// clipboard to the caret's structural context, returning the text to
    /// splice across the range plus the [`PasteMode`] it applied.
    ///
    /// Pure with respect to document state: it reads the parse, computes a
    /// string, and mutates nothing. When the document is not open in the
    /// server (no parse to consult), the response echoes the clipboard back in
    /// `re-anchor` mode — a safe no-op so the editor still completes the paste.
    pub async fn prepare_paste(&self, params: PreparePasteParams) -> Result<PreparePasteResult> {
        let Some(entry) = self.document_entry(&params.text_document.uri).await else {
            // No parsed buffer to consult — hand the clipboard back unchanged
            // rather than fail; the editor applies it as an ordinary paste.
            return Ok(PreparePasteResult {
                text: params.pasted_text,
                mode: PasteMode::Reanchor,
            });
        };

        Ok(prepare_paste_transform(
            &entry.document,
            &entry.text,
            params.range,
            &params.pasted_text,
        ))
    }
}
