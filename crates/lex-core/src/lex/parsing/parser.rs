//! Declarative Grammar Engine - Regex & Imperative Parser for lex
//!
//! This module implements a unified parser using declarative regex grammar rules
//! with imperative fallbacks for patterns that need look-ahead:
//! 1. Converts token sequences to grammar notation strings
//! 2. Matches against regex patterns in declaration order
//! 3. Falls back to imperative matchers (verbatim blocks, paragraphs)
//! 4. Extracts consumed token indices from regex match
//! 5. Recursively descends into containers when building AST
//!
//! The grammar patterns and AST building logic have been extracted to separate modules:
//! - `grammar.rs` - Pattern definitions and matching order
//! - `builder.rs` - AST node construction from matched patterns

use crate::lex::parsing::ir::{NodeType, ParseNode};
use crate::lex::token::{LineContainer, LineType, Token};
use regex::Regex;
use std::ops::Range;

mod builder;
mod grammar;

use builder::{
    blank_line_node_from_range, container_starts_with_pipe_row, convert_pattern_to_node,
    PatternMatch,
};
use grammar::{GRAMMAR_PATTERNS, LIST_ITEM_REGEX};

/// Pattern matcher for declarative grammar using regex-based matching
pub struct GrammarMatcher;

impl GrammarMatcher {
    /// Try to match a pattern at the current level using regex patterns.
    ///
    /// Converts the current token sequence to a grammar string, matches against
    /// regex patterns in declaration order, and returns the matched pattern with
    /// consumed token indices.
    ///
    /// Returns (matched_pattern, consumed_indices)
    #[allow(clippy::too_many_arguments)]
    fn try_match(
        tokens: &[LineContainer],
        start_idx: usize,
        allow_sessions: bool,
        is_first_item: bool,
        has_preceding_blank: bool,
        has_preceding_boundary: bool,
        prev_was_session: bool,
        suppress_title: bool,
    ) -> Option<(PatternMatch, Range<usize>)> {
        if start_idx >= tokens.len() {
            return None;
        }

        // Try verbatim block first (requires special imperative matching logic)
        if let Some(result) = Self::match_verbatim_block(tokens, start_idx) {
            return Some(result);
        }

        // Try table: subject + container whose first non-blank line is a pipe row.
        // Must run before the definition pattern (which matches the same subject + container).
        if let Some(result) = Self::match_table(tokens, start_idx) {
            return Some(result);
        }

        // Convert remaining tokens to grammar string
        let remaining_tokens = &tokens[start_idx..];
        let token_string = Self::tokens_to_grammar_string(remaining_tokens)?;

        // Try each pattern in order
        for (pattern_name, pattern_regex_str) in GRAMMAR_PATTERNS {
            // Skip patterns handled imperatively above
            if *pattern_name == "verbatim_block" {
                continue;
            }
            if let Ok(regex) = Regex::new(pattern_regex_str) {
                if let Some(caps) = regex.captures(&token_string) {
                    let full_match = caps.get(0)?;
                    let consumed_count = Self::count_consumed_tokens(full_match.as_str());

                    // Use captures to extract indices and build the pattern
                    let pattern = match *pattern_name {
                        "annotation_block" => PatternMatch::AnnotationBlock {
                            start_idx: 0,
                            content_idx: 1,
                        },
                        "annotation_single" => PatternMatch::AnnotationSingle { start_idx: 0 },
                        // A lone top-level list item with a nested container
                        // (lex#685). Same shape as `list_no_blank` — no preceding
                        // blank, items captured the same way — so it shares that
                        // arm's extraction logic.
                        "list_no_blank" | "list_single_with_container" => {
                            // List without preceding blank line
                            let items_str = caps.name("items")?.as_str();
                            let mut items = Vec::new();
                            let mut token_idx = 0; // No blank line, so start at 0
                            for item_cap in LIST_ITEM_REGEX.find_iter(items_str) {
                                let has_container = item_cap.as_str().contains("<container>");
                                items.push((
                                    token_idx,
                                    if has_container {
                                        Some(token_idx + 1)
                                    } else {
                                        None
                                    },
                                ));
                                token_idx += if has_container { 2 } else { 1 };
                            }

                            let trailing_blank_count = caps
                                .name("trailing_blank")
                                .map(|m| Self::count_consumed_tokens(m.as_str()))
                                .unwrap_or(0);
                            let trailing_blank_range = if trailing_blank_count > 0 {
                                Some(
                                    start_idx + consumed_count - trailing_blank_count
                                        ..start_idx + consumed_count,
                                )
                            } else {
                                None
                            };

                            PatternMatch::List {
                                items,
                                preceding_blank_range: None,
                                trailing_blank_range,
                            }
                        }
                        "list" => {
                            let blank_count = caps
                                .name("blank")
                                .map(|m| Self::count_consumed_tokens(m.as_str()))
                                .unwrap_or(0);
                            let items_str = caps.name("items")?.as_str();
                            let mut items = Vec::new();
                            let mut token_idx = blank_count;
                            for item_cap in LIST_ITEM_REGEX.find_iter(items_str) {
                                let has_container = item_cap.as_str().contains("<container>");
                                items.push((
                                    token_idx,
                                    if has_container {
                                        Some(token_idx + 1)
                                    } else {
                                        None
                                    },
                                ));
                                token_idx += if has_container { 2 } else { 1 };
                            }
                            let trailing_blank_count = caps
                                .name("trailing_blank")
                                .map(|m| Self::count_consumed_tokens(m.as_str()))
                                .unwrap_or(0);
                            let preceding_blank_range = if blank_count > 0 {
                                Some(start_idx..start_idx + blank_count)
                            } else {
                                None
                            };
                            let trailing_blank_range = if trailing_blank_count > 0 {
                                Some(
                                    start_idx + consumed_count - trailing_blank_count
                                        ..start_idx + consumed_count,
                                )
                            } else {
                                None
                            };

                            PatternMatch::List {
                                items,
                                preceding_blank_range,
                                trailing_blank_range,
                            }
                        }
                        "session" => {
                            // Allow session_no_blank in these cases:
                            // 1. At document start (is_first_item=true), OR
                            // 2. At container start when sessions are allowed (start_idx=0 && allow_sessions=true), OR
                            // 3. After a BlankLineGroup when sessions are allowed (has_preceding_blank && allow_sessions)
                            // 4. Immediately after another session (prev_was_session && allow_sessions)
                            // 5. Immediately after a container that just closed (has_preceding_boundary && allow_sessions)
                            // This prevents Sessions inside Definitions while allowing legitimate session sequences.
                            if !allow_sessions {
                                continue; // Definitions and other containers don't allow sessions
                            }
                            if !(is_first_item
                                || start_idx == 0
                                || has_preceding_blank
                                || has_preceding_boundary
                                || prev_was_session)
                            {
                                continue; // Sessions need a separator or another session before them
                            }
                            let blank_str = caps.name("blank")?.as_str();
                            let blank_count = Self::count_consumed_tokens(blank_str);
                            PatternMatch::Session {
                                subject_idx: 0,
                                content_idx: 1 + blank_count,
                                preceding_blank_range: None,
                            }
                        }
                        "definition" => PatternMatch::Definition {
                            subject_idx: 0,
                            content_idx: 1,
                        },
                        "blank_line_group" => PatternMatch::BlankLineGroup,
                        "document_title_with_subtitle" => {
                            // A `:: doc.untitled ::` among the leading document-level
                            // annotations suppresses title promotion (ADR-0002).
                            if suppress_title {
                                continue;
                            }
                            // No container lookahead needed: the subtitle variant
                            // consumed two lines (title + subtitle) before blank lines.
                            // A session only has one line before blank + container, so
                            // the presence of a container after the blank is NOT ambiguous
                            // here — it's the document body, not a session body.
                            // `<lead>` absorbs any leading blank lines (ADR-0002), so the
                            // title/subtitle sit at DocumentStart(0) + lead + 1 / + 2.
                            let lead_count = caps
                                .name("lead")
                                .map(|m| Self::count_consumed_tokens(m.as_str()))
                                .unwrap_or(0);
                            PatternMatch::DocumentTitle {
                                title_idx: 1 + lead_count,
                                subtitle_idx: Some(2 + lead_count),
                            }
                        }
                        "document_title" => {
                            if suppress_title {
                                continue;
                            }
                            // Imperative negative lookahead: not followed by container
                            let next_idx = start_idx + consumed_count;
                            if next_idx < tokens.len()
                                && matches!(&tokens[next_idx], LineContainer::Container { .. })
                            {
                                // Followed by container — this is a session, not a title
                                continue;
                            }
                            // Match is: DocumentStart(0) + lead blanks + title line + blank lines
                            let lead_count = caps
                                .name("lead")
                                .map(|m| Self::count_consumed_tokens(m.as_str()))
                                .unwrap_or(0);
                            PatternMatch::DocumentTitle {
                                title_idx: 1 + lead_count,
                                subtitle_idx: None,
                            }
                        }
                        "document_start" => PatternMatch::DocumentStart,
                        _ => continue,
                    };

                    return Some((pattern, start_idx..start_idx + consumed_count));
                }
            }
        }

        // Paragraph: matched imperatively after all regex patterns fail.
        // Stops before element boundaries (list starts, definition starts).
        Self::match_paragraph(tokens, start_idx)
    }

    /// Convert remaining tokens to grammar notation string
    fn tokens_to_grammar_string(tokens: &[LineContainer]) -> Option<String> {
        let mut result = String::new();
        for token in tokens {
            match token {
                LineContainer::Token(t) => {
                    result.push_str(&t.line_type.to_grammar_string());
                }
                LineContainer::Container { .. } => {
                    result.push_str("<container>");
                }
            }
        }
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Count how many tokens are represented in a grammar string.
    /// Each token type in angle brackets represents one token.
    fn count_consumed_tokens(grammar_str: &str) -> usize {
        grammar_str.matches('<').count()
    }

    /// Match paragraphs using imperative logic.
    ///
    /// Consumes content lines (paragraph, dialog, subject, list) one at a time,
    /// stopping before sequences that form other block elements:
    /// - Before 2+ consecutive list-like lines (list start)
    /// - Before a subject line followed by a container (definition start)
    fn match_paragraph(
        tokens: &[LineContainer],
        start_idx: usize,
    ) -> Option<(PatternMatch, Range<usize>)> {
        use LineType::*;

        let len = tokens.len();
        let mut idx = start_idx;

        while idx < len {
            match &tokens[idx] {
                LineContainer::Token(t) => match t.line_type {
                    ParagraphLine | DialogLine => {
                        idx += 1;
                    }
                    SubjectLine => {
                        // Stop if followed by container (definition start)
                        if Self::next_is_container(tokens, idx) {
                            break;
                        }
                        idx += 1;
                    }
                    SubjectOrListItemLine => {
                        // Stop if followed by container (definition start)
                        if Self::next_is_container(tokens, idx) {
                            break;
                        }
                        // Stop if followed by another list-like line (list start)
                        if Self::next_is_list_like(tokens, idx) {
                            break;
                        }
                        idx += 1;
                    }
                    ListLine => {
                        // Stop if followed by another list-like line, possibly
                        // with a container in between (list start)
                        if Self::next_is_list_continuation(tokens, idx) {
                            break;
                        }
                        idx += 1;
                    }
                    _ => break, // Blank line, annotation, document-start, etc.
                },
                LineContainer::Container { .. } => break,
            }
        }

        if idx > start_idx {
            Some((
                PatternMatch::Paragraph {
                    start_idx: 0,
                    end_idx: idx - start_idx - 1,
                },
                start_idx..idx,
            ))
        } else {
            None
        }
    }

    /// Check if the token after `idx` is a Container.
    fn next_is_container(tokens: &[LineContainer], idx: usize) -> bool {
        let next = idx + 1;
        next < tokens.len() && matches!(&tokens[next], LineContainer::Container { .. })
    }

    /// Check if the token after `idx` is a list-like line (ListLine or SubjectOrListItemLine).
    fn next_is_list_like(tokens: &[LineContainer], idx: usize) -> bool {
        let next = idx + 1;
        if next >= tokens.len() {
            return false;
        }
        matches!(
            &tokens[next],
            LineContainer::Token(t) if matches!(t.line_type, LineType::ListLine | LineType::SubjectOrListItemLine)
        )
    }

    /// Check if the token after `idx` starts a list continuation:
    /// either directly another list-like line, or a container followed by a list-like line.
    fn next_is_list_continuation(tokens: &[LineContainer], idx: usize) -> bool {
        let next = idx + 1;
        if next >= tokens.len() {
            return false;
        }
        match &tokens[next] {
            LineContainer::Token(t) => {
                matches!(
                    t.line_type,
                    LineType::ListLine | LineType::SubjectOrListItemLine
                )
            }
            LineContainer::Container { .. } => {
                // Container after list item — check if another list item follows
                let after = next + 1;
                after < tokens.len()
                    && matches!(
                        &tokens[after],
                        LineContainer::Token(t) if matches!(t.line_type, LineType::ListLine | LineType::SubjectOrListItemLine)
                    )
            }
        }
    }

    /// Match tables using imperative logic.
    ///
    /// A table is a subject line followed immediately by a container whose first
    /// non-blank line starts with a pipe character. This runs before the definition
    /// pattern (which matches the same `subject + container` shape) to ensure
    /// tables are detected by their content.
    fn match_table(
        tokens: &[LineContainer],
        start_idx: usize,
    ) -> Option<(PatternMatch, Range<usize>)> {
        use LineType::{SubjectLine, SubjectOrListItemLine};

        if start_idx >= tokens.len() {
            return None;
        }

        // Must start with a subject line
        let is_subject = matches!(
            &tokens[start_idx],
            LineContainer::Token(line) if matches!(line.line_type, SubjectLine | SubjectOrListItemLine)
        );
        if !is_subject {
            return None;
        }

        // Must be immediately followed by a container
        let content_idx = start_idx + 1;
        if content_idx >= tokens.len() {
            return None;
        }
        let container = &tokens[content_idx];
        if !matches!(container, LineContainer::Container { .. }) {
            return None;
        }

        // Container's first non-blank line must start with a pipe
        if !container_starts_with_pipe_row(container) {
            return None;
        }

        Some((
            PatternMatch::Table {
                subject_idx: 0,
                content_idx: 1,
            },
            start_idx..content_idx + 1,
        ))
    }

    /// Match verbatim blocks using imperative logic.
    ///
    /// Verbatim blocks consist of:
    /// 1. A subject line
    /// 2. Content that is either:
    ///    a) In a Container (inflow mode - content indented relative to subject)
    ///    b) Flat lines (fullwidth mode - content at fixed column, or groups)
    /// 3. A closing annotation marker (:: ... ::)
    ///
    /// This matcher handles both the original inflow case (subject + container + annotation)
    /// and the fullwidth case (subject + flat lines + annotation). To distinguish verbatim
    /// blocks from sessions followed by annotations, we require that either:
    /// - There's a Container immediately after the subject, OR
    /// - The closing annotation is at the SAME indentation as the subject
    ///
    /// Sessions have their title at the root level and content is indented. If we see
    /// a root-level annotation after a root-level subject with indented content between,
    /// that's NOT a verbatim block - it's a session followed by an annotation.
    fn match_verbatim_block(
        tokens: &[LineContainer],
        start_idx: usize,
    ) -> Option<(PatternMatch, Range<usize>)> {
        use LineType::{
            BlankLine, DataMarkerLine, DocumentStart, SubjectLine, SubjectOrListItemLine,
        };

        let len = tokens.len();
        if start_idx >= len {
            return None;
        }

        // Allow blank lines and DocumentStart before the subject to be consumed as part of this match
        let mut idx = start_idx;
        while idx < len {
            if let LineContainer::Token(line) = &tokens[idx] {
                if line.line_type == BlankLine || line.line_type == DocumentStart {
                    idx += 1;
                    continue;
                }
            }
            break;
        }

        if idx >= len {
            return None;
        }

        // Must start with a subject line
        let first_subject_idx = match &tokens[idx] {
            LineContainer::Token(line)
                if matches!(line.line_type, SubjectLine | SubjectOrListItemLine) =>
            {
                idx
            }
            _ => return None,
        };

        // Guard against absorbing a colon-terminated *paragraph* as this block's
        // subject (lex#814 §1/§3). If the very first subject owns no content of its
        // own — i.e. the next non-blank token is *another* subject — then the first
        // subject is a colon-terminated paragraph, not a verbatim/table subject.
        // Bail so the paragraph matcher claims it; the next `try_match` iteration
        // re-anchors the verbatim/table on the REAL subject (the one that owns its
        // container) and closes correctly.
        //
        // This must NOT fire for genuine verbatim shapes, whose first subject is
        // directly followed by:
        //   - a Container (inflow verbatim / multi-group verbatim), or
        //   - a DataMarkerLine (marker-form empty verbatim, e.g. `An image:` /
        //     `:: image ::`), or
        //   - flat prose lines (fullwidth mode).
        if let Some(LineContainer::Token(line)) = tokens[first_subject_idx + 1..]
            .iter()
            .find(|tc| !matches!(tc, LineContainer::Token(l) if l.line_type == BlankLine))
        {
            if matches!(line.line_type, SubjectLine | SubjectOrListItemLine) {
                return None;
            }
        }

        let mut cursor = first_subject_idx + 1;

        // Try to match one or more subject+content pairs followed by closing annotation
        // This loop handles verbatim groups: multiple subjects sharing one closing annotation
        loop {
            // Skip blank lines
            while cursor < len {
                if let LineContainer::Token(line) = &tokens[cursor] {
                    if line.line_type == BlankLine {
                        cursor += 1;
                        continue;
                    }
                }
                break;
            }

            if cursor >= len {
                return None;
            }

            // Check what we have at cursor
            match &tokens[cursor] {
                LineContainer::Container { .. } => {
                    // Found a container - this is potentially inflow mode verbatim content
                    // But we need to verify the pattern:
                    // - Verbatim: subject + container + (annotation OR another subject+container)
                    // - Session: subject + container + (other content)
                    cursor += 1;

                    // Skip blank lines after container
                    while cursor < len {
                        if let LineContainer::Token(line) = &tokens[cursor] {
                            if line.line_type == BlankLine {
                                cursor += 1;
                                continue;
                            }
                        }
                        break;
                    }

                    // After container, check what follows
                    if cursor >= len {
                        return None; // Container at end - not a verbatim block
                    }

                    match &tokens[cursor] {
                        LineContainer::Token(line) => {
                            if matches!(line.line_type, DataMarkerLine) {
                                // Container followed by closing annotation (:: label ::) - this IS verbatim!
                                // Continue loop to match it
                                continue;
                            }
                            if matches!(line.line_type, SubjectLine | SubjectOrListItemLine) {
                                // Container followed by another subject - this is a verbatim group!
                                // Continue loop to match more groups
                                continue;
                            }
                            // Container followed by something else - NOT a verbatim block
                            return None;
                        }
                        LineContainer::Container { .. } => {
                            // Container followed by another container - NOT verbatim pattern
                            return None;
                        }
                    }
                }
                LineContainer::Token(line) => {
                    if matches!(line.line_type, DataMarkerLine) {
                        // Found closing annotation (:: label ::) - success!
                        // A table is single-group: unlike verbatim, it cannot
                        // absorb preceding subject+content groups. If a shared
                        // `:: table ::` closer would build a table from a span
                        // whose first group is not pipe-row content, or from a
                        // multi-group span, back off so the normal
                        // definition/table matchers can claim their own blocks
                        // without dropping rows (lex#819).
                        if line_is_table_marker(line)
                            && !table_span_has_single_pipe_group(
                                tokens,
                                first_subject_idx + 1,
                                cursor,
                            )
                        {
                            return None;
                        }
                        // But only if we haven't mixed containers with flat content in a problematic way
                        return Some((
                            PatternMatch::VerbatimBlock {
                                subject_idx: first_subject_idx,
                                content_range: (first_subject_idx + 1)..cursor,
                                closing_idx: cursor,
                            },
                            start_idx..(cursor + 1),
                        ));
                    }

                    if matches!(line.line_type, SubjectLine | SubjectOrListItemLine) {
                        // Another subject - this is another group
                        cursor += 1;
                        continue;
                    }

                    // Any other flat token (paragraph line, etc.)
                    // This is fullwidth mode or group content
                    cursor += 1;
                }
            }
        }
    }
}

fn line_is_table_marker(line: &crate::lex::token::LineToken) -> bool {
    line.line_type == LineType::DataMarkerLine
        && line.source_tokens.iter().find_map(|token| match token {
            Token::Text(text) => Some(text.as_str()),
            _ => None,
        }) == Some("table")
}

fn table_span_has_single_pipe_group(
    tokens: &[LineContainer],
    content_start: usize,
    closing_idx: usize,
) -> bool {
    let mut saw_table_content = false;

    for token in &tokens[content_start..closing_idx] {
        match token {
            LineContainer::Token(line) if line.line_type == LineType::BlankLine => continue,
            LineContainer::Container { .. } if !saw_table_content => {
                if !container_starts_with_pipe_row(token) {
                    return false;
                }
                saw_table_content = true;
            }
            LineContainer::Token(line)
                if !saw_table_content
                    && matches!(
                        line.line_type,
                        LineType::SubjectLine | LineType::SubjectOrListItemLine
                    ) =>
            {
                return false;
            }
            LineContainer::Token(line) if !saw_table_content => {
                if !line_starts_with_pipe_row(line) {
                    return false;
                }
                saw_table_content = true;
            }
            LineContainer::Token(line)
                if matches!(
                    line.line_type,
                    LineType::SubjectLine | LineType::SubjectOrListItemLine
                ) =>
            {
                return false;
            }
            LineContainer::Container { .. } => return false,
            LineContainer::Token(_) => {}
        }
    }

    saw_table_content
}

fn line_starts_with_pipe_row(line: &crate::lex::token::LineToken) -> bool {
    for token in &line.source_tokens {
        match token {
            Token::Whitespace(_) | Token::Indentation | Token::Indent(_) => continue,
            Token::Text(text) => return text.starts_with('|'),
            _ => return false,
        }
    }
    false
}

/// Main recursive descent parser using the declarative grammar.
///
/// This is the entry point for parsing a sequence of tokens at any level.
/// It iteratively tries to match patterns and recursively descends into containers.
pub fn parse_with_declarative_grammar(
    tokens: Vec<LineContainer>,
    source: &str,
) -> Result<Vec<ParseNode>, String> {
    let tokens = fold_prose_continuations(tokens);
    // A `:: doc.untitled ::` among the leading document-level annotations
    // suppresses document-title promotion for the whole document (ADR-0002).
    // Detected here, at the one root entry point, so both parse paths (engine
    // and the `Parsing` transform stage) honor it without threading a flag in.
    let suppress_title = leading_untitled_marker(&tokens, source);
    parse_with_declarative_grammar_internal(tokens, source, true, true, suppress_title)
}

/// True when the leading document-level annotations (the run before the
/// synthetic `DocumentStart` marker) contain a `:: doc.untitled ::` marker.
/// This is the ADR-0002 no-title opt-out, honored by the parser itself.
fn leading_untitled_marker(tokens: &[LineContainer], source: &str) -> bool {
    for token in tokens {
        if let LineContainer::Token(line) = token {
            match line.line_type {
                // The metadata/content boundary — stop before the body so a
                // `doc.untitled` appearing later (as body content) is ignored.
                LineType::DocumentStart => break,
                LineType::DataMarkerLine if line_is_untitled_marker(line, source) => {
                    return true;
                }
                _ => {}
            }
        }
    }
    false
}

/// True when `line` is exactly the `:: doc.untitled ::` marker annotation
/// (marker form, no params, no body). Reconstructs the line's source text from
/// its token spans and matches the closed `:: label ::` shape.
fn line_is_untitled_marker(line: &crate::lex::token::LineToken, source: &str) -> bool {
    let (Some(start), Some(end)) = (
        line.token_spans.iter().map(|s| s.start).min(),
        line.token_spans.iter().map(|s| s.end).max(),
    ) else {
        return false;
    };
    let text = source.get(start..end).unwrap_or("").trim();
    text.strip_prefix("::")
        .and_then(|rest| rest.trim().strip_suffix("::"))
        .map(|inner| inner.trim() == "doc.untitled")
        .unwrap_or(false)
}

/// True when a line token is paragraph prose (a `ParagraphLine` or `DialogLine`).
fn is_prose_line(token: &crate::lex::token::LineToken) -> bool {
    matches!(
        token.line_type,
        LineType::ParagraphLine | LineType::DialogLine
    )
}

/// True when a run of line containers is *prose only* — it carries no structural
/// element (list, definition, table, annotation, verbatim) and so would parse to
/// nothing but paragraphs. A deeper-indented run of this shape is a hanging-indent
/// continuation of the preceding paragraph, not a nested block.
///
/// The check mirrors the paragraph boundaries the matcher uses:
///   - paragraph / dialog / blank lines are always prose;
///   - a plain subject line is prose *unless* it heads a container (subject +
///     container is a definition or table) — a lone trailing-colon line is just
///     prose;
///   - a nested container is prose only if its own contents are prose;
///   - anything starting with a list marker (`ListLine`, `SubjectOrListItemLine`)
///     or a data marker disqualifies the run — a run of those is a list, which
///     must keep its own structure, not be flattened into a paragraph.
fn is_prose_only_run(children: &[LineContainer]) -> bool {
    let mut idx = 0;
    while idx < children.len() {
        match &children[idx] {
            LineContainer::Token(t) => match t.line_type {
                LineType::ParagraphLine | LineType::DialogLine | LineType::BlankLine => {}
                LineType::SubjectLine => {
                    // Subject + container is a definition/table header, not prose.
                    if matches!(children.get(idx + 1), Some(LineContainer::Container { .. })) {
                        return false;
                    }
                }
                _ => return false,
            },
            LineContainer::Container { children: inner } => {
                if !is_prose_only_run(inner) {
                    return false;
                }
            }
        }
        idx += 1;
    }
    true
}

/// Append every leaf line of a container to `out`, dissolving nesting.
fn dissolve_prose_into(container: LineContainer, out: &mut Vec<LineContainer>) {
    match container {
        token @ LineContainer::Token(_) => out.push(token),
        LineContainer::Container { children } => {
            for child in children {
                dissolve_prose_into(child, out);
            }
        }
    }
}

/// Fold hanging-indent continuations back into their paragraph's level (lex#699).
///
/// The tokenizer turns *any* indent increase into a nested `LineContainer`, so a
/// paragraph whose continuation lines are merely more-indented (alignment / hanging
/// indent) gets split: the deeper lines land in a child container and are later
/// promoted to a *sibling* paragraph. The serializer then re-emits every paragraph
/// line at one normalized indent, and the two siblings re-parse as a single
/// paragraph — a silent semantic change across a format round-trip.
///
/// The grammar defines a paragraph as consecutive lines that stop only at list /
/// definition starts (grammar-core.lex `<paragraph>`), never at a bare indent
/// increase. So when a prose token is immediately followed by a *pure-prose*
/// container, we dissolve that container into the current level. Any blank lines
/// the tokenizer tucked inside the deeper container resurface as real separators at
/// this level — exactly what the formatter would emit — so paragraph breaks are
/// preserved while spurious indent-only splits are not.
///
/// Containers that carry structure (lists, definitions, annotations, tables) are
/// never dissolved, and a container not preceded by prose (an annotation/definition
/// body, an orphaned block) is left untouched.
fn fold_prose_continuations(children: Vec<LineContainer>) -> Vec<LineContainer> {
    let mut result: Vec<LineContainer> = Vec::with_capacity(children.len());

    for item in children {
        let dissolve = matches!(result.last(), Some(LineContainer::Token(t)) if is_prose_line(t))
            && matches!(&item, LineContainer::Container { children } if is_prose_only_run(children));

        if dissolve {
            dissolve_prose_into(item, &mut result);
            continue;
        }

        match item {
            LineContainer::Container { children } => result.push(LineContainer::Container {
                children: fold_prose_continuations(children),
            }),
            token => result.push(token),
        }
    }

    result
}

/// Internal parsing function with nesting level tracking
fn parse_with_declarative_grammar_internal(
    tokens: Vec<LineContainer>,
    source: &str,
    allow_sessions: bool,
    is_doc_start: bool,
    suppress_title: bool,
) -> Result<Vec<ParseNode>, String> {
    let mut items: Vec<ParseNode> = Vec::new();
    let mut idx = 0;

    while idx < tokens.len() {
        let (has_preceding_blank, has_preceding_boundary, prev_was_session) =
            if let Some(last_node) = items.last() {
                (
                    matches!(last_node.node_type, NodeType::BlankLineGroup),
                    // A node with children indicates we just closed a container; this counts as a boundary.
                    // DocumentStart and DocumentTitle also count as boundaries.
                    !last_node.children.is_empty()
                        || matches!(
                            last_node.node_type,
                            NodeType::DocumentStart | NodeType::DocumentTitle
                        ),
                    matches!(last_node.node_type, NodeType::Session),
                )
            } else {
                (false, false, false)
            };

        let is_first_item = idx == 0 && is_doc_start;
        if let Some((pattern, range)) = GrammarMatcher::try_match(
            &tokens,
            idx,
            allow_sessions,
            is_first_item,
            has_preceding_blank,
            has_preceding_boundary,
            prev_was_session,
            suppress_title,
        ) {
            let mut pending_nodes = Vec::new();

            if let PatternMatch::List {
                preceding_blank_range: Some(blank_range),
                ..
            } = &pattern
            {
                pending_nodes.push(blank_line_node_from_range(&tokens, blank_range.clone())?);
            }

            if let PatternMatch::Session {
                preceding_blank_range: Some(blank_range),
                ..
            } = &pattern
            {
                pending_nodes.push(blank_line_node_from_range(&tokens, blank_range.clone())?);
            }

            // Convert pattern to ParseNode
            // Sessions parse their children with allow_sessions=true to allow nested sessions
            // Other elements parse with allow_sessions=false to prevent sessions inside them
            let is_session = matches!(&pattern, PatternMatch::Session { .. });
            let item = convert_pattern_to_node(
                &tokens,
                &pattern,
                range.clone(),
                source,
                &move |children, src| {
                    parse_with_declarative_grammar_internal(children, src, is_session, false, false)
                },
            )?;
            pending_nodes.push(item);

            if let PatternMatch::List {
                trailing_blank_range: Some(blank_range),
                ..
            } = &pattern
            {
                pending_nodes.push(blank_line_node_from_range(&tokens, blank_range.clone())?);
            }

            items.extend(pending_nodes);
            idx = range.end;
        } else {
            // When no pattern matches, check if this is a Container (orphaned indented content).
            // Rather than silently dropping it, parse its children and promote them to this level.
            if let LineContainer::Container {
                children: inner, ..
            } = &tokens[idx]
            {
                if !inner.is_empty() {
                    let orphaned = parse_with_declarative_grammar_internal(
                        inner.clone(),
                        source,
                        allow_sessions,
                        false,
                        false,
                    )?;
                    items.extend(orphaned);
                }
            }
            idx += 1;
        }
    }

    Ok(items)
}

#[cfg(test)]
mod prose_continuation_tests {
    use super::*;
    use crate::lex::token::LineToken;

    fn line(line_type: LineType) -> LineContainer {
        LineContainer::Token(LineToken {
            source_tokens: vec![],
            token_spans: vec![],
            line_type,
        })
    }

    fn container(children: Vec<LineContainer>) -> LineContainer {
        LineContainer::Container { children }
    }

    #[test]
    fn prose_run_accepts_paragraph_and_lone_subject() {
        use LineType::*;
        // A hanging-indent continuation: paragraph lines, a blank, and a lone
        // trailing-colon subject line (not heading a container) are all prose.
        assert!(is_prose_only_run(&[
            line(ParagraphLine),
            line(SubjectLine),
            line(BlankLine),
        ]));
    }

    #[test]
    fn prose_run_rejects_list_markers() {
        use LineType::*;
        // A run of list-marker lines is a list, never prose — folding it into the
        // preceding paragraph would flatten real structure (lex#704 review).
        assert!(!is_prose_only_run(&[
            line(SubjectOrListItemLine),
            line(SubjectOrListItemLine),
        ]));
        assert!(!is_prose_only_run(&[line(ListLine)]));
        // A subject line that *heads* a container is a definition/table, not prose.
        assert!(!is_prose_only_run(&[
            line(SubjectLine),
            container(vec![line(ParagraphLine)]),
        ]));
    }
}

#[cfg(test)]
mod verbatim_anchor_tests {
    use super::*;
    use crate::lex::token::LineToken;

    fn line(line_type: LineType) -> LineContainer {
        LineContainer::Token(LineToken {
            source_tokens: vec![],
            token_spans: vec![],
            line_type,
        })
    }

    fn container(children: Vec<LineContainer>) -> LineContainer {
        LineContainer::Container { children }
    }

    // lex#814 §1/§3: a colon-terminated *paragraph* directly before a
    // verbatim/table subject must NOT be absorbed as the block's subject. The
    // matcher bails (returns None) so the paragraph matcher claims the colon
    // line and the next `try_match` iteration re-anchors the verbatim on the
    // REAL subject (the one that owns its container).
    #[test]
    fn colon_paragraph_before_verbatim_is_not_absorbed() {
        use LineType::*;
        // subject (colon-para) / blank / subject / container / closer
        let tokens = vec![
            line(SubjectLine),
            line(BlankLine),
            line(SubjectLine),
            container(vec![line(ParagraphLine)]),
            line(DataMarkerLine),
        ];
        assert!(
            GrammarMatcher::match_verbatim_block(&tokens, 0).is_none(),
            "first subject owns no container of its own — the block must not anchor on it"
        );
        // Re-anchored on the REAL subject at idx 2, it matches cleanly.
        assert!(GrammarMatcher::match_verbatim_block(&tokens, 2).is_some());
    }

    // A genuine multi-group verbatim (every group subject is followed by its own
    // container) must STILL match as one block — the guard must not fire here.
    #[test]
    fn multi_group_verbatim_still_matches() {
        use LineType::*;
        let tokens = vec![
            line(SubjectLine),
            container(vec![line(ParagraphLine)]),
            line(SubjectLine),
            container(vec![line(ParagraphLine)]),
            line(DataMarkerLine),
        ];
        assert!(GrammarMatcher::match_verbatim_block(&tokens, 0).is_some());
    }

    // Marker-form empty verbatim (`An image:` / `:: image ::`) — the first
    // subject is directly followed by the closing DataMarkerLine, not another
    // subject, so the guard must not fire.
    #[test]
    fn marker_form_empty_verbatim_still_matches() {
        use LineType::*;
        let tokens = vec![line(SubjectLine), line(DataMarkerLine)];
        assert!(GrammarMatcher::match_verbatim_block(&tokens, 0).is_some());
    }
}
