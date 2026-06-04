//! Main language server implementation

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::extension_dispatch::{
    dispatch_code_action as ext_dispatch_code_action,
    dispatch_completion as ext_dispatch_completion, dispatch_hover as ext_dispatch_hover,
    LspExtensionState,
};
use crate::features::commands::{self, execute_command};
use crate::features::document_links::collect_document_links;
use crate::features::document_symbols::{collect_document_symbols, LexDocumentSymbol};
use crate::features::extract::{self, ExtractError};
use crate::features::folding_ranges::{folding_ranges as collect_folding_ranges, LexFoldingRange};
use crate::features::formatting::{self, LineRange as FormattingLineRange, TextEditSpan};
use crate::features::go_to_definition::goto_definition;
use crate::features::hover::{hover as compute_hover, HoverResult};
use crate::features::references::find_references;
use crate::features::semantic_tokens::{
    collect_semantic_tokens, LexSemanticToken, SEMANTIC_TOKEN_KINDS,
};
use clapfig::{Boundary, Clapfig, SearchPath};
use lex_analysis::completion::{completion_items, CompletionCandidate, CompletionWorkspace};
use lex_analysis::diagnostics::{
    analyze as analyze_diagnostics, apply_rules, AnalysisDiagnostic, DiagnosticKind,
};
use lex_babel::formats::lex::formatting_rules::FormattingRules;
use lex_babel::templates::{
    build_asset_snippet, build_verbatim_snippet, AssetSnippetRequest, VerbatimSnippetRequest,
};
use lex_config::{
    collect_extension_diagnostic_rules, LabelsConfig, LexConfig, LoadedLexConfig, CONFIG_FILE_NAME,
    DIAGNOSTICS_RULES_PATH,
};
use lex_core::lex::ast::links::{DocumentLink as AstDocumentLink, LinkType};
use lex_core::lex::ast::range::SourceLocation;
use lex_core::lex::ast::{Document, Position as AstPosition, Range as AstRange};
use lex_core::lex::builtins as lex_builtins;
use lex_core::lex::includes::{resolve_from_source, FsLoader, IncludeError, ResolveConfig};
use lex_core::lex::parsing;
use lex_extension_host::registry::Registry;
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
    FoldingRangeParams, FoldingRangeProviderCapability, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, Location, MarkupContent, MarkupKind,
    OneOf, Position, Range, ReferenceParams, SemanticToken, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensResult, ServerCapabilities, ServerInfo, TextDocumentItem,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url, WorkDoneProgressOptions,
    WorkspaceFoldersServerCapabilities,
};
use tower_lsp::Client;

use tower_lsp::lsp_types::Diagnostic;

use tower_lsp::lsp_types::MessageType;

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

pub trait FeatureProvider: Send + Sync + 'static {
    fn semantic_tokens(&self, document: &Document) -> Vec<LexSemanticToken>;
    fn document_symbols(&self, document: &Document) -> Vec<LexDocumentSymbol>;
    fn folding_ranges(&self, document: &Document) -> Vec<LexFoldingRange>;
    fn hover(&self, document: &Document, position: AstPosition) -> Option<HoverResult>;
    fn goto_definition(&self, document: &Document, position: AstPosition) -> Vec<AstRange>;
    fn references(
        &self,
        document: &Document,
        position: AstPosition,
        include_declaration: bool,
    ) -> Vec<AstRange>;
    fn document_links(&self, document: &Document) -> Vec<AstDocumentLink>;
    fn format_document(
        &self,
        document: &Document,
        source: &str,
        rules: Option<FormattingRules>,
    ) -> Vec<TextEditSpan>;
    fn format_range(
        &self,
        document: &Document,
        source: &str,
        range: FormattingLineRange,
        rules: Option<FormattingRules>,
    ) -> Vec<TextEditSpan>;
    fn completion(
        &self,
        document: &Document,
        position: AstPosition,
        current_line: Option<&str>,
        workspace: Option<&CompletionWorkspace>,
        trigger_char: Option<&str>,
    ) -> Vec<CompletionCandidate>;
    fn execute_command(&self, command: &str, arguments: &[Value]) -> Result<Option<Value>>;
}

#[derive(Default)]
pub struct DefaultFeatureProvider;

impl DefaultFeatureProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl FeatureProvider for DefaultFeatureProvider {
    fn semantic_tokens(&self, document: &Document) -> Vec<LexSemanticToken> {
        collect_semantic_tokens(document)
    }

    fn document_symbols(&self, document: &Document) -> Vec<LexDocumentSymbol> {
        collect_document_symbols(document)
    }

    fn folding_ranges(&self, document: &Document) -> Vec<LexFoldingRange> {
        collect_folding_ranges(document)
    }

    fn hover(&self, document: &Document, position: AstPosition) -> Option<HoverResult> {
        compute_hover(document, position)
    }

    fn goto_definition(&self, document: &Document, position: AstPosition) -> Vec<AstRange> {
        goto_definition(document, position)
    }

    fn references(
        &self,
        document: &Document,
        position: AstPosition,
        include_declaration: bool,
    ) -> Vec<AstRange> {
        find_references(document, position, include_declaration)
    }

    fn document_links(&self, document: &Document) -> Vec<AstDocumentLink> {
        collect_document_links(document)
    }

    fn format_document(
        &self,
        document: &Document,
        source: &str,
        rules: Option<FormattingRules>,
    ) -> Vec<TextEditSpan> {
        formatting::format_document(document, source, rules)
    }

    fn format_range(
        &self,
        document: &Document,
        source: &str,
        range: FormattingLineRange,
        rules: Option<FormattingRules>,
    ) -> Vec<TextEditSpan> {
        formatting::format_range(document, source, range, rules)
    }

    fn completion(
        &self,
        document: &Document,
        position: AstPosition,
        current_line: Option<&str>,
        workspace: Option<&CompletionWorkspace>,
        trigger_char: Option<&str>,
    ) -> Vec<CompletionCandidate> {
        completion_items(document, position, current_line, workspace, trigger_char)
    }

    fn execute_command(&self, command: &str, arguments: &[Value]) -> Result<Option<Value>> {
        execute_command(command, arguments)
    }
}

#[derive(Clone)]
struct DocumentEntry {
    document: Arc<Document>,
    text: Arc<String>,
}

#[derive(Default)]
struct DocumentStore {
    entries: RwLock<HashMap<Url, Option<DocumentEntry>>>,
}

impl DocumentStore {
    async fn upsert(&self, uri: Url, text: String) -> Option<DocumentEntry> {
        // Permissive parse: `doc.*` and unknown `lex.*` labels — which
        // strict-mode `NormalizeLabels` rejects as parse errors — flow
        // through into the AST instead so the analysis stage can
        // surface them as in-place diagnostics. PR 4 of #584 wires up
        // the diagnostic surface; without permissive parse here, a
        // single forbidden label would blank out every LSP feature
        // for the document.
        let parsed = match parsing::parse_document_permissive(&text) {
            Ok(document) => Some(DocumentEntry {
                document: Arc::new(document),
                text: Arc::new(text),
            }),
            Err(_) => None,
        };
        self.entries.write().await.insert(uri, parsed.clone());
        parsed
    }

    async fn get(&self, uri: &Url) -> Option<DocumentEntry> {
        self.entries.read().await.get(uri).cloned().flatten()
    }

    async fn remove(&self, uri: &Url) {
        self.entries.write().await.remove(uri);
    }
}

fn document_directory_from_uri(uri: &Url) -> Option<PathBuf> {
    uri.to_file_path()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
}

fn indent_level_from_position(
    entry: &DocumentEntry,
    position: &Position,
    rules: &FormattingRules,
) -> usize {
    let indent_unit = rules.indent_string.as_str();
    if indent_unit.is_empty() {
        return 0;
    }
    let indent_len = indent_unit.len();
    let line = entry.text.lines().nth(position.line as usize).unwrap_or("");
    let prefix: String = line.chars().take(position.character as usize).collect();
    let mut level = 0;
    let mut remainder = prefix.as_str();
    while remainder.starts_with(indent_unit) {
        level += 1;
        remainder = &remainder[indent_len..];
    }
    level
}

fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: SEMANTIC_TOKEN_KINDS
            .iter()
            .map(|kind| SemanticTokenType::new(kind.as_str()))
            .collect(),
        token_modifiers: Vec::new(),
    }
}

pub struct LexLanguageServer<C = Client, P = DefaultFeatureProvider> {
    client: C,
    documents: DocumentStore,
    features: Arc<P>,
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

impl LexLanguageServer<Client, DefaultFeatureProvider> {
    pub fn new(client: Client) -> Self {
        Self::with_features(client, Arc::new(DefaultFeatureProvider::new()))
    }
}

impl<C, P> LexLanguageServer<C, P>
where
    C: LspClient,
    P: FeatureProvider,
{
    pub fn with_features(client: C, features: Arc<P>) -> Self {
        let config = load_config(None);
        Self {
            client,
            documents: DocumentStore::default(),
            features,
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
            root: inc_root.clone(),
            max_depth,
            max_total_includes,
        };
        let loader = FsLoader::new(inc_root).with_max_file_size(max_file_size);
        let registry = Registry::new();
        if let Err(e) = lex_builtins::register_into(
            &registry,
            std::sync::Arc::new(loader),
            resolve_config.clone(),
        ) {
            return vec![registry_setup_diagnostic(&e.to_string())];
        }

        match resolve_from_source(text, Some(path), &resolve_config, &registry) {
            Ok(_doc) => {
                // Resolution succeeded. We *don't* store the merged
                // tree — see fn-level docstring. The resolver was run
                // only to surface errors; the tree itself is dropped.
                Vec::new()
            }
            Err(err) => vec![include_error_to_diagnostic(&err)],
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

/// Load a [`LoadedLexConfig`] via clapfig, searching from an optional
/// workspace root. The wrapper carries both the typed [`LexConfig`] and
/// the side-channel map of extension-emitted diagnostic rules captured
/// from `[diagnostics.rules]` via the `on_unknown_key` callback.
fn load_config(workspace_root: Option<&Path>) -> LoadedLexConfig {
    let mut search_paths = vec![SearchPath::Platform];
    if let Some(root) = workspace_root {
        search_paths.push(SearchPath::Path(root.to_path_buf()));
    } else {
        search_paths.push(SearchPath::Ancestors(Boundary::Marker(".git")));
        search_paths.push(SearchPath::Cwd);
    }
    load_with(search_paths, false).unwrap_or_else(|_| {
        // Fall back to compiled defaults if config loading fails.
        load_with(vec![], true).expect("compiled defaults must load")
    })
}

fn load_with(
    search_paths: Vec<SearchPath>,
    no_env: bool,
) -> std::result::Result<LoadedLexConfig, clapfig::ClapfigError> {
    let mut builder = Clapfig::schema_builder::<LexConfig>()
        .app_name("lex")
        .file_name(CONFIG_FILE_NAME)
        .search_paths(search_paths)
        .accept_dotted_extension_keys_in(
            DIAGNOSTICS_RULES_PATH,
            clapfig::UnknownKeyDecision::Collect,
        );
    if no_env {
        builder = builder.no_env();
    }
    let (config, unknowns) = builder.load_with_unknowns()?;
    Ok(LoadedLexConfig {
        config,
        extension_diagnostic_rules: collect_extension_diagnostic_rules(unknowns),
    })
}

fn best_matching_root(roots: &[PathBuf], document_path: &Path) -> Option<PathBuf> {
    roots
        .iter()
        .filter(|root| document_path.starts_with(root))
        .max_by_key(|root| root.components().count())
        .cloned()
}

fn to_lsp_position(position: &AstPosition) -> Position {
    Position::new(position.line as u32, position.column as u32)
}

fn to_lsp_range(range: &AstRange) -> Range {
    Range {
        start: to_lsp_position(&range.start),
        end: to_lsp_position(&range.end),
    }
}

fn to_lsp_location(uri: &Url, range: &AstRange) -> Location {
    Location {
        uri: uri.clone(),
        range: to_lsp_range(range),
    }
}

fn spans_to_text_edits(text: &str, spans: Vec<TextEditSpan>) -> Vec<TextEdit> {
    if spans.is_empty() {
        return Vec::new();
    }
    let locator = SourceLocation::new(text);
    spans
        .into_iter()
        .map(|span| TextEdit {
            range: Range {
                start: to_lsp_position(&locator.byte_to_position(span.start)),
                end: to_lsp_position(&locator.byte_to_position(span.end)),
            },
            new_text: span.new_text,
        })
        .collect()
}

fn to_formatting_line_range(range: &Range) -> FormattingLineRange {
    let start = range.start.line as usize;
    let mut end = range.end.line as usize;
    if range.end.character > 0 || end == start {
        end += 1;
    }
    FormattingLineRange { start, end }
}

use lsp_types::{FormattingOptions, FormattingProperty};

/// Apply per-request LSP overrides onto existing formatting rules.
///
/// Clients can pass custom Lex formatting options through the `properties` field
/// of FormattingOptions. Supported keys (all under "lex." prefix):
/// - lex.session_blank_lines_before
/// - lex.session_blank_lines_after
/// - lex.normalize_seq_markers
/// - lex.unordered_seq_marker
/// - lex.max_blank_lines
/// - lex.indent_string
/// - lex.preserve_trailing_blanks
/// - lex.normalize_verbatim_markers
fn apply_formatting_overrides(rules: &mut FormattingRules, options: &FormattingOptions) {
    for (key, value) in &options.properties {
        match key.as_str() {
            "lex.session_blank_lines_before" => {
                if let FormattingProperty::Number(n) = value {
                    rules.session_blank_lines_before = (*n).max(0) as usize;
                }
            }
            "lex.session_blank_lines_after" => {
                if let FormattingProperty::Number(n) = value {
                    rules.session_blank_lines_after = (*n).max(0) as usize;
                }
            }
            "lex.normalize_seq_markers" => {
                if let FormattingProperty::Bool(b) = value {
                    rules.normalize_seq_markers = *b;
                }
            }
            "lex.unordered_seq_marker" => {
                if let FormattingProperty::String(s) = value {
                    if let Some(c) = s.chars().next() {
                        rules.unordered_seq_marker = c;
                    }
                }
            }
            "lex.max_blank_lines" => {
                if let FormattingProperty::Number(n) = value {
                    rules.max_blank_lines = (*n).max(0) as usize;
                }
            }
            "lex.indent_string" => {
                if let FormattingProperty::String(s) = value {
                    rules.indent_string = s.clone();
                }
            }
            "lex.preserve_trailing_blanks" => {
                if let FormattingProperty::Bool(b) = value {
                    rules.preserve_trailing_blanks = *b;
                }
            }
            "lex.normalize_verbatim_markers" => {
                if let FormattingProperty::Bool(b) = value {
                    rules.normalize_verbatim_markers = *b;
                }
            }
            _ => {}
        }
    }
}

fn from_lsp_position(position: Position) -> AstPosition {
    AstPosition::new(position.line as usize, position.character as usize)
}

fn encode_semantic_tokens(tokens: &[LexSemanticToken], text: &str) -> Vec<SemanticToken> {
    let line_offsets = compute_line_offsets(text);
    let mut data = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for token in tokens {
        let token_type_index = SEMANTIC_TOKEN_KINDS
            .iter()
            .position(|kind| *kind == token.kind)
            .unwrap_or(0) as u32;
        for (line, start, length) in split_token_on_lines(token, text, &line_offsets) {
            if length == 0 {
                continue;
            }
            let delta_line = line.saturating_sub(prev_line);
            let delta_start = if delta_line == 0 {
                start.saturating_sub(prev_start)
            } else {
                start
            };
            data.push(SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type: token_type_index,
                token_modifiers_bitset: 0,
            });
            prev_line = line;
            prev_start = start;
        }
    }

    data
}

fn compute_line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            offsets.push(idx + ch.len_utf8());
        }
    }
    offsets
}

/// Expand a semantic token range into single-line segments.
///
/// The LSP wire format encodes tokens as delta positions relative to the previous token
/// and disallows spanning multiple lines, so every multi-line range must be broken into
/// per-line slices before encoding.
fn split_token_on_lines(
    token: &LexSemanticToken,
    text: &str,
    line_offsets: &[usize],
) -> Vec<(u32, u32, u32)> {
    let span = &token.range.span;
    if span.start > text.len() || span.end > text.len() {
        // Defensive: skip tokens whose byte span exceeds the source text.
        // This can happen when the parser produces out-of-bounds ranges.
        return Vec::new();
    }
    let slice = &text[span.clone()];
    let mut segments = Vec::new();
    let mut current_line = token.range.start.line as u32;
    let mut segment_start = 0;
    let base_offset = token.range.span.start;

    for (idx, ch) in slice.char_indices() {
        if ch == '\n' {
            if idx > segment_start {
                let length = (idx - segment_start) as u32;
                let absolute_start = base_offset + segment_start;
                let line_offset = line_offsets
                    .get(current_line as usize)
                    .copied()
                    .unwrap_or(0);
                let start_col = (absolute_start.saturating_sub(line_offset)) as u32;
                segments.push((current_line, start_col, length));
            }
            current_line += 1;
            segment_start = idx + ch.len_utf8();
        }
    }

    if slice.len() > segment_start {
        let length = (slice.len() - segment_start) as u32;
        let absolute_start = base_offset + segment_start;
        let line_offset = line_offsets
            .get(current_line as usize)
            .copied()
            .unwrap_or(0);
        let start_col = (absolute_start.saturating_sub(line_offset)) as u32;
        segments.push((current_line, start_col, length));
    }

    segments
}

#[allow(deprecated)]
fn to_document_symbol(symbol: &LexDocumentSymbol) -> DocumentSymbol {
    DocumentSymbol {
        name: symbol.name.clone(),
        detail: symbol.detail.clone(),
        kind: symbol.kind,
        deprecated: None,
        range: to_lsp_range(&symbol.range),
        selection_range: to_lsp_range(&symbol.selection_range),
        children: if symbol.children.is_empty() {
            None
        } else {
            Some(symbol.children.iter().map(to_document_symbol).collect())
        },
        tags: None,
    }
}

fn to_lsp_folding_range(range: &LexFoldingRange) -> FoldingRange {
    FoldingRange {
        start_line: range.start_line,
        start_character: range.start_character,
        end_line: range.end_line,
        end_character: range.end_character,
        kind: range.kind.clone(),
        collapsed_text: None,
    }
}

fn to_lsp_completion_item(candidate: &CompletionCandidate) -> CompletionItem {
    CompletionItem {
        label: candidate.label.clone(),
        kind: Some(candidate.kind),
        detail: candidate.detail.clone(),
        insert_text: candidate.insert_text.clone(),
        ..Default::default()
    }
}

fn build_document_link(uri: &Url, link: &AstDocumentLink) -> Option<DocumentLink> {
    let target = link_target_uri(uri, link)?;
    Some(DocumentLink {
        range: to_lsp_range(&link.range),
        target: Some(target),
        tooltip: None,
        data: None,
    })
}

fn link_target_uri(document_uri: &Url, link: &AstDocumentLink) -> Option<Url> {
    match link.link_type {
        LinkType::Url => Url::parse(&link.target).ok(),
        LinkType::File | LinkType::VerbatimSrc => {
            resolve_file_like_target(document_uri, &link.target)
        }
    }
}

fn resolve_file_like_target(document_uri: &Url, target: &str) -> Option<Url> {
    if target.is_empty() {
        return None;
    }
    let path = Path::new(target);
    if path.is_absolute() {
        return Url::from_file_path(path).ok();
    }
    if document_uri.scheme() == "file" {
        let mut base = document_uri.to_file_path().ok()?;
        base.pop();
        base.push(target);
        Url::from_file_path(base).ok()
    } else {
        parent_directory_uri(document_uri).join(target).ok()
    }
}

fn parent_directory_uri(uri: &Url) -> Url {
    let mut base = uri.clone();
    let mut path = base.path().to_string();
    if let Some(idx) = path.rfind('/') {
        path.truncate(idx + 1);
    } else {
        path.push('/');
    }
    base.set_path(&path);
    base.set_query(None);
    base.set_fragment(None);
    base
}

#[async_trait]
impl<C, P> tower_lsp::LanguageServer for LexLanguageServer<C, P>
where
    C: LspClient,
    P: FeatureProvider,
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
            let tokens = self.features.semantic_tokens(&document);
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
            let symbols = self.features.document_symbols(&document);
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

            if let Some(result) = self.features.hover(&document, position) {
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
            let ranges = self.features.folding_ranges(&document);
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

            let ranges = self.features.goto_definition(&document, position);
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
            let ranges = self
                .features
                .references(&document, position, include_declaration);
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
            let links = self.features.document_links(&document);
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
            let edits = self
                .features
                .format_document(&document, text.as_str(), Some(rules));
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
            let edits =
                self.features
                    .format_range(&document, text.as_str(), line_range, Some(rules));
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

            let candidates = self.features.completion(
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
            _ => self
                .features
                .execute_command(&params.command, &params.arguments),
        }
    }
}

impl<C, P> LexLanguageServer<C, P>
where
    C: LspClient,
    P: FeatureProvider,
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

/// Slice `text` by an LSP `Range`. Returns `None` when the range falls
/// outside the document or splits a multi-byte character.
///
/// `character` is treated as a **UTF-8 byte offset** to match the rest
/// of the server: lex-core's `SourceLocation::byte_to_position`
/// computes `column = byte_offset - line_start`, and `to_lsp_position`
/// forwards that value to LSP as-is. Using char offsets here would
/// mis-slice any selection containing multi-byte characters. See the
/// crate-level "Position Encoding" docs for the full convention.
fn slice_text_by_range(text: &str, range: Range) -> Option<String> {
    let start_line = range.start.line as usize;
    let end_line = range.end.line as usize;
    let start_col = range.start.character as usize;
    let end_col = range.end.character as usize;
    if start_line > end_line || (start_line == end_line && start_col > end_col) {
        return None;
    }

    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if end_line >= lines.len() && !(end_line == lines.len() && end_col == 0) {
        return None;
    }

    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i < start_line || i > end_line {
            continue;
        }
        let line_bytes = line.as_bytes();
        let from = if i == start_line { start_col } else { 0 };
        let to = if i == end_line {
            end_col
        } else {
            line_bytes.len()
        };
        if from > line_bytes.len() || to > line_bytes.len() {
            return None;
        }
        // Reject ranges that cut a UTF-8 character in half rather than
        // returning a string with replacement characters.
        if !line.is_char_boundary(from) || !line.is_char_boundary(to) {
            return None;
        }
        out.push_str(&line[from..to]);
    }
    Some(out)
}

/// Compute the include-resolution root for an entry document.
///
/// Order:
/// 1. `[includes].root` from `LexConfig` if set.
/// 2. Directory of the nearest `.lex.toml` walking upward from the
///    entry document's directory.
/// 3. The entry document's own directory.
///
/// Always returns an absolute, lexically-normalized path so the
/// resolver's root-escape prefix check is sound.
fn inc_root_for(entry_path: &Path, cfg: &LexConfig) -> PathBuf {
    let raw = if let Some(root) = cfg.includes.root.as_ref() {
        PathBuf::from(root)
    } else {
        let start = entry_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        find_nearest_config_dir(&start).unwrap_or(start)
    };
    absolutize_path(&raw)
}

/// Walk upward from `start` looking for a directory that contains
/// `.lex.toml`. Returns that directory, or `None` if we hit the
/// filesystem root without finding one.
fn find_nearest_config_dir(start: &Path) -> Option<PathBuf> {
    let mut cur: PathBuf = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    loop {
        if cur.join(CONFIG_FILE_NAME).is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Best-effort absolutize: try `Path::canonicalize` first (handles
/// symlinks + resolves `..` against the real filesystem), falling back
/// to `current_dir().join(path)` if the path doesn't exist on disk.
/// Always returns an absolute path; `ResolveConfig::root` requires one
/// for the root-escape prefix check to be sound.
fn absolutize_path(p: &Path) -> PathBuf {
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

/// Map an [`IncludeError`] to an LSP [`Diagnostic`].
///
/// The diagnostic's range points at the offending `lex.include`
/// annotation when the error carries one (Cycle, DepthExceeded,
/// NotFound, ContainerPolicy, MissingSrc, TotalIncludesExceeded,
/// FileTooLarge); otherwise it falls back to the document head
/// (line 0, column 0) so the user at least sees something in the
/// editor's diagnostics panel.
fn include_error_to_diagnostic(err: &IncludeError) -> Diagnostic {
    let (range, code, message) = match err {
        IncludeError::Cycle { include_site, .. } => {
            (to_lsp_range(include_site), "include-cycle", err.to_string())
        }
        IncludeError::DepthExceeded { include_site, .. } => (
            to_lsp_range(include_site),
            "include-depth-exceeded",
            err.to_string(),
        ),
        IncludeError::RootEscape { .. } => (head_range(), "include-root-escape", err.to_string()),
        IncludeError::AbsolutePath { .. } => {
            (head_range(), "include-absolute-path", err.to_string())
        }
        IncludeError::NotFound { include_site, .. } => (
            to_lsp_range(include_site),
            "include-not-found",
            err.to_string(),
        ),
        IncludeError::ParseFailed { .. } => (head_range(), "include-parse-failed", err.to_string()),
        IncludeError::ContainerPolicy { include_site, .. } => (
            to_lsp_range(include_site),
            "include-container-policy",
            err.to_string(),
        ),
        IncludeError::LoaderIo { .. } => (head_range(), "include-loader-io", err.to_string()),
        IncludeError::MissingSrc { include_site } => (
            to_lsp_range(include_site),
            "include-missing-src",
            err.to_string(),
        ),
        IncludeError::TotalIncludesExceeded { include_site, .. } => (
            to_lsp_range(include_site),
            "include-total-exceeded",
            err.to_string(),
        ),
        IncludeError::FileTooLarge { include_site, .. } => (
            to_lsp_range(include_site),
            "include-file-too-large",
            err.to_string(),
        ),
        IncludeError::HandlerFailed { include_site, .. } => (
            to_lsp_range(include_site),
            "include-handler-failed",
            err.to_string(),
        ),
    };
    Diagnostic {
        range,
        severity: Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR),
        code: Some(tower_lsp::lsp_types::NumberOrString::String(
            code.to_string(),
        )),
        code_description: None,
        source: Some("lex".to_string()),
        message,
        related_information: None,
        tags: None,
        data: None,
    }
}

/// Synthesize a document-head diagnostic when registry registration
/// fails (e.g., another path of the LSP already registered the `lex`
/// namespace and we collided). This should never happen in practice
/// — we build a fresh `Registry` per resolve call — but the path is
/// here so a future regression surfaces an editor diagnostic rather
/// than a silent panic.
fn registry_setup_diagnostic(message: &str) -> Diagnostic {
    Diagnostic {
        range: head_range(),
        severity: Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR),
        code: Some(tower_lsp::lsp_types::NumberOrString::String(
            "include-registry-setup".to_string(),
        )),
        code_description: None,
        source: Some("lex".to_string()),
        message: format!("could not configure include resolver: {message}"),
        related_information: None,
        tags: None,
        data: None,
    }
}

fn head_range() -> Range {
    Range {
        start: Position::new(0, 0),
        end: Position::new(0, 0),
    }
}

/// Build the markdown body for an include hover. Shows the source path
/// from the annotation, the resolved on-disk path, and a small content
/// preview consisting of the first two non-blank lines of the target
/// (no AST parsing — just raw text). Designed to fit in a hover popup,
/// not to replace opening the file.
///
/// Uses a four-backtick code fence so a triple-backtick that happens to
/// appear in a previewed line (e.g., a markdown verbatim block) does
/// not terminate the fence early and corrupt the rendered hover.
fn include_preview_markdown(src: &str, target: &Path, target_source: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("**`lex.include`** → `{src}`\n\n"));
    out.push_str(&format!("Resolved: `{}`\n\n", target.display()));

    let preview_lines: Vec<&str> = target_source
        .lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.is_empty())
        .take(2)
        .collect();
    if preview_lines.is_empty() {
        out.push_str("_(empty file)_");
    } else {
        out.push_str("````lex\n");
        for line in &preview_lines {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("````");
    }
    out
}

fn to_lsp_diagnostic(diag: AnalysisDiagnostic) -> Diagnostic {
    use lex_analysis::diagnostics::DiagnosticSeverity as AS;
    let severity = match diag.severity {
        AS::Error => tower_lsp::lsp_types::DiagnosticSeverity::ERROR,
        AS::Warning => tower_lsp::lsp_types::DiagnosticSeverity::WARNING,
        AS::Info => tower_lsp::lsp_types::DiagnosticSeverity::INFORMATION,
        AS::Hint => tower_lsp::lsp_types::DiagnosticSeverity::HINT,
    };

    let code = diag.kind.code().into_owned();

    let source = match &diag.kind {
        DiagnosticKind::Handler { namespace, .. } => format!("lex:{namespace}"),
        _ => "lex".to_string(),
    };

    Diagnostic {
        range: to_lsp_range(&diag.range),
        severity: Some(severity),
        code: Some(tower_lsp::lsp_types::NumberOrString::String(code)),
        code_description: None,
        source: Some(source),
        message: diag.message,
        related_information: None,
        tags: None,
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::semantic_tokens::LexSemanticTokenKind;
    use lex_analysis::test_support::sample_source;
    use serde::Deserialize;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tempfile::tempdir;
    use tower_lsp::lsp_types::{
        CompletionItemKind, DidOpenTextDocumentParams, DocumentFormattingParams,
        DocumentLinkParams, DocumentRangeFormattingParams, DocumentSymbolParams, FoldingRangeKind,
        FoldingRangeParams, FormattingOptions, GotoDefinitionParams, HoverParams, Position, Range,
        ReferenceContext, ReferenceParams, SemanticTokensParams, SymbolKind,
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

    #[derive(Default)]
    struct MockFeatureProvider {
        semantic_tokens_called: AtomicUsize,
        document_symbols_called: AtomicUsize,
        hover_called: AtomicUsize,
        folding_called: AtomicUsize,
        last_hover_position: Mutex<Option<AstPosition>>,
        definition_called: AtomicUsize,
        references_called: AtomicUsize,
        document_links_called: AtomicUsize,
        last_references_include: Mutex<Option<bool>>,
        formatting_called: AtomicUsize,
        range_formatting_called: AtomicUsize,
        completion_called: AtomicUsize,
        execute_command_called: AtomicUsize,
    }

    impl FeatureProvider for MockFeatureProvider {
        fn semantic_tokens(&self, _: &Document) -> Vec<LexSemanticToken> {
            self.semantic_tokens_called.fetch_add(1, Ordering::SeqCst);
            vec![LexSemanticToken {
                kind: LexSemanticTokenKind::DocumentTitle,
                range: AstRange::new(0..5, AstPosition::new(0, 0), AstPosition::new(0, 5)),
            }]
        }

        fn document_symbols(&self, _: &Document) -> Vec<LexDocumentSymbol> {
            self.document_symbols_called.fetch_add(1, Ordering::SeqCst);
            vec![LexDocumentSymbol {
                name: "symbol".into(),
                detail: None,
                kind: SymbolKind::FILE,
                range: AstRange::new(0..5, AstPosition::new(0, 0), AstPosition::new(0, 5)),
                selection_range: AstRange::new(
                    0..5,
                    AstPosition::new(0, 0),
                    AstPosition::new(0, 5),
                ),
                children: Vec::new(),
            }]
        }

        fn folding_ranges(&self, _: &Document) -> Vec<LexFoldingRange> {
            self.folding_called.fetch_add(1, Ordering::SeqCst);
            vec![LexFoldingRange {
                start_line: 0,
                start_character: Some(0),
                end_line: 1,
                end_character: Some(0),
                kind: Some(FoldingRangeKind::Region),
            }]
        }

        fn hover(&self, _: &Document, position: AstPosition) -> Option<HoverResult> {
            self.hover_called.fetch_add(1, Ordering::SeqCst);
            *self.last_hover_position.lock().unwrap() = Some(position);
            Some(HoverResult {
                range: AstRange::new(0..5, AstPosition::new(0, 0), AstPosition::new(0, 5)),
                contents: "hover".into(),
            })
        }

        fn goto_definition(&self, _: &Document, _: AstPosition) -> Vec<AstRange> {
            self.definition_called.fetch_add(1, Ordering::SeqCst);
            vec![AstRange::new(
                0..5,
                AstPosition::new(0, 0),
                AstPosition::new(0, 5),
            )]
        }

        fn references(
            &self,
            _: &Document,
            _: AstPosition,
            include_declaration: bool,
        ) -> Vec<AstRange> {
            self.references_called.fetch_add(1, Ordering::SeqCst);
            *self.last_references_include.lock().unwrap() = Some(include_declaration);
            vec![AstRange::new(
                0..5,
                AstPosition::new(0, 0),
                AstPosition::new(0, 5),
            )]
        }

        fn document_links(&self, _: &Document) -> Vec<AstDocumentLink> {
            self.document_links_called.fetch_add(1, Ordering::SeqCst);
            vec![AstDocumentLink::new(
                AstRange::new(0..5, AstPosition::new(0, 0), AstPosition::new(0, 5)),
                "https://example.com".to_string(),
                LinkType::Url,
            )]
        }

        fn format_document(
            &self,
            _: &Document,
            _: &str,
            _: Option<FormattingRules>,
        ) -> Vec<TextEditSpan> {
            self.formatting_called.fetch_add(1, Ordering::SeqCst);
            vec![TextEditSpan {
                start: 0,
                end: 0,
                new_text: "formatted".into(),
            }]
        }

        fn format_range(
            &self,
            _: &Document,
            _: &str,
            _: FormattingLineRange,
            _: Option<FormattingRules>,
        ) -> Vec<TextEditSpan> {
            self.range_formatting_called.fetch_add(1, Ordering::SeqCst);
            vec![TextEditSpan {
                start: 0,
                end: 0,
                new_text: "range".into(),
            }]
        }

        fn completion(
            &self,
            _: &Document,
            _: AstPosition,
            _: Option<&str>,
            _: Option<&CompletionWorkspace>,
            _: Option<&str>,
        ) -> Vec<CompletionCandidate> {
            self.completion_called.fetch_add(1, Ordering::SeqCst);
            vec![CompletionCandidate {
                label: "completion".into(),
                detail: None,
                kind: CompletionItemKind::TEXT,
                insert_text: None,
            }]
        }

        fn execute_command(&self, command: &str, _: &[Value]) -> Result<Option<Value>> {
            self.execute_command_called.fetch_add(1, Ordering::SeqCst);
            if command == "test.command" {
                Ok(Some(Value::String("executed".into())))
            } else {
                Ok(None)
            }
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

    async fn open_sample_document(server: &LexLanguageServer<NoopClient, MockFeatureProvider>) {
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

    #[tokio::test]
    async fn semantic_tokens_call_feature_layer() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
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

        assert_eq!(provider.semantic_tokens_called.load(Ordering::SeqCst), 1);
        let data_len = match result {
            SemanticTokensResult::Tokens(tokens) => tokens.data.len(),
            SemanticTokensResult::Partial(partial) => partial.data.len(),
        };
        assert!(data_len > 0);
    }

    #[tokio::test]
    async fn document_symbols_call_feature_layer() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
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
        assert_eq!(provider.document_symbols_called.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn hover_uses_feature_provider_position() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
        open_sample_document(&server).await;

        let hover = server
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: sample_uri() },
                    position: Position::new(0, 0),
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();

        assert!(matches!(hover.contents, HoverContents::Markup(_)));
        assert_eq!(provider.hover_called.load(Ordering::SeqCst), 1);
        let stored = provider.last_hover_position.lock().unwrap().unwrap();
        assert_eq!(stored.line, 0);
        assert_eq!(stored.column, 0);
    }

    #[tokio::test]
    async fn folding_range_uses_feature_provider() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
        open_sample_document(&server).await;

        let ranges = server
            .folding_range(FoldingRangeParams {
                text_document: TextDocumentIdentifier { uri: sample_uri() },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(provider.folding_called.load(Ordering::SeqCst), 1);
        assert_eq!(ranges.len(), 1);
    }

    #[tokio::test]
    async fn goto_definition_uses_feature_provider() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
        open_sample_document(&server).await;

        let response = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: sample_uri() },
                    position: Position::new(0, 0),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(provider.definition_called.load(Ordering::SeqCst), 1);
        match response {
            GotoDefinitionResponse::Array(locations) => assert_eq!(locations.len(), 1),
            _ => panic!("unexpected goto definition response"),
        }
    }

    #[derive(Deserialize)]
    struct SnippetResponse {
        text: String,
        #[serde(rename = "cursorOffset")]
        cursor_offset: usize,
    }

    #[tokio::test]
    async fn execute_insert_commands() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
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
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
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
    async fn references_use_feature_provider() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
        open_sample_document(&server).await;

        let result = server
            .references(ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: sample_uri() },
                    position: Position::new(0, 0),
                },
                context: ReferenceContext {
                    include_declaration: true,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(provider.references_called.load(Ordering::SeqCst), 1);
        assert_eq!(result.len(), 1);
        assert_eq!(
            *provider.last_references_include.lock().unwrap(),
            Some(true)
        );
    }

    #[tokio::test]
    async fn document_links_use_feature_provider() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
        open_sample_document(&server).await;

        let links = server
            .document_link(DocumentLinkParams {
                text_document: TextDocumentIdentifier { uri: sample_uri() },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(provider.document_links_called.load(Ordering::SeqCst), 1);
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().map(|url| url.as_str()),
            Some("https://example.com/")
        );
    }

    #[tokio::test]
    async fn formatting_uses_feature_provider() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
        open_sample_document(&server).await;

        let edits = server
            .formatting(DocumentFormattingParams {
                text_document: TextDocumentIdentifier { uri: sample_uri() },
                options: FormattingOptions::default(),
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(provider.formatting_called.load(Ordering::SeqCst), 1);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "formatted");
    }

    #[tokio::test]
    async fn range_formatting_uses_feature_provider() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());
        open_sample_document(&server).await;

        let edits = server
            .range_formatting(DocumentRangeFormattingParams {
                text_document: TextDocumentIdentifier { uri: sample_uri() },
                range: Range {
                    start: Position::new(0, 0),
                    end: Position::new(0, 0),
                },
                options: FormattingOptions::default(),
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(provider.range_formatting_called.load(Ordering::SeqCst), 1);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "range");
    }

    #[tokio::test]
    async fn semantic_tokens_returns_none_when_document_missing() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);

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
    async fn execute_command_uses_feature_provider() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider.clone());

        let result = server
            .execute_command(ExecuteCommandParams {
                command: "test.command".into(),
                arguments: vec![],
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(provider.execute_command_called.load(Ordering::SeqCst), 1);
        assert_eq!(result, Value::String("executed".into()));
    }

    #[tokio::test]
    async fn hover_returns_none_without_document_entry() {
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);

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
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);

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
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);

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
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);

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
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);

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
        LexLanguageServer<CapturingClient, DefaultFeatureProvider>,
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
        let server = LexLanguageServer::with_features(
            client.clone(),
            Arc::new(DefaultFeatureProvider::new()),
        );

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
        let expected = Url::from_file_path(absolutize_path(&dir.path().join("chapter.lex")))
            .expect("file uri");
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
        let (server, _client, uri, dir) =
            open_in_tempdir(&[("main.lex", host_text)], "main.lex").await;

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
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);
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

    /// `slice_text_by_range` treats `Range.character` as a UTF-8 byte
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
        let server = LexLanguageServer::with_features(
            client.clone(),
            Arc::new(DefaultFeatureProvider::new()),
        );
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
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);

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
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);
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
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);
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
        let provider = Arc::new(MockFeatureProvider::default());
        let server = LexLanguageServer::with_features(NoopClient, provider);
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
}
