use crate::inline::extract_references;
use crate::reference_targets::{
    targets_from_annotation, targets_from_definition, targets_from_reference_type,
    targets_from_session, ReferenceTarget,
};
use crate::utils::{
    find_annotation_at_position, find_definition_at_position, find_definitions_by_subject,
    find_session_at_position, find_sessions_by_identifier, for_each_text_content,
    reference_at_position,
};
use lex_core::lex::ast::traits::AstNode;
use lex_core::lex::ast::{Document, Position, Range};

pub fn find_references(
    document: &Document,
    position: Position,
    include_declaration: bool,
) -> Vec<Range> {
    let targets = determine_targets(document, position);
    if targets.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    if include_declaration {
        ranges.extend(declaration_ranges(document, &targets));
    }
    ranges.extend(reference_occurrences(document, &targets));
    dedup_ranges(&mut ranges);
    ranges
}

fn determine_targets(document: &Document, position: Position) -> Vec<ReferenceTarget> {
    if let Some(reference) = reference_at_position(document, position) {
        let targets = targets_from_reference_type(&reference.reference_type);
        if !targets.is_empty() {
            return targets;
        }
    }

    if let Some(annotation) = find_annotation_at_position(document, position) {
        let targets = targets_from_annotation(annotation);
        if !targets.is_empty() {
            return targets;
        }
    }

    if let Some(definition) = find_definition_at_position(document, position) {
        let targets = targets_from_definition(definition);
        if !targets.is_empty() {
            return targets;
        }
    }

    if let Some(session) = find_session_at_position(document, position) {
        let targets = targets_from_session(session);
        if !targets.is_empty() {
            return targets;
        }
    }

    Vec::new()
}

fn declaration_ranges(document: &Document, targets: &[ReferenceTarget]) -> Vec<Range> {
    let mut ranges = Vec::new();
    for target in targets {
        match target {
            ReferenceTarget::AnnotationLabel(label) => {
                for annotation in document.find_annotations_by_label(label) {
                    ranges.push(annotation.header_location().clone());
                }
            }
            ReferenceTarget::CitationKey(key) => {
                let annotations = document.find_annotations_by_label(key);
                if annotations.is_empty() {
                    ranges.extend(definition_ranges(document, key));
                } else {
                    for annotation in annotations {
                        ranges.push(annotation.header_location().clone());
                    }
                }
            }
            ReferenceTarget::DefinitionSubject(subject) => {
                ranges.extend(definition_ranges(document, subject));
            }
            ReferenceTarget::Session(identifier) => {
                for session in find_sessions_by_identifier(document, identifier) {
                    if let Some(header) = session.header_location() {
                        ranges.push(header.clone());
                    } else {
                        ranges.push(session.range().clone());
                    }
                }
            }
        }
    }
    ranges
}

fn definition_ranges(document: &Document, subject: &str) -> Vec<Range> {
    find_definitions_by_subject(document, subject)
        .into_iter()
        .map(|definition| {
            definition
                .header_location()
                .cloned()
                .unwrap_or_else(|| definition.range().clone())
        })
        .collect()
}

/// Does `target` resolve to at least one declaration anywhere in
/// `document`? This is the boolean form of [`declaration_ranges`] — same
/// resolution rules (case-insensitive throughout, citation keys fall
/// back from annotation labels to definition subjects) — used by the
/// opt-in `check --references` pass to decide whether a reference is
/// dangling. Because it runs over the merged tree it resolves
/// bidirectionally: the target may live in any included fragment or in
/// the master, regardless of where the reference sits.
pub fn target_resolves(document: &Document, target: &ReferenceTarget) -> bool {
    // Trim each query so resolution matches `reference_matches`'
    // trimmed, case-insensitive comparison and never false-positives on
    // an untrimmed target. (The `find_*` helpers normalize internally
    // via `normalize_key`; trimming here makes the contract explicit and
    // self-contained rather than relying on the callee.)
    match target {
        ReferenceTarget::AnnotationLabel(label) => annotation_label_exists(document, label.trim()),
        ReferenceTarget::CitationKey(key) => {
            let trimmed = key.trim();
            annotation_label_exists(document, trimmed)
                || !find_definitions_by_subject(document, trimmed).is_empty()
        }
        ReferenceTarget::DefinitionSubject(subject) => {
            !find_definitions_by_subject(document, subject.trim()).is_empty()
        }
        ReferenceTarget::Session(identifier) => {
            !find_sessions_by_identifier(document, identifier.trim()).is_empty()
        }
    }
}

/// Case-insensitive existence check for an annotation label anywhere in
/// the document (document-level annotations plus every annotation nested
/// in the root session). `find_annotations_by_label` matches exactly;
/// reference resolution is case-insensitive (see [`reference_matches`]),
/// so this scans rather than delegating.
fn annotation_label_exists(document: &Document, label: &str) -> bool {
    let needle = label.trim();
    document
        .annotations()
        .iter()
        .chain(document.root.iter_annotations_recursive())
        .any(|ann| ann.data.label.value.trim().eq_ignore_ascii_case(needle))
}

pub fn reference_occurrences(document: &Document, targets: &[ReferenceTarget]) -> Vec<Range> {
    let mut matches = Vec::new();
    for_each_text_content(document, &mut |text| {
        for reference in extract_references(text) {
            if targets
                .iter()
                .any(|target| reference_matches(&reference.reference_type, target))
            {
                matches.push(reference.range);
            }
        }
    });
    matches
}

fn reference_matches(
    reference: &lex_core::lex::inlines::ReferenceType,
    target: &ReferenceTarget,
) -> bool {
    use lex_core::lex::inlines::ReferenceType;
    match (reference, target) {
        (
            ReferenceType::AnnotationReference { label },
            ReferenceTarget::AnnotationLabel(expected),
        ) => label.eq_ignore_ascii_case(expected),
        (ReferenceType::FootnoteNumber { number }, ReferenceTarget::AnnotationLabel(expected)) => {
            expected == &number.to_string()
        }
        (ReferenceType::Citation(data), ReferenceTarget::CitationKey(key)) => data
            .keys
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(key)),
        (ReferenceType::Citation(data), ReferenceTarget::AnnotationLabel(label)) => data
            .keys
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(label)),
        (ReferenceType::General { target: value }, ReferenceTarget::DefinitionSubject(subject)) => {
            normalize(value) == normalize(subject)
        }
        (
            ReferenceType::ToCome {
                identifier: Some(value),
            },
            ReferenceTarget::DefinitionSubject(subject),
        ) => normalize(value) == normalize(subject),
        (ReferenceType::Session { target }, ReferenceTarget::Session(identifier)) => {
            target.eq_ignore_ascii_case(identifier)
        }
        _ => false,
    }
}

fn normalize(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

fn dedup_ranges(ranges: &mut Vec<Range>) {
    ranges.sort_by_key(|range| (range.span.start, range.span.end));
    ranges.dedup_by(|a, b| a.span == b.span && a.start == b.start && a.end == b.end);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::lex::parsing;

    fn fixture() -> (Document, String) {
        let source = r#":: test.note ::
    Something.

Cache:
    Definition body.

1. Intro

    First reference [Cache].
    Second reference [Cache] and annotation [::note].
"#;
        let document = parsing::parse_document(source).expect("fixture parses");
        (document, source.to_string())
    }

    fn position_of(source: &str, needle: &str) -> Position {
        let offset = source
            .find(needle)
            .unwrap_or_else(|| panic!("needle not found: {needle}"));
        let mut line = 0;
        let mut col = 0;
        for ch in source[..offset].chars() {
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += ch.len_utf8();
            }
        }
        Position::new(line, col)
    }

    #[test]
    fn finds_references_from_usage() {
        let (document, source) = fixture();
        let position = position_of(&source, "Cache]");
        let ranges = find_references(&document, position, false);
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn finds_references_from_definition() {
        let (document, source) = fixture();
        let position = position_of(&source, "Cache:");
        let ranges = find_references(&document, position, false);
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn includes_declaration_when_requested() {
        let (document, source) = fixture();
        let position = position_of(&source, "Cache:");
        let ranges = find_references(&document, position, true);
        assert!(ranges.len() >= 3);
        let definition_header = document
            .root
            .children
            .iter()
            .find_map(|item| match item {
                lex_core::lex::ast::ContentItem::Definition(def) => def
                    .header_location()
                    .cloned()
                    .or_else(|| Some(def.range().clone())),
                _ => None,
            })
            .expect("definition header available");
        assert!(ranges.contains(&definition_header));
    }

    #[test]
    fn finds_annotation_references() {
        let (document, source) = fixture();
        let position = position_of(&source, "::note]");
        let ranges = find_references(&document, position, false);
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].contains(position));
    }
}
