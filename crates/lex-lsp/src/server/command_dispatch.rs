//! `workspace/executeCommand` + custom-request dispatch.
//!
//! The `LanguageServer::execute_command` trait method in `server.rs` is a thin
//! delegate to [`LexLanguageServer::execute_command_dispatch`] here; this module
//! holds the per-command argument parsing and the `lex.extractToInclude` /
//! `lex/preparePaste` handlers. Per-command *behavior* lives in
//! `lex-analysis` / `lex-babel` / `lex-lsp-core`; this is the wiring.

use std::collections::HashMap;
use std::path::PathBuf;

use lex_babel::formats::lex::formatting_rules::FormattingRules;
use lex_babel::templates::{
    build_asset_snippet, build_verbatim_snippet, AssetSnippetRequest, VerbatimSnippetRequest,
};
use lex_lsp_core::prepare_paste::{
    prepare_paste as prepare_paste_transform, PasteMode, PreparePasteParams, PreparePasteResult,
};
use serde_json::{json, Value};
use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::{ExecuteCommandParams, Position, Range, TextEdit, Url};

use super::{
    absolutize_path, document_directory_from_uri, from_lsp_position, inc_root_for,
    indent_level_from_position, slice_text_by_range, to_lsp_location, to_lsp_range,
    LexLanguageServer, LspClient,
};
use crate::features::commands::{self, execute_command};
use crate::features::extract::{self, ExtractError};

impl<C> LexLanguageServer<C>
where
    C: LspClient,
{
    /// Dispatch a `workspace/executeCommand` request. Commands with bespoke
    /// server-side handling are matched here; everything else falls through to
    /// the stateless [`execute_command`](crate::features::commands::execute_command).
    pub(crate) async fn execute_command_dispatch(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<Value>> {
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

                                // `toggle_annotation_resolution(doc, pos, resolved)` is a
                                // setter: `resolved=true` stamps `status=resolved`, `false`
                                // clears it. So the "resolve" command always passes `true`,
                                // while "toggle" must read the current state and flip it.
                                let target_state =
                                    if command == commands::COMMAND_RESOLVE_ANNOTATION {
                                        true
                                    } else if let Some(annotation) =
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
