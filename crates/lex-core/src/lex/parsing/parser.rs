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
use crate::lex::token::{LineContainer, LineType};
use regex::Regex;
use std::ops::Range;

mod builder;
mod grammar;

use builder::{blank_line_node_from_range, convert_pattern_to_node, PatternMatch};
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
    fn try_match(
        tokens: &[LineContainer],
        start_idx: usize,
        allow_sessions: bool,
        is_first_item: bool,
        has_preceding_blank: bool,
        has_preceding_boundary: bool,
        prev_was_session: bool,
    ) -> Option<(PatternMatch, Range<usize>)> {
        if start_idx >= tokens.len() {
            return None;
        }

        // Try verbatim block first (requires special imperative matching logic)
        if let Some(result) = Self::match_verbatim_block(tokens, start_idx) {
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
                        "list_no_blank" => {
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
                            // No container lookahead needed: the subtitle variant
                            // consumed two lines (title + subtitle) before blank lines.
                            // A session only has one line before blank + container, so
                            // the presence of a container after the blank is NOT ambiguous
                            // here — it's the document body, not a session body.
                            // Match: DocumentStart(0) + title(1) + subtitle(2) + blank lines
                            PatternMatch::DocumentTitle {
                                title_idx: 1,
                                subtitle_idx: Some(2),
                            }
                        }
                        "document_title" => {
                            // Imperative negative lookahead: not followed by container
                            let next_idx = start_idx + consumed_count;
                            if next_idx < tokens.len()
                                && matches!(&tokens[next_idx], LineContainer::Container { .. })
                            {
                                // Followed by container — this is a session, not a title
                                continue;
                            }
                            // Match is: DocumentStart(0) + title line(1) + blank lines
                            PatternMatch::DocumentTitle {
                                title_idx: 1,
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
            AnnotationStartLine, BlankLine, DocumentStart, SubjectLine, SubjectOrListItemLine,
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
                            if matches!(line.line_type, AnnotationStartLine) {
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
                    if matches!(line.line_type, AnnotationStartLine) {
                        // Found closing annotation (:: label ::) - success!
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

/// Main recursive descent parser using the declarative grammar.
///
/// This is the entry point for parsing a sequence of tokens at any level.
/// It iteratively tries to match patterns and recursively descends into containers.
pub fn parse_with_declarative_grammar(
    tokens: Vec<LineContainer>,
    source: &str,
) -> Result<Vec<ParseNode>, String> {
    parse_with_declarative_grammar_internal(tokens, source, true, true)
}

/// Internal parsing function with nesting level tracking
fn parse_with_declarative_grammar_internal(
    tokens: Vec<LineContainer>,
    source: &str,
    allow_sessions: bool,
    is_doc_start: bool,
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
                    parse_with_declarative_grammar_internal(children, src, is_session, false)
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
                    )?;
                    items.extend(orphaned);
                }
            }
            idx += 1;
        }
    }

    Ok(items)
}
