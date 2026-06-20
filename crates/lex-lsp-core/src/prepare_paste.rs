//! Smart paste: the `lex/preparePaste` transform.
//!
//! Lex encodes document structure as indentation (four spaces per level), which
//! makes copy-and-paste quietly hostile: clipboard text carries the *absolute*
//! indentation of wherever it was copied, so a block lifted from deep inside one
//! document arrives over- or under-indented when dropped elsewhere. Smart paste
//! re-anchors pasted text to the caret's structural context at paste time.
//!
//! This module is the whole server-side of the feature (comms#73,
//! `specs/proposals/smart-paste.lex`). The transform logic lives here, in the
//! sync core, because deciding *whether* and *how* to re-anchor is "pure logic
//! over the AST" — the editors contribute only a capture-and-apply shim. The
//! tower-lsp request wiring (`lex/preparePaste`) is a thin wrapper in
//! `lexd-lsp`'s `server.rs` that calls [`prepare_paste`].
//!
//! The entry point [`prepare_paste`] is pure with respect to document state: it
//! reads the already-parsed [`Document`], computes a string, and mutates
//! nothing.
//!
//! The four phases mirror the spec:
//! - **§3 classification** ([`classify`]): pick a [`PasteMode`], innermost-first.
//! - **§4.1 anchor** ([`resolve_anchor`]): content indentation of the enclosing
//!   container, derived from the parse — *not* the caret line's whitespace.
//! - **§4.2/§4.3 baseline + offset** ([`reanchor`]): dedent the clipboard to its
//!   common baseline, re-apply the anchor as one constant offset.
//! - **§4.4 first line** (inside [`reanchor`]): fresh-line re-anchors every line;
//!   merge strips line 1 and re-anchors the rest. On a fresh line the transform
//!   subtracts any whitespace the editor auto-indented ahead of the caret that
//!   the request range does not overwrite, so the anchor is never doubled
//!   (comms#73 #3, `surviving_leading_indent`).

use lex_analysis::utils::find_verbatim_at_position;
use lex_core::lex::ast::{Document, Position as AstPosition};
use lsp_types::{Position, Range};
use serde::{Deserialize, Serialize};

/// Lex's canonical indentation: four display columns per structural level.
pub const TAB_WIDTH: usize = 4;

/// Parameters for the `lex/preparePaste` request.
///
/// `text_document` identifies the buffer (the server resolves it to the parse it
/// already holds); `range` is what the paste replaces (its *start* is the
/// structural anchor); `pasted_text` is the raw clipboard text, verbatim,
/// including original indentation and any trailing newline.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePasteParams {
    pub text_document: lsp_types::TextDocumentIdentifier,
    pub range: Range,
    pub pasted_text: String,
}

/// Response for the `lex/preparePaste` request: the text to splice across
/// `range`, plus the [`PasteMode`] the server applied (advisory — editors may
/// surface it but need not act on it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePasteResult {
    pub text: String,
    pub mode: PasteMode,
}

/// The paste-mode classification (spec §3). Resolved innermost-first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasteMode {
    /// Caret inside a verbatim block — indentation is literal content; inserted
    /// unchanged. Wins over every other mode.
    #[serde(rename = "passthrough-verbatim")]
    PassthroughVerbatim,
    /// Caret inside a table's pipe rows — cell structure is delimiter-driven;
    /// inserted unchanged.
    #[serde(rename = "passthrough-table")]
    PassthroughTable,
    /// Clipboard is a single line with no newline — no inter-line structure to
    /// re-anchor; inserted unchanged.
    #[serde(rename = "passthrough-single-line")]
    PassthroughSingleLine,
    /// Everything else — caret in a session/list/definition body with multi-line
    /// clipboard text. The re-anchor transform runs.
    #[serde(rename = "re-anchor")]
    Reanchor,
}

impl PasteMode {
    /// The wire string the editor sees (kebab-case, e.g. `passthrough-verbatim`).
    pub fn as_str(self) -> &'static str {
        match self {
            PasteMode::PassthroughVerbatim => "passthrough-verbatim",
            PasteMode::PassthroughTable => "passthrough-table",
            PasteMode::PassthroughSingleLine => "passthrough-single-line",
            PasteMode::Reanchor => "re-anchor",
        }
    }
}

/// Compute the smart-paste result for a paste of `pasted_text` over `range`
/// in the already-parsed `document` whose source is `source`.
///
/// Pure: reads the parse, returns a string and a mode, mutates nothing.
pub fn prepare_paste(
    document: &Document,
    source: &str,
    range: Range,
    pasted_text: &str,
) -> PreparePasteResult {
    // Empty clipboard: no edit, native (no-op) paste proceeds (§6).
    if pasted_text.is_empty() {
        return PreparePasteResult {
            text: String::new(),
            mode: PasteMode::Reanchor,
        };
    }

    let fresh_line = is_fresh_line(source, range.start);
    let mode = classify(document, source, range.start, pasted_text, fresh_line);

    let text = match mode {
        PasteMode::PassthroughVerbatim
        | PasteMode::PassthroughTable
        | PasteMode::PassthroughSingleLine => pasted_text.to_string(),
        PasteMode::Reanchor => {
            let anchor = resolve_anchor(document, source, range.start);
            // §4.4 / comms#73 #3: on a fresh-line paste the editor may have
            // auto-indented the caret line with whitespace the (possibly empty)
            // request range does not overwrite. That whitespace survives the
            // edit, so the server must account for it — otherwise the anchor it
            // emits on the first line stacks on top of the surviving spaces and
            // the line is double-indented. A merge paste's first line is not
            // anchored at all (it continues existing content, §4.4), so there is
            // no anchor to double there; compensation only matters where the
            // first line carries an anchor, i.e. on a fresh line.
            let caret_indent = if fresh_line {
                surviving_leading_indent(source, range.start)
            } else {
                0
            };
            reanchor(pasted_text, anchor, fresh_line, caret_indent)
        }
    };

    PreparePasteResult { text, mode }
}

/// §3: classify the paste by the caret's structural context, innermost-first.
///
/// `fresh_line` (§4.4) is the caret-position signal computed by [`is_fresh_line`]:
/// `true` when everything before the caret on its source line is whitespace (the
/// paste starts a new block), `false` when the paste merges into existing content.
fn classify(
    document: &Document,
    source: &str,
    anchor_pos: Position,
    pasted_text: &str,
    fresh_line: bool,
) -> PasteMode {
    let ast_pos = to_ast_position(anchor_pos);

    // Innermost-first: a single-line paste inside a verbatim block is
    // `passthrough-verbatim`, not `passthrough-single-line` (§3 closing note).
    // Verbatim wins outright — re-indenting literal content would corrupt the
    // very thing verbatim blocks exist to preserve.
    if find_verbatim_at_position(document, ast_pos).is_some() {
        return PasteMode::PassthroughVerbatim;
    }

    // Table: cell structure is delimiter-driven, not indentation-driven.
    if is_in_table(source, anchor_pos) {
        return PasteMode::PassthroughTable;
    }

    // Single-line clipboard: split on caret context (§3, gemini refinement).
    //   - Merge (caret follows existing content): no structural block is being
    //     placed — the line continues the current one, so it stays passthrough.
    //   - Fresh line (caret on a blank/whitespace-only prefix): the single line
    //     IS a new block being dropped at this structural level, so it is
    //     re-anchored just like a multi-line fresh paste. A line carrying the
    //     source's absolute indentation would otherwise land at the wrong level.
    if is_single_line(pasted_text) && !fresh_line {
        return PasteMode::PassthroughSingleLine;
    }

    PasteMode::Reanchor
}

/// True when the clipboard holds a single line with no newline (§3).
fn is_single_line(pasted_text: &str) -> bool {
    !pasted_text.contains('\n')
}

/// Detect whether `pos` sits on a table pipe row. Tables are delimiter-driven,
/// so a leading `|` on the caret's source line is the structural signal — the
/// same heuristic the table-navigation core uses. Cheap and parse-independent;
/// the verbatim check above has already claimed the parse-only case.
fn is_in_table(source: &str, pos: Position) -> bool {
    line_at(source, pos.line as usize)
        .map(|line| line.trim_start().starts_with('|'))
        .unwrap_or(false)
}

/// §4.1: the anchor is the *content indentation of the structural container
/// enclosing the range start* — the body indentation a new child of that
/// container should carry. Derived from the parse, never from the caret line's
/// whitespace, so it is correct on a blank line left at column zero deep inside
/// a session.
///
/// Walks the AST for the innermost container (session / definition / list item)
/// whose source range encloses `pos`. The anchor is that container's content
/// indentation, read as the leading-whitespace width (in display columns) of the
/// container's first non-blank content line. With no enclosing container — paste
/// at document top level — the anchor is zero.
pub fn resolve_anchor(document: &Document, source: &str, pos: Position) -> usize {
    let ast_pos = to_ast_position(pos);
    // Pre-split the source into lines *once*. `content_indent` reads a line per
    // candidate container child; without this, each read re-split the whole
    // source (`line_at`), giving O(N²) over the document. A shared slice keeps
    // the walk linear.
    let lines: Vec<&str> = source
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut best: Option<usize> = None;
    visit_containers(&document.root.children, &lines, ast_pos, &mut best);
    best.unwrap_or(0)
}

/// Recursively descend container children, tracking the deepest content
/// indentation of any container that encloses `pos`. Deeper (later-visited)
/// containers overwrite shallower ones, yielding the innermost anchor.
fn visit_containers(
    items: &[lex_core::lex::ast::ContentItem],
    lines: &[&str],
    pos: AstPosition,
    best: &mut Option<usize>,
) {
    use lex_core::lex::ast::{AstNode, ContentItem};
    for item in items {
        match item {
            ContentItem::Session(session) => {
                if encloses_body(session.range(), pos) {
                    if let Some(indent) = content_indent(&session.children, lines) {
                        *best = Some(indent);
                    }
                    visit_containers(&session.children, lines, pos, best);
                }
            }
            ContentItem::Definition(def) => {
                if encloses_body(def.range(), pos) {
                    if let Some(indent) = content_indent(&def.children, lines) {
                        *best = Some(indent);
                    }
                    visit_containers(&def.children, lines, pos, best);
                }
            }
            ContentItem::List(list) => {
                for entry in &list.items {
                    if let ContentItem::ListItem(li) = entry {
                        if encloses_body(li.range(), pos) {
                            if let Some(indent) = content_indent(&li.children, lines) {
                                *best = Some(indent);
                            }
                            visit_containers(&li.children, lines, pos, best);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Whether `pos` sits inside a container's *body* — within its source range and
/// on a line strictly after its head (title / subject / marker) line. The
/// strict-after rule is what makes a caret on the head line resolve to the
/// *parent* container (per §4.1, the anchor is the body indentation a new child
/// would carry, which the head line itself is not part of), while still
/// catching a blank line left at column zero anywhere below the head.
fn encloses_body(range: &lex_core::lex::ast::Range, pos: AstPosition) -> bool {
    range.contains(pos) && pos.line > range.start.line
}

/// The content indentation of a container: the leading-whitespace width (in
/// display columns at [`TAB_WIDTH`]) of the first non-blank source line among
/// its children. `None` when the container has no materialised content line to
/// read (e.g. an empty body) — the caller then falls back to a shallower
/// container or to zero.
fn content_indent(children: &[lex_core::lex::ast::ContentItem], lines: &[&str]) -> Option<usize> {
    use lex_core::lex::ast::{AstNode, ContentItem};
    for child in children {
        // Skip blank-line groups — they carry no content indentation.
        if matches!(child, ContentItem::BlankLineGroup(_)) {
            continue;
        }
        let start_line = child.range().start.line;
        if let Some(line) = lines.get(start_line) {
            if !line.trim().is_empty() {
                return Some(leading_width(line));
            }
        }
    }
    None
}

/// §4.4: a *fresh-line* paste is one whose range start sits on a blank or
/// whitespace-only line (the whole paste is a new block). A *merge* paste
/// follows existing content on its line. We treat the line positionally: if
/// everything before the caret column on its source line is whitespace, it's a
/// fresh line.
pub fn is_fresh_line(source: &str, pos: Position) -> bool {
    match line_at(source, pos.line as usize) {
        Some(line) => {
            // `pos.character` is a UTF-8 *byte* offset, not a char count — the
            // rest of this server treats LSP columns as byte offsets (see
            // `slice_text_by_range` in `lexd-lsp`'s `server.rs`). Walk chars,
            // accumulating their byte lengths, and stop at the caret column:
            // counting `chars().take(character)` would over-read on any line
            // with multi-byte content before the caret. No allocation.
            let caret = pos.character as usize;
            let mut bytes_seen = 0;
            for ch in line.chars() {
                if bytes_seen >= caret {
                    break;
                }
                if !ch.is_whitespace() {
                    return false;
                }
                bytes_seen += ch.len_utf8();
            }
            true
        }
        // No such line (e.g. paste at the very end past the last newline):
        // there is no pre-existing content to merge into, so it's fresh.
        None => true,
    }
}

/// §4.4 / comms#73 #3: the display width of the leading whitespace on `pos`'s
/// line that lies *before* `pos` and therefore survives a replacement of the
/// request range (whose start is `pos`). This is the whitespace an editor may
/// have auto-indented onto a fresh line without the range covering it.
///
/// Walks chars up to the caret's byte offset (LSP columns are byte offsets in
/// this server — see [`is_fresh_line`]) accumulating display width, stopping at
/// the caret or at the first non-whitespace char. Callers invoke it only for
/// fresh-line pastes, where the prefix is whitespace by definition; the
/// non-whitespace `break` and the past-end case keep it total regardless.
fn surviving_leading_indent(source: &str, pos: Position) -> usize {
    let Some(line) = line_at(source, pos.line as usize) else {
        return 0;
    };
    let caret = pos.character as usize;
    let mut bytes_seen = 0;
    let mut width = 0;
    for ch in line.chars() {
        if bytes_seen >= caret {
            break;
        }
        match ch {
            ' ' => width += 1,
            '\t' => width += TAB_WIDTH - (width % TAB_WIDTH),
            _ => break,
        }
        bytes_seen += ch.len_utf8();
    }
    width
}

/// The re-anchor transform (§4.2–§4.4), pure whitespace arithmetic over lines.
///
/// - `anchor`: target content indentation (display columns).
/// - `fresh_line`: §4.4 — `true` re-anchors every line; `false` (merge) strips
///   line 1 down to the clipboard baseline (preserving any relative indentation
///   it carried *beyond* the baseline) with no anchor, and re-anchors lines 2..n.
/// - `caret_indent`: display width of whitespace already on the caret line that
///   precedes the splice point and survives the edit (§4.4 / comms#73 #3). On a
///   fresh line an editor may auto-indent the caret without the request range
///   covering that whitespace; subtracting it from the first emitted line keeps
///   the anchor from stacking on top of the surviving spaces. Zero for merge
///   pastes and for fresh lines whose range already covers (or starts before)
///   the leading whitespace.
///
/// The clipboard's trailing newline (and internal blank lines) are preserved.
pub fn reanchor(pasted_text: &str, anchor: usize, fresh_line: bool, caret_indent: usize) -> String {
    // Split into lines while remembering whether the text ended with a newline,
    // so we can reproduce a trailing newline exactly (§6).
    let had_trailing_newline = pasted_text.ends_with('\n');
    let body = pasted_text.strip_suffix('\n').unwrap_or(pasted_text);
    // `split('\n')` on "" yields a single "" element; guard so an all-empty
    // clipboard doesn't masquerade as a one-line paste.
    let lines: Vec<&str> = body.split('\n').collect();

    // §4.2: baseline = min leading-whitespace width over non-blank lines.
    let baseline = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| leading_width(line))
        .min()
        .unwrap_or(0);

    // §4.3: delta = anchor - baseline, applied as one constant offset. Signed,
    // so a shallower paste (negative delta) clamps at zero per line.
    let delta = anchor as isize - baseline as isize;

    let mut out = String::new();
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }

        // Blank (empty or whitespace-only) lines are emitted empty: never pad a
        // blank line (§4.3).
        if line.trim().is_empty() {
            continue;
        }

        let (orig_indent, content) = split_leading(line);

        if idx == 0 && !fresh_line {
            // §4.4 merge: the first pasted line continues existing content, so
            // it gets no anchor. But strip only down to the clipboard *baseline*,
            // not to bare content: if line 1 was indented deeper than the block
            // baseline, that extra relative indentation is part of the copied
            // structure and is preserved (gemini refinement). `orig_indent` is
            // never below `baseline` (baseline is the per-line min), so the
            // subtraction is non-negative; `max(0, …)` keeps it total.
            let rel_indent = orig_indent.saturating_sub(baseline);
            out.extend(std::iter::repeat_n(' ', rel_indent));
            out.push_str(content);
            continue;
        }

        // §4.3: max(0, original_indent + delta) spaces, then stripped content.
        let mut new_indent = (orig_indent as isize + delta).max(0) as usize;
        // §4.4 / comms#73 #3: the first emitted line shares the caret's physical
        // line, so any whitespace already present before the splice (and not
        // overwritten by the range) is still in the buffer. Drop that much from
        // the emitted indent so the two don't add up to a doubled anchor. Only
        // the first line is affected; this branch is reached at idx == 0 only on
        // a fresh-line paste (merge handles its own first line above), and
        // `caret_indent` is zero for every non-fresh paste, so the guard is
        // exact. A `saturating_sub` keeps it total when the surviving whitespace
        // already exceeds the target — an insert-only edit cannot remove it, so
        // the line clamps to no added indent (exact dedent then needs the editor
        // to expand the range, per the §4.4 contract).
        if idx == 0 {
            new_indent = new_indent.saturating_sub(caret_indent);
        }
        out.extend(std::iter::repeat_n(' ', new_indent));
        out.push_str(content);
    }

    if had_trailing_newline {
        out.push('\n');
    }
    out
}

/// Leading-whitespace width of `line` in display columns at [`TAB_WIDTH`]: a tab
/// advances to the next multiple of `TAB_WIDTH`, a space counts one. Measuring in
/// columns (not bytes) makes mixed tabs/spaces comparable (§6).
fn leading_width(line: &str) -> usize {
    let mut width = 0;
    for ch in line.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => width += TAB_WIDTH - (width % TAB_WIDTH),
            _ => break,
        }
    }
    width
}

/// Split a line into (leading-whitespace display width, content after it).
/// Whitespace is measured in display columns so the returned content is the
/// line with all leading spaces/tabs stripped.
fn split_leading(line: &str) -> (usize, &str) {
    let mut width = 0;
    for (offset, ch) in line.char_indices() {
        match ch {
            ' ' => width += 1,
            '\t' => width += TAB_WIDTH - (width % TAB_WIDTH),
            _ => return (width, &line[offset..]),
        }
    }
    // Whole line was whitespace (callers guard against this, but be total).
    (width, "")
}

/// The 0-indexed `line_no`th line of `source`, without its line terminator.
fn line_at(source: &str, line_no: usize) -> Option<&str> {
    source
        .split('\n')
        .nth(line_no)
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
}

/// LSP [`Position`] (u32 line/character) → AST [`AstPosition`] (usize
/// line/column). Both are 0-indexed and line-based; the column conventions agree
/// for the leading-whitespace region we care about (ASCII indentation).
fn to_ast_position(pos: Position) -> AstPosition {
    AstPosition::new(pos.line as usize, pos.character as usize)
}

#[cfg(test)]
mod tests;
