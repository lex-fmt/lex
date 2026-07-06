//! Re-insert attached annotations into the content stream for serialization
//! (lex#682).
//!
//! The parser's `attach_annotations` stage moves annotation nodes out of the
//! content stream onto each element's `annotations` field. The Lex serializer
//! walks the stream and so never emits them — silent content loss on format.
//!
//! This reverses that: each attached annotation is placed back into the content
//! stream at its original source position (now that annotations carry an
//! accurate `location` — see lex#693), so a re-parse re-attaches it to the same
//! element. An annotation whose location falls inside its owning element's span
//! was a container-end annotation and goes into that element's child stream;
//! otherwise it sat beside the element and goes in the parent stream. Each
//! stream is then sorted by source position, weaving annotations back among the
//! existing children (and their `BlankLineGroup`s) in source order.
//!
//! Document/root annotations are woven back into the root stream at their
//! original source position, then re-sorted with everything else, so each lands
//! where it was authored: a head annotation (the document-start rule) stays at
//! the head; a trailing one (the container-end rule) stays at the tail. Emitting
//! at the source position lets the same attachment rule re-fire on re-parse.
//! Forcing every doc annotation to the head instead broke the container-end case
//! and tripped a parser drop of head block-annotations (lex#696).
//!
//! `Table` annotations are left untouched — `Table::accept` already emits them.

use lex_core::lex::ast::elements::BlankLineGroup;
use lex_core::lex::ast::traits::AstNode;
use lex_core::lex::ast::{Annotation, ContentItem, Document};

/// Source-order sort key for a content item.
fn item_key(item: &ContentItem) -> (usize, usize, usize) {
    let r = item.range();
    (r.start.line, r.start.column, r.span.start)
}

/// Whether an annotation's recovered body is empty — a marker (`:: label ::`)
/// re-parses with a single empty `Paragraph` child, which would otherwise make
/// the serializer emit the block-open form (`:: label`). Treat such bodies as
/// absent so the marker round-trips.
fn child_is_empty(item: &ContentItem) -> bool {
    match item {
        ContentItem::BlankLineGroup(_) => true,
        ContentItem::Paragraph(p) => p.lines.iter().all(|l| match l {
            ContentItem::TextLine(tl) => tl.content.as_string().trim().is_empty(),
            _ => true,
        }),
        _ => false,
    }
}

/// Normalize an annotation's body. A marker (`:: label ::`) re-parses with a
/// lone empty-paragraph artifact; clear it so the marker emits without a body.
/// A genuine block body is left intact — including its blank-line separators
/// between paragraphs — and recursed into so nested annotations inline.
fn clean_annotation(ann: &mut Annotation) {
    let has_real_body = ann.children.iter().any(|c| !child_is_empty(c));
    if has_real_body {
        process_stream(ann.children.as_mut_vec());
    } else {
        ann.children.as_mut_vec().clear();
    }
}

pub fn inline_attached_annotations(doc: &mut Document) {
    let mut doc_anns = std::mem::take(&mut doc.annotations);
    doc_anns.append(&mut doc.root.annotations);

    // Drop the document-level annotations back into the root stream; the blank
    // groups around their original slots are still present (the parser removed
    // only the annotation item when it attached). `process_stream` then cleans
    // each body and re-sorts the whole stream by source position, weaving them
    // back where they were authored.
    //
    // Insert at the head, not the tail: a reader-built document (Markdown,
    // RFC-XML, …) has no real source positions — every item shares the default
    // (0, 0, 0) key — so the subsequent stable sort preserves insertion order.
    // Document-level annotations there (notably the ADR-0002 `:: doc.untitled ::`
    // no-title marker, which the parser only honors when it *leads*) must sit at
    // the head. For a lex-sourced document the positional sort governs instead,
    // so a genuine container-end annotation still sorts back to the tail.
    // Whether the document carried genuine document-level annotations (as
    // opposed to element-attached ones that merely bubble up here). Only those
    // need the structural head-blank below.
    let had_doc_anns = !doc_anns.is_empty();

    let children = doc.root.children.as_mut_vec();
    // Prepend the document-level annotations in a single pass. `insert(0, …)` in
    // a loop is O(M·N) — every insertion shifts the entire existing tail — so
    // build the head first and append the original stream (O(M + N)).
    let mut rest = std::mem::take(children);
    children.reserve(doc_anns.len() + rest.len());
    children.extend(doc_anns.into_iter().map(ContentItem::Annotation));
    children.append(&mut rest);
    process_stream(children);
    if had_doc_anns {
        ensure_blank_after_leading_marker_annotations(children);
    }
}

/// A leading marker annotation (`:: label ::`, empty body) at the document head
/// is document-level: the parser promotes it out of the body only when a blank
/// separates it from the first content block (the document-start rule). A
/// reader-built AST carries no `BlankLineGroup`, so without this the
/// serialized marker would sit flush against the first paragraph and re-parse
/// as an annotation *attached* to that paragraph rather than a document-level
/// one — the `:: doc.untitled ::` no-title marker would then be lost (ADR-0002).
///
/// Guarded by `had_doc_anns` at the call site so it never fires on a marker
/// that was element-attached in the source (`:: foo ::` with no following blank
/// attaches forward and must stay that way). Insert the structural blank only
/// when one is not already present, so a lex-sourced document (whose
/// `BlankLineGroup` survives the round-trip) stays byte-identical.
fn ensure_blank_after_leading_marker_annotations(items: &mut Vec<ContentItem>) {
    let mut end = 0;
    while matches!(items.get(end), Some(ContentItem::Annotation(a)) if a.children.is_empty()) {
        end += 1;
    }
    // A leading marker run exists and is followed by a non-blank content block.
    if end > 0 && end < items.len() && !matches!(items[end], ContentItem::BlankLineGroup(_)) {
        items.insert(
            end,
            ContentItem::BlankLineGroup(BlankLineGroup::new(1, Vec::new())),
        );
    }
}

/// Split annotations attached to one element: those whose source line falls
/// within the element's `[lo, hi]` span were container-end annotations and go
/// into the element's own `children`; the rest bubble to the parent stream.
fn route(
    anns: &mut Vec<Annotation>,
    lo: usize,
    hi: usize,
    children: &mut Vec<ContentItem>,
    bubble: &mut Vec<Annotation>,
) {
    for ann in std::mem::take(anns) {
        if (lo..=hi).contains(&ann.location.start.line) {
            children.push(ContentItem::Annotation(ann));
        } else {
            bubble.push(ann);
        }
    }
}

/// Rebuild a stream: pull attached annotations off each element, route them
/// inward (container-end) or to this level, recurse, then re-sort by source
/// order so annotations land where they were authored.
fn process_stream(items: &mut Vec<ContentItem>) {
    let mut bubble: Vec<Annotation> = Vec::new();

    for item in items.iter_mut() {
        match item {
            ContentItem::Session(s) => {
                let (lo, hi) = (s.location.start.line, s.location.end.line);
                route(
                    &mut s.annotations,
                    lo,
                    hi,
                    s.children.as_mut_vec(),
                    &mut bubble,
                );
                process_stream(s.children.as_mut_vec());
            }
            ContentItem::Definition(d) => {
                let (lo, hi) = (d.location.start.line, d.location.end.line);
                route(
                    &mut d.annotations,
                    lo,
                    hi,
                    d.children.as_mut_vec(),
                    &mut bubble,
                );
                process_stream(d.children.as_mut_vec());
            }
            ContentItem::ListItem(li) => {
                let (lo, hi) = (li.location.start.line, li.location.end.line);
                route(
                    &mut li.annotations,
                    lo,
                    hi,
                    li.children.as_mut_vec(),
                    &mut bubble,
                );
                process_stream(li.children.as_mut_vec());
            }
            ContentItem::List(l) => {
                let (lo, hi) = (l.location.start.line, l.location.end.line);
                route(
                    &mut l.annotations,
                    lo,
                    hi,
                    l.items.as_mut_vec(),
                    &mut bubble,
                );
                process_stream(l.items.as_mut_vec());
            }
            ContentItem::Paragraph(p) => bubble.append(&mut p.annotations),
            ContentItem::VerbatimBlock(v) => bubble.append(&mut v.annotations),
            // Annotation children are processed by `clean_annotation` below
            // (recursing once); do not recurse here too, or nested annotations
            // would be processed 2^depth times.
            // Table annotations are emitted by Table::accept; leave in place.
            _ => {}
        }
    }

    for ann in bubble {
        items.push(ContentItem::Annotation(ann));
    }
    // Clean every annotation now in this stream (bubbled-in plus any that were
    // already inline) — recurses into block bodies exactly once.
    for item in items.iter_mut() {
        if let ContentItem::Annotation(a) = item {
            clean_annotation(a);
        }
    }
    // Restore source order so reinserted annotations land where they were
    // authored — but only when every item shares one source origin. After
    // include expansion a stream can mix coordinate spaces from different files,
    // where a positional sort would scramble already-correct order (#682 review).
    let mixed_origin = items
        .iter()
        .any(|i| i.range().origin_path != items[0].range().origin_path);
    if !items.is_empty() && !mixed_origin {
        items.sort_by_key(item_key);
    }
}
