//! Shared Skeleton reducer + Faithfulness comparator (lex#681, lex#781).
//!
//! `canon(&Document) -> Canon` is the **Skeleton reducer**: it projects an AST
//! down to what the Faithfulness invariant compares — Document Title + block
//! structure + inline content — with everything a formatter or serializer is
//! *allowed* to change quotiented out (source ranges, `BlankLineGroup`s, marker
//! spelling, trailing whitespace, annotation-label spelling, table-cell padding).
//! See CONTEXT.md ("Skeleton", "Faithfulness").
//!
//! It lives here, shared, because two suites compare Skeletons:
//!   - `format_invariants` — `canon(parse(D)) == canon(parse(format(D)))`
//!     (formatter semantic preservation).
//!   - conversion faithfulness — `canon(read(src)) == canon(reparse(serialize(read(src))))`
//!     (a reader's document survives serialize→reparse; see `check_faithful`).
//!
//! We deliberately do NOT reuse `AstSnapshot`: its `label` comes from
//! `display_label()`, which truncates text at 50 chars and omits table cells /
//! footnotes — blind to exactly the table/footnote/reference content this
//! comparison targets.
//!
//! What `canon` quotients out (everything a formatter/serializer may change):
//!   - source ranges / offsets            (never represented in Canon)
//!   - blank-line groups                  (dropped: purely presentational separators)
//!   - list/marker *spelling*             (dropped; decoration *style* kept)
//!   - trailing whitespace                (every text field trimmed)
//!   - annotation label *spelling*        (canonical `.value` kept)
//!   - table cell padding                 (cell text trimmed)

use lex_babel::format::Format;
use lex_babel::formats::lex::LexFormat;
use lex_core::lex::ast::elements::inlines::{InlineNode, ReferenceType};
use lex_core::lex::ast::elements::sequence_marker::DecorationStyle;
use lex_core::lex::ast::{Annotation, ContentItem, Document, TableRow, TextContent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Canon {
    Document {
        title: Option<String>,
        subtitle: Option<String>,
        annotations: Vec<Canon>,
        children: Vec<Canon>,
    },
    Session {
        title: String,
        style: Option<String>,
        annotations: Vec<Canon>,
        children: Vec<Canon>,
    },
    Paragraph {
        text: String,
        refs: Vec<String>,
        annotations: Vec<Canon>,
    },
    List {
        style: Option<String>,
        annotations: Vec<Canon>,
        items: Vec<Canon>,
    },
    ListItem {
        text: String,
        refs: Vec<String>,
        annotations: Vec<Canon>,
        children: Vec<Canon>,
    },
    Definition {
        subject: String,
        annotations: Vec<Canon>,
        children: Vec<Canon>,
    },
    Verbatim {
        subject: String,
        closing_label: String,
        lines: Vec<String>,
    },
    Annotation {
        label: String,
        params: Vec<(String, String)>,
        children: Vec<Canon>,
    },
    Table {
        subject: String,
        rows: Vec<CanonRow>,
        footnotes: Vec<Canon>,
        annotations: Vec<Canon>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonRow {
    header: bool,
    cells: Vec<CanonCell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonCell {
    text: String,
    refs: Vec<String>,
    colspan: usize,
    rowspan: usize,
    align: String,
}

/// Stable tag for a reference's classified type. Captures *which* of the 8
/// reference forms the parser resolved, plus the target/key, so the suite
/// asserts reference-type preservation explicitly (lex#681 ask).
fn ref_tag(ty: &ReferenceType) -> String {
    match ty {
        ReferenceType::ToCome { identifier } => {
            format!("tk:{}", identifier.as_deref().unwrap_or(""))
        }
        ReferenceType::Citation(data) => format!("cite:{}", data.keys.join(",")),
        ReferenceType::AnnotationReference { label } => format!("annref:{label}"),
        ReferenceType::FootnoteNumber { number } => format!("foot:{number}"),
        ReferenceType::Session { target } => format!("session:{target}"),
        ReferenceType::Url { target } => format!("url:{target}"),
        ReferenceType::File { target } => format!("file:{target}"),
        ReferenceType::General { target } => format!("general:{target}"),
        ReferenceType::NotSure => "notsure".to_string(),
    }
}

/// Extract classified references from a text field by parsing its inlines.
/// `inlines()` may be lazy/unpopulated on a freshly-parsed document, so parse
/// on a clone.
fn refs_in(tc: &TextContent) -> Vec<String> {
    let mut tc = tc.clone();
    tc.inlines_or_parse()
        .iter()
        .filter_map(|node| match node {
            InlineNode::Reference { data, .. } => Some(ref_tag(&data.reference_type)),
            _ => None,
        })
        .collect()
}

fn style_tag(style: DecorationStyle) -> String {
    format!("{style:?}")
}

fn canon_annotations(anns: &[Annotation]) -> Vec<Canon> {
    anns.iter().map(canon_annotation).collect()
}

fn canon_annotation(ann: &Annotation) -> Canon {
    // A marker annotation (`:: label ::`, no body) re-parses with a lone
    // empty-Paragraph artifact, while a reader builds it with no children at
    // all. Both are the same marker, so drop empty-paragraph children here —
    // the Faithfulness analog of the serializer's `clean_annotation`. Without
    // this a reader-emitted marker (e.g. the ADR-0002 `:: doc.untitled ::`)
    // would never compare equal to its own re-parse.
    let children: Vec<Canon> = canon_items(ann.children.iter())
        .into_iter()
        .filter(|c| !is_empty_paragraph(c))
        .collect();
    Canon::Annotation {
        // `.value` is the canonical label; the formatter re-emits the source
        // spelling form, which reparses back to the same canonical value.
        label: ann.data.label.value.clone(),
        params: ann
            .data
            .parameters
            .iter()
            .map(|p| (p.key.clone(), p.value.clone()))
            .collect(),
        children,
    }
}

/// True for a content-free `Canon::Paragraph` — the empty-body artifact a
/// marker annotation re-parses into. See [`canon_annotation`].
fn is_empty_paragraph(c: &Canon) -> bool {
    matches!(
        c,
        Canon::Paragraph { text, refs, annotations }
            if text.is_empty() && refs.is_empty() && annotations.is_empty()
    )
}

fn canon_items<'a, I: Iterator<Item = &'a ContentItem>>(items: I) -> Vec<Canon> {
    items.filter_map(canon_item).collect()
}

fn canon_row(row: &TableRow) -> CanonRow {
    CanonRow {
        header: row.cells.iter().any(|c| c.header),
        cells: row
            .cells
            .iter()
            .map(|c| CanonCell {
                text: c.content.as_string().trim().to_string(),
                refs: refs_in(&c.content),
                colspan: c.colspan,
                rowspan: c.rowspan,
                align: format!("{:?}", c.align),
            })
            .collect(),
    }
}

/// Project a `ContentItem` to its semantic Canon, or `None` for nodes that are
/// purely presentational (blank-line groups) or already folded into a parent
/// (loose `TextLine`s are handled inside `Paragraph`).
fn canon_item(item: &ContentItem) -> Option<Canon> {
    Some(match item {
        ContentItem::BlankLineGroup(_) => return None,
        // A bare TextLine outside a Paragraph: treat as a one-line paragraph.
        ContentItem::TextLine(tl) => Canon::Paragraph {
            text: tl.content.as_string().trim_end().to_string(),
            refs: refs_in(&tl.content),
            annotations: Vec::new(),
        },
        ContentItem::Paragraph(p) => {
            let mut text = String::new();
            let mut refs = Vec::new();
            for line in &p.lines {
                if let ContentItem::TextLine(tl) = line {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(tl.content.as_string().trim_end());
                    refs.extend(refs_in(&tl.content));
                }
            }
            Canon::Paragraph {
                text,
                refs,
                annotations: canon_annotations(&p.annotations),
            }
        }
        ContentItem::Session(s) => Canon::Session {
            title: s.title.as_string().trim_end().to_string(),
            style: s.marker.as_ref().map(|m| style_tag(m.style)),
            annotations: canon_annotations(&s.annotations),
            children: canon_items(s.children.iter()),
        },
        ContentItem::List(l) => Canon::List {
            style: l.marker.as_ref().map(|m| style_tag(m.style)),
            annotations: canon_annotations(&l.annotations),
            items: canon_items(l.items.iter()),
        },
        ContentItem::ListItem(li) => {
            // Project *all* text elements (not just the first) so multi-line
            // item content is covered; collect refs from each.
            let mut text = li
                .text
                .iter()
                .map(|t| t.as_string().trim_end())
                .collect::<Vec<_>>()
                .join("\n");
            let mut refs: Vec<String> = li.text.iter().flat_map(refs_in).collect();
            let mut children = canon_items(li.children.iter());

            // A foreign reader (comrak) builds a list item as
            // `{ text: "", children: [Paragraph, …] }` — the item's lead text
            // lives in a wrapping Paragraph, not in `text`. lex has no such
            // wrapper: its lead text sits on the marker line, so the serializer
            // hoists that leading Paragraph onto the `- text` marker line
            // (lex#798), and it re-parses as `{ text: "…", children: [] }`. Fold
            // the two representations together here — when the item carries no
            // marker-line text but leads with a Paragraph, treat that
            // paragraph's text as the item text. This is the Faithfulness analog
            // of the serializer's marker-line hoist, exactly as
            // `canon_annotation` mirrors the serializer's `clean_annotation`. It
            // only fires on the empty-text (reader-built) shape; a Lex-sourced
            // item always has marker-line text, so its Skeleton is unchanged.
            if text.is_empty() {
                if let Some(Canon::Paragraph {
                    text: lead,
                    refs: lead_refs,
                    annotations,
                }) = children.first()
                {
                    if annotations.is_empty() {
                        text = lead.clone();
                        refs = lead_refs.clone();
                        children.remove(0);
                    }
                }
            }

            Canon::ListItem {
                text,
                refs,
                annotations: canon_annotations(&li.annotations),
                children,
            }
        }
        ContentItem::Definition(d) => Canon::Definition {
            subject: d.subject.as_string().trim_end().to_string(),
            annotations: canon_annotations(&d.annotations),
            children: canon_items(d.children.iter()),
        },
        ContentItem::VerbatimBlock(v) => Canon::Verbatim {
            subject: v.subject.as_string().trim_end().to_string(),
            closing_label: v.closing_data.label.value.clone(),
            lines: v
                .children
                .iter()
                .filter_map(|c| match c {
                    ContentItem::VerbatimLine(vl) => Some(vl.content.as_string().to_string()),
                    _ => None,
                })
                .collect(),
        },
        ContentItem::VerbatimLine(vl) => Canon::Verbatim {
            subject: String::new(),
            closing_label: String::new(),
            lines: vec![vl.content.as_string().to_string()],
        },
        ContentItem::Annotation(a) => canon_annotation(a),
        ContentItem::Table(t) => {
            let mut rows: Vec<CanonRow> = Vec::new();
            for r in &t.header_rows {
                rows.push(canon_row(r));
            }
            for r in &t.body_rows {
                rows.push(canon_row(r));
            }
            let footnotes = match &t.footnotes {
                Some(list) => canon_items(list.items.iter()),
                None => Vec::new(),
            };
            Canon::Table {
                subject: t.subject.as_string().trim_end().to_string(),
                rows,
                footnotes,
                annotations: canon_annotations(&t.annotations),
            }
        }
    })
}

/// The Skeleton reducer: project a `Document` to its Faithfulness-comparable
/// Skeleton.
pub fn canon(doc: &Document) -> Canon {
    Canon::Document {
        title: doc
            .title
            .as_ref()
            .map(|t| t.content.as_string().trim_end().to_string()),
        subtitle: doc
            .title
            .as_ref()
            .and_then(|t| t.subtitle.as_ref())
            .map(|s| s.as_string().trim_end().to_string()),
        annotations: canon_annotations(&doc.annotations),
        children: canon_items(doc.root.children.iter()),
    }
}

/// Faithfulness (CONTEXT.md, the primary conversion invariant): a document read
/// by `reader` from `src`, serialized to Lex, and re-parsed is the *same*
/// document — Skeleton-equal, never byte-equal. This is the conversion sibling
/// of `format_invariants::check_semantic_preserved`:
///
/// ```text
/// canon(reader.parse(src)) == canon(lex_parse(lex_serialize(reader.parse(src))))
/// ```
///
/// Returns a diff-bearing `Err` on mismatch so callers can report it. Blank-line
/// *counts* are decoration and are ignored (they live only in `BlankLineGroup`s,
/// which `canon` drops); block *structure* and the Title are not.
pub fn check_faithful(reader: &dyn Format, src: &str) -> Result<(), String> {
    let read = reader
        .parse(src)
        .map_err(|e| format!("[{}] reader failed to parse source: {e}", reader.name()))?;

    let lex = LexFormat::default();
    let lex_text = lex
        .serialize(&read)
        .map_err(|e| format!("failed to serialize to Lex: {e}"))?;
    let reparsed = lex.parse(&lex_text).map_err(|e| {
        format!("serialized Lex did not re-parse: {e}\n--- Lex output ---\n{lex_text}")
    })?;

    let want = canon(&read);
    let got = canon(&reparsed);
    if want == got {
        Ok(())
    } else {
        Err(format!(
            "NOT FAITHFUL ({} -> Lex -> Lex)\n--- source ---\n{src}\n--- serialized Lex ---\n{lex_text}\n--- canon(read(src)) ---\n{want:#?}\n--- canon(reparse(serialize(read(src)))) ---\n{got:#?}",
            reader.name()
        ))
    }
}
