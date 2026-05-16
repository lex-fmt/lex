//! Per-`DocNode` round-trip proptests for the lex-babel IR.
//!
//! These tests are the executable form of the round-trip contract
//! documented in `crates/lex-babel/src/ir/mod.rs`: for every variant of
//! [`DocNode`] and [`InlineContent`], `to_ir(from_ir(ir1)) == ir1`
//! modulo the explicitly-documented accepted losses (heading levels,
//! inline-format nesting, video/audio inline).
//!
//! Filed against #614 (umbrella #613). The Phase 3b flip in PR #621
//! made `document_annotations` symmetric but deferred the per-variant
//! proptest matrix; this module closes that gap.

use lex_babel::ir::nodes::{
    Annotation, Audio, DocNode, Document, Heading, Image, InlineContent, LabelForm, List, ListForm,
    ListItem, ListStyle, Paragraph, ReferenceType, Table, TableCell, TableCellAlignment, TableRow,
    Verbatim, Video,
};
use lex_babel::{from_ir, to_ir};
use proptest::prelude::*;

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Build a single-child IR document, run `to_ir(from_ir(ir))`, return
/// the round-tripped node so the caller can assert against it.
fn round_trip_node(node: DocNode) -> DocNode {
    let ir = Document {
        title: None,
        subtitle: None,
        children: vec![node],
        document_annotations: Vec::new(),
    };
    let ast = from_ir(&ir);
    let ir2 = to_ir(&ast);
    assert_eq!(
        ir2.children.len(),
        1,
        "round-trip should preserve child count (got {} children)",
        ir2.children.len()
    );
    ir2.children.into_iter().next().unwrap()
}

/// Flatten inline content to plain text. The IR `Bold([Italic([Text])])`
/// nesting flattens through `to_lex` to `*_text_*` — documented loss.
/// This helper lets tests compare visible text without asserting on the
/// structural shape inside formatting containers.
fn inline_text(content: &[InlineContent]) -> String {
    let mut out = String::new();
    for inline in content {
        match inline {
            InlineContent::Text(t) | InlineContent::Code(t) | InlineContent::Math(t) => {
                out.push_str(t)
            }
            InlineContent::Reference { raw, .. } => {
                out.push('[');
                out.push_str(raw);
                out.push(']');
            }
            InlineContent::Link { text, href } => {
                out.push_str(text);
                out.push(' ');
                out.push('[');
                out.push_str(href);
                out.push(']');
            }
            InlineContent::Bold(c) => {
                out.push('*');
                out.push_str(&inline_text(c));
                out.push('*');
            }
            InlineContent::Italic(c) => {
                out.push('_');
                out.push_str(&inline_text(c));
                out.push('_');
            }
            InlineContent::Image(_) => { /* skipped for plain-text comparison */ }
        }
    }
    out
}

// -----------------------------------------------------------------------------
// Inline strategies
// -----------------------------------------------------------------------------

/// Text-only inline content; the simplest case. Used wherever the
/// surrounding container's round-trip is the variable under test and
/// we don't want inline structure mixing in.
fn plain_text_inline() -> impl Strategy<Value = Vec<InlineContent>> {
    "[a-zA-Z][a-zA-Z0-9 ]{0,20}".prop_map(|s| vec![InlineContent::Text(s)])
}

// -----------------------------------------------------------------------------
// 1. Paragraph
// -----------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn paragraph_round_trips(text in "[a-zA-Z][a-zA-Z0-9 ]{0,30}") {
        let node = DocNode::Paragraph(Paragraph {
            content: vec![InlineContent::Text(text.clone())],
        });
        let back = round_trip_node(node);
        match back {
            DocNode::Paragraph(p) => {
                prop_assert_eq!(inline_text(&p.content), text);
            }
            other => prop_assert!(false, "expected Paragraph, got {:?}", other),
        }
    }
}

// -----------------------------------------------------------------------------
// 2. Heading (accepted loss: level reconstruction)
// -----------------------------------------------------------------------------
//
// `to_lex_heading` reconstructs nesting from parent context rather than
// the stored `level` field. A bare heading at IR level 1 round-trips
// through serialization losing nothing meaningful at the *content* level;
// the level itself is the documented loss. Test asserts content survival
// only.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn heading_content_round_trips(text in "[a-zA-Z][a-zA-Z0-9 ]{0,30}") {
        let node = DocNode::Heading(Heading {
            level: 1,
            content: vec![InlineContent::Text(text.clone())],
            children: Vec::new(),
        });
        let back = round_trip_node(node);
        match back {
            DocNode::Heading(h) => {
                prop_assert_eq!(inline_text(&h.content), text);
                // Note: h.level is NOT asserted equal — documented loss.
            }
            other => prop_assert!(false, "expected Heading, got {:?}", other),
        }
    }
}

// -----------------------------------------------------------------------------
// 3. List + ListItem (across ListStyle × ListForm)
// -----------------------------------------------------------------------------

fn list_item_strategy() -> impl Strategy<Value = ListItem> {
    plain_text_inline().prop_map(|content| ListItem {
        content,
        children: Vec::new(),
    })
}

prop_compose! {
    fn list_strategy(style: ListStyle, form: ListForm)
        (items in prop::collection::vec(list_item_strategy(), 1..4))
        -> List
    {
        List {
            items,
            ordered: !matches!(style, ListStyle::Bullet),
            style,
            form,
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    #[test]
    fn list_bullet_short_round_trips(list in list_strategy(ListStyle::Bullet, ListForm::Short)) {
        let expected_text: Vec<String> =
            list.items.iter().map(|i| inline_text(&i.content)).collect();
        let back = round_trip_node(DocNode::List(list));
        match back {
            DocNode::List(l) => {
                prop_assert_eq!(matches!(l.style, ListStyle::Bullet), true);
                let actual_text: Vec<String> =
                    l.items.iter().map(|i| inline_text(&i.content)).collect();
                prop_assert_eq!(actual_text, expected_text);
            }
            other => prop_assert!(false, "expected List, got {:?}", other),
        }
    }

    #[test]
    fn list_numeric_short_round_trips(list in list_strategy(ListStyle::Numeric, ListForm::Short)) {
        let expected_text: Vec<String> =
            list.items.iter().map(|i| inline_text(&i.content)).collect();
        let back = round_trip_node(DocNode::List(list));
        match back {
            DocNode::List(l) => {
                prop_assert_eq!(matches!(l.style, ListStyle::Numeric), true);
                let actual_text: Vec<String> =
                    l.items.iter().map(|i| inline_text(&i.content)).collect();
                prop_assert_eq!(actual_text, expected_text);
            }
            other => prop_assert!(false, "expected List, got {:?}", other),
        }
    }
}

// -----------------------------------------------------------------------------
// 4. Definition
// -----------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn definition_round_trips(
        term in "[A-Z][a-zA-Z0-9 ]{0,15}".prop_map(|s| s.trim_end().to_string()),
        body in "[a-zA-Z][a-zA-Z0-9 ]{0,30}",
    ) {
        let node = DocNode::Definition(lex_babel::ir::nodes::Definition {
            term: vec![InlineContent::Text(term.clone())],
            description: vec![DocNode::Paragraph(Paragraph {
                content: vec![InlineContent::Text(body.clone())],
            })],
        });
        let back = round_trip_node(node);
        match back {
            DocNode::Definition(d) => {
                prop_assert_eq!(inline_text(&d.term), term);
                prop_assert_eq!(d.description.len(), 1);
                if let DocNode::Paragraph(p) = &d.description[0] {
                    prop_assert_eq!(inline_text(&p.content), body);
                } else {
                    prop_assert!(false, "expected Paragraph in description");
                }
            }
            other => prop_assert!(false, "expected Definition, got {:?}", other),
        }
    }
}

// -----------------------------------------------------------------------------
// 5. Verbatim — covers #614 bonus: parameters survival
// -----------------------------------------------------------------------------

prop_compose! {
    fn verbatim_strategy()
        (subject in prop::option::of("[A-Z][a-zA-Z0-9 ]{0,10}"),
         lang in prop::option::of("[a-z][a-z0-9_-]{0,8}"),
         content in "[a-zA-Z][a-zA-Z0-9 \n]{0,30}".prop_map(|s| s.trim_end().to_string()),
         params in prop::collection::vec(
             ("[a-z][a-z0-9_]{0,6}", "[a-zA-Z0-9_]{1,10}"), 0..3
         ))
        -> Verbatim
    {
        // de-dup parameter keys: lex parameter syntax requires unique keys
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let parameters: Vec<(String, String)> = params
            .into_iter()
            .filter(|(k, _)| seen.insert(k.clone()))
            .collect();
        Verbatim {
            subject: subject.map(|s| s.trim_end().to_string()).filter(|s| !s.is_empty()),
            language: lang.filter(|s| !s.is_empty()),
            content,
            parameters,
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn verbatim_parameters_survive_round_trip(verb in verbatim_strategy()) {
        // Only the no-language case actually serializes through the IR
        // path uniformly; with a language the registry might attempt
        // IR-build hydration. Filter to the generic verbatim path so
        // the test isolates the new `parameters` field.
        prop_assume!(verb.language.is_none() || verb.language.as_deref() == Some(""));
        let expected_params = verb.parameters.clone();
        let expected_content = verb.content.clone();
        let back = round_trip_node(DocNode::Verbatim(verb));
        match back {
            DocNode::Verbatim(v) => {
                prop_assert_eq!(v.parameters, expected_params);
                prop_assert_eq!(v.content, expected_content);
            }
            other => prop_assert!(false, "expected Verbatim, got {:?}", other),
        }
    }
}

#[test]
fn verbatim_with_third_party_label_and_params_survives_round_trip() {
    // Targeted regression for #614 Sub C follow-up: a verbatim with a
    // closing label that isn't an `on_ir_build` canonical (so it falls
    // back to `DocNode::Verbatim` rather than hydrating) keeps its
    // `parameters` through the lex round-trip.
    let verb = Verbatim {
        subject: Some("Snippet".to_string()),
        language: Some("acme.snippet".to_string()),
        content: "x = 1\ny = 2".to_string(),
        parameters: vec![
            ("lang".to_string(), "python".to_string()),
            ("highlight".to_string(), "true".to_string()),
        ],
    };
    let back = round_trip_node(DocNode::Verbatim(verb.clone()));
    match back {
        DocNode::Verbatim(v) => {
            assert_eq!(v.subject, verb.subject);
            assert_eq!(v.language, verb.language);
            assert_eq!(v.content, verb.content);
            assert_eq!(v.parameters, verb.parameters);
        }
        other => panic!("expected Verbatim, got {other:?}"),
    }
}

// -----------------------------------------------------------------------------
// 6. Annotation — preserves LabelForm (#593)
// -----------------------------------------------------------------------------

#[test]
fn annotation_label_form_canonical_round_trips() {
    let node = DocNode::Annotation(Annotation {
        label: "lex.metadata.author".to_string(),
        parameters: Vec::new(),
        content: vec![DocNode::Paragraph(Paragraph {
            content: vec![InlineContent::Text("Alice".to_string())],
        })],
        form: LabelForm::Canonical,
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Annotation(a) => {
            assert_eq!(a.label, "lex.metadata.author");
            assert!(matches!(a.form, LabelForm::Canonical));
        }
        other => panic!("expected Annotation, got {other:?}"),
    }
}

#[test]
fn annotation_parameters_survive_round_trip() {
    let node = DocNode::Annotation(Annotation {
        label: "lex.metadata.author".to_string(),
        parameters: vec![
            ("name".to_string(), "Alice".to_string()),
            ("email".to_string(), "a@example.com".to_string()),
        ],
        content: Vec::new(),
        form: LabelForm::Canonical,
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Annotation(a) => {
            assert_eq!(a.parameters.len(), 2);
            let by_key: std::collections::HashMap<&str, &str> = a
                .parameters
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            assert_eq!(by_key.get("name"), Some(&"Alice"));
            assert_eq!(by_key.get("email"), Some(&"a@example.com"));
        }
        other => panic!("expected Annotation, got {other:?}"),
    }
}

// -----------------------------------------------------------------------------
// 7. Image / Video / Audio (block)
// -----------------------------------------------------------------------------
//
// Block-level media goes through the `lex.media.*` verbatim hydration
// path: a `DocNode::Image` exports as `:: image src=... ::` and
// re-hydrates on the way back. Properties: src + alt + title survive.

#[test]
fn image_round_trips() {
    let node = DocNode::Image(Image {
        src: "photo.jpg".to_string(),
        alt: "A photo".to_string(),
        title: Some("Vacation".to_string()),
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Image(i) => {
            assert_eq!(i.src, "photo.jpg");
            assert_eq!(i.alt, "A photo");
            assert_eq!(i.title.as_deref(), Some("Vacation"));
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn video_round_trips() {
    let node = DocNode::Video(Video {
        src: "movie.mp4".to_string(),
        title: Some("Lecture 1".to_string()),
        poster: Some("poster.jpg".to_string()),
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Video(v) => {
            assert_eq!(v.src, "movie.mp4");
            assert_eq!(v.title.as_deref(), Some("Lecture 1"));
            assert_eq!(v.poster.as_deref(), Some("poster.jpg"));
        }
        other => panic!("expected Video, got {other:?}"),
    }
}

#[test]
fn audio_round_trips() {
    let node = DocNode::Audio(Audio {
        src: "song.mp3".to_string(),
        title: Some("Intro".to_string()),
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Audio(a) => {
            assert_eq!(a.src, "song.mp3");
            assert_eq!(a.title.as_deref(), Some("Intro"));
        }
        other => panic!("expected Audio, got {other:?}"),
    }
}

// -----------------------------------------------------------------------------
// 8. Table (with header + body + caption)
// -----------------------------------------------------------------------------

#[test]
fn table_round_trips() {
    let cell = |text: &str| TableCell {
        content: vec![DocNode::Paragraph(Paragraph {
            content: vec![InlineContent::Text(text.to_string())],
        })],
        header: false,
        align: TableCellAlignment::None,
        colspan: 1,
        rowspan: 1,
    };
    let mut header_cell = cell("Name");
    header_cell.header = true;
    let mut header_cell2 = cell("Age");
    header_cell2.header = true;
    let node = DocNode::Table(Table {
        header: vec![TableRow {
            cells: vec![header_cell, header_cell2],
        }],
        rows: vec![TableRow {
            cells: vec![cell("Alice"), cell("30")],
        }],
        caption: Some(vec![InlineContent::Text("Demographics".to_string())]),
        footnotes: Vec::new(),
        fullwidth: false,
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Table(t) => {
            assert_eq!(t.header.len(), 1);
            assert_eq!(t.header[0].cells.len(), 2);
            assert_eq!(t.rows.len(), 1);
            assert_eq!(t.rows[0].cells.len(), 2);
            // Caption is a documented v1 loss — the pipe-table verbatim
            // form doesn't encode it. See module-level "Accepted losses".
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

// -----------------------------------------------------------------------------
// 9. InlineContent — covers #614 Reference promotion
// -----------------------------------------------------------------------------
//
// References gain a typed `kind` from the lex-core classifier on
// re-parse. The IR may construct a Reference with `kind: NotSure`
// (markdown / rfc-xml importers), but after a round-trip through Lex
// the classifier re-runs and the `kind` is populated.
//
// Linkable kinds (Url / File / Session) are rewritten to
// `InlineContent::Link` by `resolve_implicit_anchors` on the
// lex → IR side — documented behaviour, see the module-level "Accepted
// losses" doc on `crates/lex-babel/src/ir/mod.rs`. The tests below
// assert the post-round-trip Link shape for those kinds, and the
// post-round-trip Reference + typed kind for the non-linkable kinds
// (Citation / FootnoteNumber / AnnotationReference / ToCome / General).

#[test]
fn reference_url_resolves_to_link_after_round_trip() {
    let node = DocNode::Paragraph(Paragraph {
        content: vec![InlineContent::Reference {
            raw: "https://example.com".to_string(),
            kind: ReferenceType::NotSure,
        }],
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Paragraph(p) => match &p.content[..] {
            [InlineContent::Link { text, href }] => {
                assert_eq!(href, "https://example.com");
                assert_eq!(text, "https://example.com");
            }
            other => panic!("expected single Link, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn reference_session_resolves_to_link_after_round_trip() {
    let node = DocNode::Paragraph(Paragraph {
        content: vec![InlineContent::Reference {
            raw: "#2.1".to_string(),
            kind: ReferenceType::NotSure,
        }],
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Paragraph(p) => match &p.content[..] {
            [InlineContent::Link { text, href }] => {
                assert_eq!(href, "#2.1");
                assert_eq!(text, "#2.1");
            }
            other => panic!("expected single Link, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn reference_url_with_anchor_text_resolves_to_link() {
    // The anchor heuristic pulls the preceding word as link text. With
    // the surrounding text present we land in the "word before"
    // branch of `resolve_implicit_anchors`.
    let node = DocNode::Paragraph(Paragraph {
        content: vec![
            InlineContent::Text("visit example ".to_string()),
            InlineContent::Reference {
                raw: "https://example.com".to_string(),
                kind: ReferenceType::NotSure,
            },
        ],
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Paragraph(p) => {
            // After round-trip: [Text("visit "), Link { text: "example", href: "https://example.com" }]
            // The exact shape depends on the anchor extraction; assert
            // that a Link with the expected href exists.
            let found_link = p.content.iter().any(|i| {
                matches!(i, InlineContent::Link { href, text }
                    if href == "https://example.com" && text == "example")
            });
            assert!(found_link, "expected Link with extracted anchor, got {p:?}");
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn reference_footnote_classifies_after_round_trip() {
    let node = DocNode::Paragraph(Paragraph {
        content: vec![InlineContent::Reference {
            raw: "42".to_string(),
            kind: ReferenceType::NotSure,
        }],
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Paragraph(p) => match &p.content[..] {
            [InlineContent::Reference { raw, kind }] => {
                assert_eq!(raw, "42");
                assert!(
                    matches!(kind, ReferenceType::FootnoteNumber { number } if *number == 42),
                    "expected FootnoteNumber, got {kind:?}"
                );
            }
            other => panic!("expected single Reference, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn reference_file_resolves_to_link_after_round_trip() {
    let node = DocNode::Paragraph(Paragraph {
        content: vec![InlineContent::Reference {
            raw: "./readme.md".to_string(),
            kind: ReferenceType::NotSure,
        }],
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Paragraph(p) => match &p.content[..] {
            [InlineContent::Link { text, href }] => {
                assert_eq!(href, "./readme.md");
                assert_eq!(text, "./readme.md");
            }
            other => panic!("expected single Link, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn reference_to_come_classifies_after_round_trip() {
    let node = DocNode::Paragraph(Paragraph {
        content: vec![InlineContent::Reference {
            raw: "TK".to_string(),
            kind: ReferenceType::NotSure,
        }],
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Paragraph(p) => match &p.content[..] {
            [InlineContent::Reference { kind, .. }] => {
                assert!(
                    matches!(kind, ReferenceType::ToCome { identifier: None }),
                    "expected ToCome, got {kind:?}"
                );
            }
            other => panic!("expected single Reference, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn inline_bold_text_round_trips() {
    let node = DocNode::Paragraph(Paragraph {
        content: vec![
            InlineContent::Text("Hello ".to_string()),
            InlineContent::Bold(vec![InlineContent::Text("world".to_string())]),
        ],
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Paragraph(p) => {
            // Bold containing plain Text round-trips as a Bold node.
            // The documented loss only kicks in for Bold([Italic([...])])
            // nesting (which flattens to text).
            assert_eq!(inline_text(&p.content), "Hello *world*");
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn inline_italic_text_round_trips() {
    let node = DocNode::Paragraph(Paragraph {
        content: vec![InlineContent::Italic(vec![InlineContent::Text(
            "emphasis".to_string(),
        )])],
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Paragraph(p) => {
            assert_eq!(inline_text(&p.content), "_emphasis_");
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn inline_code_round_trips() {
    let node = DocNode::Paragraph(Paragraph {
        content: vec![
            InlineContent::Text("use ".to_string()),
            InlineContent::Code("println!".to_string()),
        ],
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Paragraph(p) => match &p.content[..] {
            [InlineContent::Text(t), InlineContent::Code(c)] => {
                assert_eq!(t, "use ");
                assert_eq!(c, "println!");
            }
            other => panic!("expected Text + Code, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn inline_math_round_trips() {
    let node = DocNode::Paragraph(Paragraph {
        content: vec![
            InlineContent::Text("Energy: ".to_string()),
            InlineContent::Math("E=mc^2".to_string()),
        ],
    });
    let back = round_trip_node(node);
    match back {
        DocNode::Paragraph(p) => match &p.content[..] {
            [InlineContent::Text(t), InlineContent::Math(m)] => {
                assert_eq!(t, "Energy: ");
                assert_eq!(m, "E=mc^2");
            }
            other => panic!("expected Text + Math, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

// -----------------------------------------------------------------------------
// 10. Document — title, subtitle, document_annotations (Phase 3b coverage)
// -----------------------------------------------------------------------------

#[test]
fn document_annotations_round_trip() {
    // The Phase 3b flip (#621) made `document_annotations` the single
    // source of truth on the lex → IR → lex path. Re-assert that with
    // the structured-Reference IR shape in place.
    let ir = Document {
        title: None,
        subtitle: None,
        children: Vec::new(),
        document_annotations: vec![Annotation {
            label: "lex.metadata.title".to_string(),
            parameters: Vec::new(),
            content: vec![DocNode::Paragraph(Paragraph {
                content: vec![InlineContent::Text("My Document".to_string())],
            })],
            form: LabelForm::Canonical,
        }],
    };
    let ast = from_ir(&ir);
    let ir2 = to_ir(&ast);
    assert_eq!(ir2.document_annotations.len(), 1);
    let ann = &ir2.document_annotations[0];
    assert_eq!(ann.label, "lex.metadata.title");
    assert_eq!(ann.content.len(), 1);
    if let DocNode::Paragraph(p) = &ann.content[0] {
        assert_eq!(inline_text(&p.content), "My Document");
    } else {
        panic!("expected Paragraph in document annotation body");
    }
}

#[test]
fn document_title_subtitle_round_trip() {
    let ir = Document {
        title: Some(vec![InlineContent::Text("Main Title".to_string())]),
        subtitle: Some(vec![InlineContent::Text("Subtitle".to_string())]),
        children: vec![DocNode::Paragraph(Paragraph {
            content: vec![InlineContent::Text("Body".to_string())],
        })],
        document_annotations: Vec::new(),
    };
    let ast = from_ir(&ir);
    let ir2 = to_ir(&ast);
    assert_eq!(
        ir2.title.as_ref().map(|t| inline_text(t)).as_deref(),
        Some("Main Title")
    );
    assert_eq!(
        ir2.subtitle.as_ref().map(|t| inline_text(t)).as_deref(),
        Some("Subtitle")
    );
}
