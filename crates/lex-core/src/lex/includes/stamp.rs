//! Origin stamping.
//!
//! Walk every node in a [`Document`] and set `Range.origin_path` on each
//! `.location` field so downstream code (file-ref resolution, diagnostics,
//! LSP goto) can report locations against the authoring file. The walk
//! stamps the *block-level* `.location` fields; finer-grained inline ranges
//! land separately when file-ref resolution starts consulting them.

use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::elements::session::Session;
use crate::lex::ast::Document;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) fn stamp_doc(doc: &mut Document, origin: &Arc<PathBuf>) {
    if let Some(title) = doc.title.as_mut() {
        title.location.origin_path = Some(Arc::clone(origin));
    }
    for ann in doc.annotations.iter_mut() {
        stamp_annotation(ann, origin);
    }
    stamp_session(&mut doc.root, origin);
}

fn stamp_session(s: &mut Session, origin: &Arc<PathBuf>) {
    s.location.origin_path = Some(Arc::clone(origin));
    if let Some(loc) = s.title.location.as_mut() {
        loc.origin_path = Some(Arc::clone(origin));
    }
    for ann in s.annotations.iter_mut() {
        stamp_annotation(ann, origin);
    }
    for item in s.children.as_mut_vec().iter_mut() {
        stamp_item(item, origin);
    }
}

fn stamp_annotation(
    a: &mut crate::lex::ast::elements::annotation::Annotation,
    origin: &Arc<PathBuf>,
) {
    a.location.origin_path = Some(Arc::clone(origin));
    a.data.location.origin_path = Some(Arc::clone(origin));
    for item in a.children.as_mut_vec().iter_mut() {
        stamp_item(item, origin);
    }
}

fn stamp_item(item: &mut ContentItem, origin: &Arc<PathBuf>) {
    match item {
        ContentItem::Session(s) => stamp_session(s, origin),
        ContentItem::Annotation(a) => stamp_annotation(a, origin),
        ContentItem::Paragraph(p) => {
            p.location.origin_path = Some(Arc::clone(origin));
            for ann in p.annotations.iter_mut() {
                stamp_annotation(ann, origin);
            }
            for line in p.lines.iter_mut() {
                stamp_item(line, origin);
            }
        }
        ContentItem::List(l) => {
            l.location.origin_path = Some(Arc::clone(origin));
            for li in l.items.as_mut_vec().iter_mut() {
                stamp_item(li, origin);
            }
        }
        ContentItem::ListItem(li) => {
            li.location.origin_path = Some(Arc::clone(origin));
            for ann in li.annotations.iter_mut() {
                stamp_annotation(ann, origin);
            }
            for child in li.children.as_mut_vec().iter_mut() {
                stamp_item(child, origin);
            }
        }
        ContentItem::Definition(d) => {
            d.location.origin_path = Some(Arc::clone(origin));
            for ann in d.annotations.iter_mut() {
                stamp_annotation(ann, origin);
            }
            for child in d.children.as_mut_vec().iter_mut() {
                stamp_item(child, origin);
            }
        }
        ContentItem::VerbatimBlock(v) => {
            v.location.origin_path = Some(Arc::clone(origin));
        }
        ContentItem::VerbatimLine(vl) => {
            vl.location.origin_path = Some(Arc::clone(origin));
        }
        ContentItem::Table(t) => {
            t.location.origin_path = Some(Arc::clone(origin));
        }
        ContentItem::TextLine(tl) => {
            tl.location.origin_path = Some(Arc::clone(origin));
        }
        ContentItem::BlankLineGroup(b) => {
            b.location.origin_path = Some(Arc::clone(origin));
        }
    }
}
