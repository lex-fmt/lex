//! Extract-to-include: server-side machinery for the `lex.extractToInclude`
//! workspace command.
//!
//! Selecting a section of a Lex document and "extracting" it splits the
//! selection out into a new file referenced via `:: lex.include src="..." ::`.
//! All path validation, indent normalization, and a Lex-fragment parse
//! check live here in Rust (called from the LSP server) so the per-editor
//! shims stay thin — see lex#497.
//!
//! Two entry points:
//!
//! * [`validate_include_path`] — pure path/URL/existence validation against
//!   the configured includes root. Surfaces every failure mode as a distinct
//!   [`ExtractError`] variant so the editor displays a clear message.
//!
//! * [`build_extract_workspace_edit`] — runs validation, indent-shifts the
//!   selection so its shallowest non-blank line lands at column 0, parses
//!   the shifted text as a Lex fragment, and constructs an atomic
//!   [`WorkspaceEdit`] containing (1) `CreateFile` for the target and (2)
//!   `TextEdit` replacing the selection with the include annotation.
//!
//! The full `GeneralContainer` host-policy check (e.g. rejecting a Session
//! selected for extraction into a Definition body) is deferred — the
//! [`ExtractError::ContainerPolicy`] variant is reserved for it. The
//! current fragment-parse check covers the common authoring mistakes
//! (selection that doesn't parse cleanly when shifted to column 0).

use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{
    CreateFile, CreateFileOptions, DocumentChangeOperation, DocumentChanges, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, ResourceOp, TextDocumentEdit,
    TextEdit, Url, WorkspaceEdit,
};

/// Distinct failure modes for the extract operation. Each variant maps
/// to a user-facing message in [`ExtractError::message`].
#[derive(Debug, Clone, PartialEq)]
pub enum ExtractError {
    /// `src` was the empty string.
    EmptyPath,
    /// `src` looked like a URL (`https://…`, `file://…`, …).
    UrlScheme { scheme: String },
    /// `src` is platform-absolute in a way the resolver rejects up
    /// front. In practice this fires on Windows-shaped absolute paths
    /// (e.g. `C:\foo`). On Unix a leading `/` is root-absolute per the
    /// Lex spec (resolved under the includes root) and lands in
    /// [`Self::EscapesRoot`] only if it normalizes outside that root,
    /// so `AbsolutePath` is effectively Windows-only there.
    AbsolutePath { src: String },
    /// `src` lexically normalizes outside the includes root.
    EscapesRoot { src: String },
    /// The target file already exists on disk — refuse to overwrite.
    TargetExists { path: PathBuf },
    /// The target's parent directory does not exist. Directory
    /// auto-creation is explicitly out of scope, so we fail fast here
    /// rather than producing an opaque `CreateFile`-time failure in
    /// the editor (whose atomicity guarantees vary).
    ParentDirMissing { path: PathBuf },
    /// The host document URI is not a `file://` URL (e.g. stdin, `inmemory:`).
    InvalidHostUri,
    /// The host document sits outside the configured includes root — we
    /// can't compute a sensible relative target without it.
    HostNotUnderRoot,
    /// Selection was empty or whitespace-only.
    SelectionEmpty,
    /// Selection didn't parse as valid Lex once indent-shifted.
    ParseFailed { message: String },
    /// Indent-shifted selection cannot legally sit at the host's container
    /// position — e.g. a `Session` selected for extraction into a `Definition`
    /// body. Carries a human-readable reason.
    ContainerPolicy { reason: String },
}

impl ExtractError {
    /// Human-readable message suitable for direct display in an editor
    /// error notification. Each variant returns a distinct string so the
    /// editor can show actionable feedback without re-implementing the
    /// rule.
    pub fn message(&self) -> String {
        match self {
            Self::EmptyPath => "Include path cannot be empty.".to_string(),
            Self::UrlScheme { scheme } => {
                format!("Include path must not be a URL (got scheme `{scheme}:`).")
            }
            Self::AbsolutePath { src } => format!(
                "Include path `{src}` must not be platform-absolute. Use a relative path or a root-absolute `/path` (relative to the includes root)."
            ),
            Self::EscapesRoot { src } => {
                format!("Include path `{src}` resolves outside the configured includes root.")
            }
            Self::TargetExists { path } => format!(
                "Target file `{}` already exists. Choose a different name to avoid overwriting it.",
                path.display()
            ),
            Self::ParentDirMissing { path } => format!(
                "Target directory `{}` does not exist. Create it first (extract does not auto-create directories).",
                path.display()
            ),
            Self::InvalidHostUri => {
                "Extract requires a file-backed document (got a non-file URI).".to_string()
            }
            Self::HostNotUnderRoot => {
                "Host document is outside the configured includes root.".to_string()
            }
            Self::SelectionEmpty => "Selection is empty.".to_string(),
            Self::ParseFailed { message } => {
                format!("Selection does not parse as a valid Lex fragment: {message}")
            }
            Self::ContainerPolicy { reason } => reason.clone(),
        }
    }
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ExtractError {}

/// Validate that `src` is a usable include path: non-empty, not a URL,
/// not platform-absolute, lexically normalizes inside `includes_root`,
/// and doesn't point at an existing file.
///
/// `host_path` is the canonical filesystem path of the host document —
/// relative `src` values resolve against its parent directory. The
/// returned path is the lexically normalized absolute target.
///
/// Filesystem touch is intentional: the existence check needs `Path::exists`.
/// Everything else is lexical to keep behavior stable under symlinks and
/// non-canonical roots.
pub fn validate_include_path(
    src: &str,
    host_path: &Path,
    includes_root: &Path,
) -> Result<PathBuf, ExtractError> {
    if src.is_empty() {
        return Err(ExtractError::EmptyPath);
    }
    if let Some(scheme) = url_scheme(src) {
        return Err(ExtractError::UrlScheme { scheme });
    }

    match lex_core::lex::includes::resolve_file_reference(src, Some(host_path), includes_root) {
        Ok(normalized) => {
            if normalized.exists() {
                return Err(ExtractError::TargetExists { path: normalized });
            }
            // Fail fast if the parent directory is missing. Editors apply
            // a CreateFile op against a non-existent parent inconsistently
            // (vscode silently aborts the whole WorkspaceEdit, others
            // surface a generic LSP error); surfacing a typed
            // ExtractError here keeps the editor-side message actionable.
            if let Some(parent) = normalized.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    return Err(ExtractError::ParentDirMissing {
                        path: parent.to_path_buf(),
                    });
                }
            }
            Ok(normalized)
        }
        Err(lex_core::lex::includes::IncludeError::AbsolutePath { .. }) => {
            Err(ExtractError::AbsolutePath {
                src: src.to_string(),
            })
        }
        Err(lex_core::lex::includes::IncludeError::RootEscape { .. }) => {
            Err(ExtractError::EscapesRoot {
                src: src.to_string(),
            })
        }
        Err(other) => Err(ExtractError::ParseFailed {
            message: other.to_string(),
        }),
    }
}

/// Return the URL scheme (e.g. `https`, `file`) if `src` looks like one.
/// RFC 3986 scheme: ALPHA *(ALPHA / DIGIT / "+" / "-" / ".").
///
/// Single-letter prefixes (e.g. `c:foo`) and short non-alpha prefixes don't
/// count — they're plain relative paths or Windows drive references handled
/// downstream by `AbsolutePath`.
fn url_scheme(src: &str) -> Option<String> {
    let (scheme, rest) = src.split_once(':')?;
    if rest.is_empty() {
        return None;
    }
    if scheme.len() < 2 {
        return None;
    }
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
        return None;
    }
    Some(scheme.to_string())
}

/// Indent-normalize `text` so the shallowest non-blank line lands at
/// column 0 while deeper lines keep their relative indentation. Returns
/// `(shifted_text, original_min_indent)`.
///
/// Blank/whitespace-only lines are preserved verbatim in length-zero form
/// (a single newline) so the shifted text doesn't carry trailing
/// whitespace from the source's original indent depth.
pub fn indent_shift(text: &str) -> (String, usize) {
    let min_indent = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(leading_space_count)
        .min()
        .unwrap_or(0);

    let mut shifted = String::with_capacity(text.len());
    let mut iter = text.lines().peekable();
    while let Some(line) = iter.next() {
        if line.trim().is_empty() {
            shifted.push_str(line.trim_end_matches([' ', '\t']));
        } else {
            let cut = line
                .char_indices()
                .scan(0usize, |col, (byte_idx, ch)| {
                    if *col >= min_indent {
                        Some((byte_idx, ch, true))
                    } else if ch == ' ' {
                        *col += 1;
                        Some((byte_idx, ch, false))
                    } else {
                        // Non-space (e.g. tab) before reaching min_indent: stop
                        // counting and keep the rest verbatim. Mixed-tab cases
                        // are rare in Lex (4-space indent only) and aren't
                        // worth surprising the user over.
                        Some((byte_idx, ch, true))
                    }
                })
                .find_map(|(idx, _, done)| if done { Some(idx) } else { None })
                .unwrap_or(line.len());
            shifted.push_str(&line[cut..]);
        }
        if iter.peek().is_some() || text.ends_with('\n') {
            shifted.push('\n');
        }
    }
    (shifted, min_indent)
}

fn leading_space_count(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

/// Build the atomic [`WorkspaceEdit`] for an extract operation. Runs
/// validation, indent-shifts the selection, parses the shifted text as
/// a Lex fragment (surfaces [`ExtractError::ParseFailed`] on failure),
/// and assembles the edit as a single document-changes vector so the
/// editor applies the file creation and selection replacement together.
///
/// The deeper [`ExtractError::ContainerPolicy`] check (host-side
/// `GeneralContainer` validity for the included content) is reserved
/// for a follow-up and is currently never produced.
///
/// `selection_text` is the verbatim selected slice of the host document
/// (the editor extracts it before invoking the command). `host_indent`
/// is the column at which the selection's first line starts — used to
/// indent the inserted `:: lex.include ::` annotation back to that
/// position so the replacement preserves the host's structure.
pub fn build_extract_workspace_edit(
    host_uri: &Url,
    host_path: &Path,
    selection_range: Range,
    selection_text: &str,
    host_indent: usize,
    src: &str,
    includes_root: &Path,
) -> Result<WorkspaceEdit, ExtractError> {
    if selection_text.trim().is_empty() {
        return Err(ExtractError::SelectionEmpty);
    }

    let target_path = validate_include_path(src, host_path, includes_root)?;

    let (shifted, _original_indent) = indent_shift(selection_text);

    // Parse pre-check: the shifted selection must parse as a valid Lex
    // document fragment on its own. This catches selections that break
    // structure when re-indented to column 0 (e.g. mid-list slices, or
    // verbatim closings without their opener). The deeper
    // host-container-policy check (`GeneralContainer` rules for where
    // the extracted content can sit) is deferred — see module docs.
    if let Err(e) = lex_core::lex::parsing::parse_document(&shifted) {
        return Err(ExtractError::ParseFailed {
            message: e.to_string(),
        });
    }

    let target_uri = Url::from_file_path(&target_path).map_err(|_| ExtractError::InvalidHostUri)?;

    let replacement = format!(
        "{indent}:: lex.include src=\"{src}\" ::",
        indent = " ".repeat(host_indent)
    );

    let create_op = ResourceOp::Create(CreateFile {
        uri: target_uri.clone(),
        options: Some(CreateFileOptions {
            overwrite: Some(false),
            ignore_if_exists: Some(false),
        }),
        annotation_id: None,
    });

    let text_doc_edit = TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier {
            uri: host_uri.clone(),
            version: None,
        },
        edits: vec![OneOf::Left(TextEdit {
            range: selection_range,
            new_text: replacement,
        })],
    };

    // Ensure the file-creation includes the extracted content. The LSP
    // `CreateFile` op itself only creates an empty file; the content lands
    // via a TextDocumentEdit whose URI points at the just-created file.
    let target_text_edit = TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier {
            uri: target_uri,
            version: None,
        },
        edits: vec![OneOf::Left(TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            new_text: shifted,
        })],
    };

    Ok(WorkspaceEdit {
        document_changes: Some(DocumentChanges::Operations(vec![
            DocumentChangeOperation::Op(create_op),
            DocumentChangeOperation::Edit(target_text_edit),
            DocumentChangeOperation::Edit(text_doc_edit),
        ])),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn host_in(root: &Path, rel: &str) -> PathBuf {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, "").unwrap();
        p
    }

    // ---- validate_include_path ---------------------------------------------

    #[test]
    fn validate_rejects_empty_path() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let err = validate_include_path("", &host, tmp.path()).unwrap_err();
        assert_eq!(err, ExtractError::EmptyPath);
        assert!(err.message().contains("empty"));
    }

    #[test]
    fn validate_rejects_url_scheme() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let err = validate_include_path("https://example/foo.lex", &host, tmp.path()).unwrap_err();
        assert!(matches!(err, ExtractError::UrlScheme { ref scheme } if scheme == "https"));
        assert!(err.message().contains("URL"));
    }

    #[test]
    fn validate_rejects_file_url() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let err = validate_include_path("file:///abs/foo.lex", &host, tmp.path()).unwrap_err();
        assert!(matches!(err, ExtractError::UrlScheme { .. }));
    }

    /// On Unix, a leading `/` is treated as *root-absolute* by the Lex
    /// include resolver (relative to the includes root), so `/abs/path`
    /// is a valid include path — not an `AbsolutePath` error. The
    /// `AbsolutePath` variant exists for Windows-style platform-absolute
    /// paths like `C:\foo`, which on Windows would hit
    /// `Path::is_absolute() == true` without a leading `/`.
    #[cfg(unix)]
    #[test]
    fn validate_unix_root_absolute_path_is_valid() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let ok = validate_include_path("/inside_root.lex", &host, tmp.path()).unwrap();
        assert!(ok.starts_with(tmp.path()));
        assert_eq!(
            ok.file_name().and_then(|n| n.to_str()),
            Some("inside_root.lex")
        );
    }

    #[cfg(windows)]
    #[test]
    fn validate_rejects_absolute_path() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let err = validate_include_path("C:\\extract_target.lex", &host, tmp.path()).unwrap_err();
        assert!(
            matches!(err, ExtractError::AbsolutePath { .. }),
            "expected AbsolutePath, got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_root_escape_via_dotdot() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "sub/doc.lex");
        // `../../escape.lex` from `sub/doc.lex` resolves above tmp/, outside the root.
        let err = validate_include_path("../../escape.lex", &host, tmp.path()).unwrap_err();
        assert!(
            matches!(err, ExtractError::EscapesRoot { .. }),
            "expected EscapesRoot, got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_missing_parent_directory() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        // `subdir/` does not exist under tmp, so creating `subdir/foo.lex`
        // would fail at CreateFile time. We should catch that up front.
        let err = validate_include_path("subdir/foo.lex", &host, tmp.path()).unwrap_err();
        assert!(
            matches!(err, ExtractError::ParentDirMissing { .. }),
            "expected ParentDirMissing, got {err:?}"
        );
        assert!(err.message().contains("does not exist"));
    }

    #[test]
    fn validate_accepts_existing_parent_directory() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        std::fs::create_dir_all(tmp.path().join("subdir")).unwrap();
        let resolved = validate_include_path("subdir/foo.lex", &host, tmp.path()).unwrap();
        assert!(resolved.starts_with(tmp.path()));
        assert_eq!(
            resolved.file_name().and_then(|n| n.to_str()),
            Some("foo.lex")
        );
    }

    #[test]
    fn validate_rejects_existing_target() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let target = tmp.path().join("existing.lex");
        std::fs::write(&target, "content").unwrap();
        let err = validate_include_path("existing.lex", &host, tmp.path()).unwrap_err();
        assert!(
            matches!(err, ExtractError::TargetExists { .. }),
            "expected TargetExists, got {err:?}"
        );
    }

    #[test]
    fn validate_accepts_valid_relative_path() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let resolved = validate_include_path("chapter.lex", &host, tmp.path()).unwrap();
        // Compare via lexical equality — the result is normalized.
        assert_eq!(
            resolved.file_name().and_then(|n| n.to_str()),
            Some("chapter.lex")
        );
        assert!(resolved.starts_with(tmp.path()));
    }

    #[test]
    fn error_messages_are_distinct_per_variant() {
        let variants = [
            ExtractError::EmptyPath,
            ExtractError::UrlScheme {
                scheme: "https".to_string(),
            },
            ExtractError::AbsolutePath {
                src: "/abs".to_string(),
            },
            ExtractError::EscapesRoot {
                src: "../x".to_string(),
            },
            ExtractError::TargetExists {
                path: PathBuf::from("/x"),
            },
            ExtractError::ParentDirMissing {
                path: PathBuf::from("/missing"),
            },
            ExtractError::InvalidHostUri,
            ExtractError::HostNotUnderRoot,
            ExtractError::SelectionEmpty,
            ExtractError::ParseFailed {
                message: "oops".to_string(),
            },
            ExtractError::ContainerPolicy {
                reason: "no sessions inside definitions".to_string(),
            },
        ];
        let messages: Vec<String> = variants.iter().map(|v| v.message()).collect();
        let unique: std::collections::HashSet<&String> = messages.iter().collect();
        assert_eq!(
            unique.len(),
            messages.len(),
            "every ExtractError variant must produce a distinct message; got {messages:?}"
        );
    }

    // ---- indent_shift ------------------------------------------------------

    #[test]
    fn indent_shift_zero_indent_is_noop() {
        let (out, min) = indent_shift("Line 1\nLine 2\n");
        assert_eq!(out, "Line 1\nLine 2\n");
        assert_eq!(min, 0);
    }

    #[test]
    fn indent_shift_drops_uniform_indent() {
        let (out, min) = indent_shift("    Line 1\n    Line 2\n");
        assert_eq!(out, "Line 1\nLine 2\n");
        assert_eq!(min, 4);
    }

    #[test]
    fn indent_shift_preserves_relative_indent() {
        let src = "        Outer\n            Inner\n        Outer2\n";
        let (out, min) = indent_shift(src);
        assert_eq!(out, "Outer\n    Inner\nOuter2\n");
        assert_eq!(min, 8);
    }

    #[test]
    fn indent_shift_ignores_blank_lines_for_min() {
        // The blank line in the middle shouldn't count as zero-indent.
        let src = "    Line 1\n\n    Line 2\n";
        let (out, min) = indent_shift(src);
        assert_eq!(out, "Line 1\n\nLine 2\n");
        assert_eq!(min, 4);
    }

    #[test]
    fn indent_shift_handles_12_column_indent() {
        let src = "            Deeply\n            indented\n";
        let (out, min) = indent_shift(src);
        assert_eq!(out, "Deeply\nindented\n");
        assert_eq!(min, 12);
    }

    #[test]
    fn indent_shift_no_trailing_newline_preserved() {
        let (out, min) = indent_shift("    no-newline");
        assert_eq!(out, "no-newline");
        assert_eq!(min, 4);
    }

    // ---- build_extract_workspace_edit --------------------------------------

    #[test]
    fn build_workspace_edit_rejects_empty_selection() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let host_uri = Url::from_file_path(&host).unwrap();
        let err = build_extract_workspace_edit(
            &host_uri,
            &host,
            Range::new(Position::new(0, 0), Position::new(0, 0)),
            "   \n",
            0,
            "out.lex",
            tmp.path(),
        )
        .unwrap_err();
        assert_eq!(err, ExtractError::SelectionEmpty);
    }

    #[test]
    fn build_workspace_edit_emits_create_and_edit() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let host_uri = Url::from_file_path(&host).unwrap();
        let edit = build_extract_workspace_edit(
            &host_uri,
            &host,
            Range::new(Position::new(2, 4), Position::new(5, 0)),
            "    Hello.\n    World.\n",
            4,
            "extracted.lex",
            tmp.path(),
        )
        .unwrap();

        let ops = match edit.document_changes.unwrap() {
            DocumentChanges::Operations(ops) => ops,
            _ => panic!("expected operations"),
        };
        assert_eq!(
            ops.len(),
            3,
            "expected create + target-content + host-replace"
        );
        match &ops[0] {
            DocumentChangeOperation::Op(ResourceOp::Create(c)) => {
                assert!(c.uri.path().ends_with("extracted.lex"));
            }
            _ => panic!("first op must be CreateFile"),
        }
    }

    #[test]
    fn build_workspace_edit_indent_shifts_selection_content() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let host_uri = Url::from_file_path(&host).unwrap();
        let edit = build_extract_workspace_edit(
            &host_uri,
            &host,
            Range::new(Position::new(0, 4), Position::new(2, 0)),
            "    Line 1\n    Line 2\n",
            4,
            "out.lex",
            tmp.path(),
        )
        .unwrap();
        let ops = match edit.document_changes.unwrap() {
            DocumentChanges::Operations(ops) => ops,
            _ => panic!("expected operations"),
        };
        let target_content = match &ops[1] {
            DocumentChangeOperation::Edit(edit) => match &edit.edits[0] {
                OneOf::Left(e) => e.new_text.clone(),
                _ => panic!("unexpected edit shape"),
            },
            _ => panic!("expected TextDocumentEdit"),
        };
        assert_eq!(target_content, "Line 1\nLine 2\n");
    }

    #[test]
    fn build_workspace_edit_writes_include_annotation_at_host_indent() {
        let tmp = TempDir::new().unwrap();
        let host = host_in(tmp.path(), "doc.lex");
        let host_uri = Url::from_file_path(&host).unwrap();
        let edit = build_extract_workspace_edit(
            &host_uri,
            &host,
            Range::new(Position::new(0, 8), Position::new(2, 0)),
            "        Line 1\n        Line 2\n",
            8,
            "out.lex",
            tmp.path(),
        )
        .unwrap();
        let ops = match edit.document_changes.unwrap() {
            DocumentChanges::Operations(ops) => ops,
            _ => panic!("expected operations"),
        };
        let host_replace = match &ops[2] {
            DocumentChangeOperation::Edit(edit) => match &edit.edits[0] {
                OneOf::Left(e) => e.new_text.clone(),
                _ => panic!("unexpected edit shape"),
            },
            _ => panic!("expected TextDocumentEdit"),
        };
        assert_eq!(host_replace, "        :: lex.include src=\"out.lex\" ::");
    }
}
