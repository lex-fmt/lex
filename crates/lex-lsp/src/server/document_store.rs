//! Document storage: parsed-buffer cache keyed by URI.
//!
//! [`DocumentStore`] holds the most recent permissive parse of every open
//! buffer. Every LSP feature reads its [`Document`] + source text from
//! here. [`DocumentEntry`] bundles the parsed tree with the exact source
//! it was parsed from so position↔text conversions stay consistent.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use lex_core::lex::ast::Document;
use lex_core::lex::parsing;
use tokio::sync::RwLock;
use tower_lsp::lsp_types::Url;

#[derive(Clone)]
pub(crate) struct DocumentEntry {
    pub(crate) document: Arc<Document>,
    pub(crate) text: Arc<String>,
}

#[derive(Default)]
pub(crate) struct DocumentStore {
    pub(crate) entries: RwLock<HashMap<Url, Option<DocumentEntry>>>,
}

impl DocumentStore {
    pub(crate) async fn upsert(&self, uri: Url, text: String) -> Option<DocumentEntry> {
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

    pub(crate) async fn get(&self, uri: &Url) -> Option<DocumentEntry> {
        self.entries.read().await.get(uri).cloned().flatten()
    }

    pub(crate) async fn remove(&self, uri: &Url) {
        self.entries.write().await.remove(uri);
    }
}

pub(crate) fn document_directory_from_uri(uri: &Url) -> Option<PathBuf> {
    uri.to_file_path()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
}
