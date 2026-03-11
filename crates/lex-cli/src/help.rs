//! In-editor help system for Lex format documentation.
//!
//! Provides access to the Lex specification and guide documents from within
//! editors. This enables features like `:LexHelp` commands that display
//! contextual documentation without leaving the editor.
//!
//! The help system discovers `.lex` files in the `docs/` and `specs/` directories
//! of the Lex repository and returns their contents filtered by topic.
//!
//! # Compile-time Dependency
//!
//! This module uses `CARGO_MANIFEST_DIR` to locate documentation files at compile
//! time. This works for development builds but requires the documentation to be
//! bundled with the binary for distribution. For CLI usage, the docs are typically
//! available; for LSP commands, consider alternative discovery mechanisms.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A single help document with its title and content.
#[derive(Debug, Clone, PartialEq)]
pub struct HelpEntry {
    /// Display title derived from the file path (e.g., "specs/grammar.lex").
    pub title: String,
    /// The full content of the documentation file.
    pub content: String,
}

/// Response containing matching help entries.
#[derive(Debug, Clone, PartialEq)]
pub struct HelpResponse {
    /// Matching documentation entries, possibly empty if no matches found.
    pub entries: Vec<HelpEntry>,
}

/// Queries the documentation for help on a topic.
///
/// If `topic` is `Some`, filters entries to those whose paths contain the topic
/// string (case-insensitive). If `None`, returns default entries: the main
/// overview document and general reference, or up to 3 arbitrary docs if those
/// aren't found.
///
/// # Errors
///
/// Returns `io::Error` if the documentation directories cannot be read.
pub fn query_help(topic: Option<&str>) -> io::Result<HelpResponse> {
    let files = discover_files(&["comms/docs", "comms/specs"])?;
    let matches = match topic {
        Some(keyword) => filter_by_topic(&files, keyword),
        None => default_entries(&files),
    };
    let mut entries = Vec::new();
    for path in matches {
        if let Ok(content) = fs::read_to_string(&path) {
            entries.push(HelpEntry {
                title: display_title(&path),
                content,
            });
        }
    }
    Ok(HelpResponse { entries })
}

fn discover_files(roots: &[&str]) -> io::Result<Vec<PathBuf>> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| Path::new("."));
    let mut files = Vec::new();
    for root in roots {
        let path = base.join(root);
        if path.exists() {
            collect_files(&path, &mut files)?;
        }
    }
    Ok(files)
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("lex") {
            files.push(path);
        }
    }
    Ok(())
}

fn filter_by_topic(files: &[PathBuf], topic: &str) -> Vec<PathBuf> {
    let needle = topic.to_ascii_lowercase();
    files
        .iter()
        .filter(|path| {
            path.to_string_lossy()
                .to_ascii_lowercase()
                .contains(&needle)
        })
        .cloned()
        .collect()
}

fn default_entries(files: &[PathBuf]) -> Vec<PathBuf> {
    let mut entries = Vec::new();
    if let Some(entry) = find_file(files, "on-all-of-lex") {
        entries.push(entry);
    }
    if let Some(entry) = find_file(files, "general.lex") {
        entries.push(entry);
    }
    if entries.is_empty() {
        entries.extend_from_slice(&files[..files.len().min(3)]);
    }
    entries
}

fn find_file(files: &[PathBuf], needle: &str) -> Option<PathBuf> {
    let lower = needle.to_ascii_lowercase();
    files
        .iter()
        .find(|path| path.to_string_lossy().to_ascii_lowercase().contains(&lower))
        .cloned()
}

fn display_title(path: &Path) -> String {
    path.strip_prefix("docs")
        .or_else(|_| path.strip_prefix("specs"))
        .unwrap_or(path)
        .to_string_lossy()
        .trim_start_matches('/')
        .trim_start_matches('\\')
        .replace("\\", "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_general_help_without_topic() {
        let response = query_help(None).expect("help");
        assert!(!response.entries.is_empty());
        assert!(
            response.entries[0].title.contains("on-all-of-lex")
                || response.entries[0].title.contains("general")
        );
    }

    #[test]
    fn filters_entries_by_topic() {
        let response = query_help(Some("grammar")).expect("help");
        assert!(response
            .entries
            .iter()
            .any(|entry| entry.title.to_ascii_lowercase().contains("grammar")));
    }
}
