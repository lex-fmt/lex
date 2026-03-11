//! Citation parsing for inline references.
//!
//! Handles parsing of academic citations with the format `[@key1; @key2, pp. 45-46]`.
//! Supports multiple citation keys and page locators.

use crate::lex::ast::elements::inlines::{CitationData, CitationLocator, PageFormat, PageRange};

/// Parse citation data from the content inside `[@...]` brackets.
pub(super) fn parse_citation_data(content: &str) -> Option<CitationData> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (keys_segment, locator_segment) = split_locator_segment(trimmed);
    let keys = parse_citation_keys(keys_segment)?;
    let locator = locator_segment.and_then(parse_citation_locator);

    Some(CitationData { keys, locator })
}

/// Split citation content into keys segment and optional locator segment.
///
/// Finds the last comma that starts a page locator (e.g., ", pp. 45").
fn split_locator_segment(content: &str) -> (&str, Option<&str>) {
    let mut locator_index = None;
    let mut search_start = 0;
    while let Some(pos) = content[search_start..].find(',') {
        let idx = search_start + pos;
        let tail = content[idx + 1..].trim_start();
        if looks_like_locator_start(tail) {
            locator_index = Some(idx);
        }
        search_start = idx + 1;
    }

    if let Some(idx) = locator_index {
        let keys = content[..idx].trim_end();
        let locator = content[idx + 1..].trim_start();
        if locator.is_empty() {
            (keys, None)
        } else {
            (keys, Some(locator))
        }
    } else {
        (content, None)
    }
}

/// Check if text starts with a page locator pattern (p. or pp.).
fn looks_like_locator_start(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("pp") {
        lower
            .chars()
            .nth(2)
            .is_some_and(|ch| ch == '.' || ch.is_whitespace() || ch.is_ascii_digit())
    } else if lower.starts_with('p') {
        lower
            .chars()
            .nth(1)
            .is_some_and(|ch| ch == '.' || ch.is_whitespace() || ch.is_ascii_digit())
    } else {
        false
    }
}

/// Parse citation keys from the keys segment.
///
/// Supports both comma and semicolon separators. Strips leading `@` from keys.
fn parse_citation_keys(segment: &str) -> Option<Vec<String>> {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return None;
    }

    let delimiter = if trimmed.contains(';') { ';' } else { ',' };
    let mut keys = Vec::new();
    for chunk in trimmed.split(delimiter) {
        let mut key = chunk.trim();
        if key.is_empty() {
            continue;
        }
        if let Some(stripped) = key.strip_prefix('@') {
            key = stripped.trim();
        }
        if key.is_empty() {
            continue;
        }
        keys.push(key.to_string());
    }

    if keys.is_empty() {
        None
    } else {
        Some(keys)
    }
}

/// Parse a citation locator (page specification).
///
/// Examples: "p.45", "pp. 45-46", "p. 1,2,3"
fn parse_citation_locator(text: &str) -> Option<CitationLocator> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let (format, rest) = if lower.starts_with("pp") {
        (PageFormat::Pp, trimmed[2..].trim_start())
    } else if lower.starts_with('p') {
        (PageFormat::P, trimmed[1..].trim_start())
    } else {
        return None;
    };

    let rest = rest
        .strip_prefix('.')
        .map(|r| r.trim_start())
        .unwrap_or(rest);
    if rest.is_empty() {
        return None;
    }
    let ranges = parse_page_ranges(rest);
    if ranges.is_empty() {
        return None;
    }

    Some(CitationLocator {
        format,
        ranges,
        raw: trimmed.to_string(),
    })
}

/// Parse page ranges from comma-separated page specifications.
///
/// Supports single pages (45) and ranges (45-46).
fn parse_page_ranges(text: &str) -> Vec<PageRange> {
    let mut ranges = Vec::new();
    for part in text.split(',') {
        let segment = part.trim();
        if segment.is_empty() {
            continue;
        }
        if let Some(idx) = segment.find('-') {
            let start = segment[..idx].trim();
            let end = segment[idx + 1..].trim();
            if let Ok(start_num) = start.parse::<u32>() {
                let end_num = if end.is_empty() {
                    None
                } else {
                    match end.parse::<u32>().ok() {
                        Some(value) => Some(value),
                        None => continue,
                    }
                };
                ranges.push(PageRange {
                    start: start_num,
                    end: end_num,
                });
            }
        } else if let Ok(number) = segment.parse::<u32>() {
            ranges.push(PageRange {
                start: number,
                end: None,
            });
        }
    }
    ranges
}
