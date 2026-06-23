//! LSP ↔ AST conversion layer.
//!
//! Free functions that translate between lex-core AST types
//! (`Position`, `Range`, `DocumentLink`, semantic tokens, …) and their
//! `tower_lsp::lsp_types` counterparts, plus the semantic-token wire
//! encoding and a couple of small text-slicing helpers.
//!
//! Position-encoding convention: `Position.character` is a **UTF-8 byte
//! offset** into the line (see the crate-level docs). [`slice_text_by_range`]
//! is the canonical byte-offset slicing routine, including the
//! multi-byte-boundary guards.

use std::path::Path;

use lex_core::lex::ast::links::{DocumentLink as AstDocumentLink, LinkType};
use lex_core::lex::ast::range::SourceLocation;
use lex_core::lex::ast::{Position as AstPosition, Range as AstRange};
use lsp_types::{FormattingOptions, FormattingProperty};
use tower_lsp::lsp_types::{
    CompletionItem, DocumentLink, DocumentSymbol, FoldingRange, Location, Position, Range,
    SemanticToken, SemanticTokenType, SemanticTokensLegend, TextEdit, Url,
};

use lex_analysis::completion::CompletionCandidate;
use lex_babel::formats::lex::formatting_rules::FormattingRules;

use crate::features::document_symbols::LexDocumentSymbol;
use crate::features::folding_ranges::LexFoldingRange;
use crate::features::formatting::{LineRange as FormattingLineRange, TextEditSpan};
use crate::features::semantic_tokens::{LexSemanticToken, SEMANTIC_TOKEN_KINDS};

use super::document_store::DocumentEntry;

pub(crate) fn indent_level_from_position(
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

pub(crate) fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: SEMANTIC_TOKEN_KINDS
            .iter()
            .map(|kind| SemanticTokenType::new(kind.as_str()))
            .collect(),
        token_modifiers: Vec::new(),
    }
}

pub(crate) fn to_lsp_position(position: &AstPosition) -> Position {
    Position::new(position.line as u32, position.column as u32)
}

pub(crate) fn to_lsp_range(range: &AstRange) -> Range {
    Range {
        start: to_lsp_position(&range.start),
        end: to_lsp_position(&range.end),
    }
}

pub(crate) fn to_lsp_location(uri: &Url, range: &AstRange) -> Location {
    Location {
        uri: uri.clone(),
        range: to_lsp_range(range),
    }
}

pub(crate) fn spans_to_text_edits(text: &str, spans: Vec<TextEditSpan>) -> Vec<TextEdit> {
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

pub(crate) fn to_formatting_line_range(range: &Range) -> FormattingLineRange {
    let start = range.start.line as usize;
    let mut end = range.end.line as usize;
    if range.end.character > 0 || end == start {
        end += 1;
    }
    FormattingLineRange { start, end }
}

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
pub(crate) fn apply_formatting_overrides(rules: &mut FormattingRules, options: &FormattingOptions) {
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

pub(crate) fn from_lsp_position(position: Position) -> AstPosition {
    AstPosition::new(position.line as usize, position.character as usize)
}

pub(crate) fn encode_semantic_tokens(
    tokens: &[LexSemanticToken],
    text: &str,
) -> Vec<SemanticToken> {
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

pub(crate) fn compute_line_offsets(text: &str) -> Vec<usize> {
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
pub(crate) fn split_token_on_lines(
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
pub(crate) fn to_document_symbol(symbol: &LexDocumentSymbol) -> DocumentSymbol {
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

pub(crate) fn to_lsp_folding_range(range: &LexFoldingRange) -> FoldingRange {
    FoldingRange {
        start_line: range.start_line,
        start_character: range.start_character,
        end_line: range.end_line,
        end_character: range.end_character,
        kind: range.kind.clone(),
        collapsed_text: None,
    }
}

pub(crate) fn to_lsp_completion_item(candidate: &CompletionCandidate) -> CompletionItem {
    CompletionItem {
        label: candidate.label.clone(),
        kind: Some(candidate.kind),
        detail: candidate.detail.clone(),
        insert_text: candidate.insert_text.clone(),
        ..Default::default()
    }
}

pub(crate) fn build_document_link(uri: &Url, link: &AstDocumentLink) -> Option<DocumentLink> {
    let target = link_target_uri(uri, link)?;
    Some(DocumentLink {
        range: to_lsp_range(&link.range),
        target: Some(target),
        tooltip: None,
        data: None,
    })
}

pub(crate) fn link_target_uri(document_uri: &Url, link: &AstDocumentLink) -> Option<Url> {
    match link.link_type {
        LinkType::Url => Url::parse(&link.target).ok(),
        LinkType::File | LinkType::VerbatimSrc => {
            resolve_file_like_target(document_uri, &link.target)
        }
    }
}

pub(crate) fn resolve_file_like_target(document_uri: &Url, target: &str) -> Option<Url> {
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

pub(crate) fn parent_directory_uri(uri: &Url) -> Url {
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

/// Slice `text` by an LSP `Range`. Returns `None` when the range falls
/// outside the document or splits a multi-byte character.
///
/// `character` is treated as a **UTF-8 byte offset**, following the
/// crate's position-encoding convention: lex-core's
/// `SourceLocation::byte_to_position` computes
/// `column = byte_offset - line_start`, and `to_lsp_position` forwards
/// that value to LSP as-is. Using char offsets here would mis-slice any
/// selection containing multi-byte characters. See the crate-level
/// "Position Encoding" docs for the full convention (and its one known
/// straggler).
pub(crate) fn slice_text_by_range(text: &str, range: Range) -> Option<String> {
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

pub(crate) fn head_range() -> Range {
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
pub(crate) fn include_preview_markdown(src: &str, target: &Path, target_source: &str) -> String {
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
