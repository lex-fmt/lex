//! `lex.include` resolution for the language server.
//!
//! Drives include resolution for *diagnostic* purposes (the server always
//! stores the unresolved host-buffer parse — see [`Self::resolve_and_upsert`])
//! and powers goto-definition / hover previews for include sites.

use lex_core::lex::ast::{Document, Position as AstPosition};
use lex_core::lex::builtins as lex_builtins;
use lex_core::lex::includes::{FsLoader, ResolveConfig};
use tower_lsp::lsp_types::{
    Diagnostic, Hover, HoverContents, Location, MarkupContent, MarkupKind, Url,
};

use super::{
    absolutize_path, head_range, inc_root_for, include_error_to_diagnostic,
    include_preview_markdown, registry_setup_diagnostic, to_lsp_range, LexLanguageServer,
    LspClient,
};

impl<C> LexLanguageServer<C>
where
    C: LspClient,
{
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
    pub(crate) async fn resolve_and_upsert(&self, uri: &Url, text: &str) -> Vec<Diagnostic> {
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
    pub(crate) async fn goto_for_include(
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
    pub(crate) async fn hover_for_include(
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
}
