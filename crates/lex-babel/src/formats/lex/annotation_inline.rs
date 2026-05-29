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
//! Document/root annotations are emitted at the document head, each followed by
//! a blank line, so they re-attach to the `Document` (the document-start rule),
//! regardless of where they sat in the source.
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
    doc_anns.sort_by_key(|a| (a.location.start.line, a.location.start.column));

    process_stream(doc.root.children.as_mut_vec());

    if !doc_anns.is_empty() {
        let children = doc.root.children.as_mut_vec();
        let mut head: Vec<ContentItem> = Vec::with_capacity(doc_anns.len() * 2 + children.len());
        for mut ann in doc_anns {
            clean_annotation(&mut ann);
            head.push(ContentItem::Annotation(ann));
            head.push(ContentItem::BlankLineGroup(BlankLineGroup {
                count: 1,
                source_tokens: Vec::new(),
                location: Default::default(),
            }));
        }
        head.append(children);
        *children = head;
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
