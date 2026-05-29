//! Coalesce adjacent blank-line groups before serialization (lex#686).
//!
//! The parser can split a single run of blank lines into multiple adjacent
//! `BlankLineGroup` nodes — notably after a list, where the terminator peels one
//! blank into its own group (`1` then `2` for three source blanks). The
//! serializer emits each group against an absolute newline floor
//! (`ensure_blank_lines`) rather than summing them, so two groups of `1` collapse
//! to a single blank. The result is non-idempotent: 3 blanks → 2 → 1.
//!
//! Merging adjacent `BlankLineGroup`s into one (summing counts) makes blank-run
//! emission a fixed point: any split that totals N serializes to `min(N, max)`,
//! and re-parsing that output (however it re-splits) coalesces back to the same
//! total. Purely a normalization — adjacent blank lines are one run regardless
//! of node boundaries.

use lex_core::lex::ast::{ContentItem, Document};

pub fn coalesce_blank_line_groups(doc: &mut Document) {
    coalesce(doc.root.children.as_mut_vec());
}

fn coalesce(items: &mut Vec<ContentItem>) {
    // Recurse into every child container first.
    for item in items.iter_mut() {
        match item {
            ContentItem::Session(s) => coalesce(s.children.as_mut_vec()),
            ContentItem::Definition(d) => coalesce(d.children.as_mut_vec()),
            ContentItem::ListItem(li) => coalesce(li.children.as_mut_vec()),
            ContentItem::List(l) => coalesce(l.items.as_mut_vec()),
            ContentItem::Annotation(a) => coalesce(a.children.as_mut_vec()),
            ContentItem::Table(t) => {
                // Mirror Table::accept: cell children, attached annotations,
                // and the footnote list are all traversed on serialization.
                for row in t.header_rows.iter_mut().chain(t.body_rows.iter_mut()) {
                    for cell in &mut row.cells {
                        coalesce(cell.children.as_mut_vec());
                    }
                }
                for ann in &mut t.annotations {
                    coalesce(ann.children.as_mut_vec());
                }
                if let Some(list) = &mut t.footnotes {
                    coalesce(list.items.as_mut_vec());
                }
            }
            _ => {}
        }
    }

    // Merge runs of adjacent BlankLineGroups in this stream.
    let mut out: Vec<ContentItem> = Vec::with_capacity(items.len());
    for item in std::mem::take(items) {
        if let ContentItem::BlankLineGroup(b) = &item {
            if let Some(ContentItem::BlankLineGroup(prev)) = out.last_mut() {
                prev.count += b.count;
                continue;
            }
        }
        out.push(item);
    }
    *items = out;
}
