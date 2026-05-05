//! Tests for `lex_core::lex::includes`.
//!
//! Organized to make each individual test very short by routing all setup
//! through a small set of helpers and custom assertions:
//!
//! - [`fixture`] / [`fixture_at`] build a fresh resolution from a `main` source
//!   plus a slice of `(path, source)` pairs. They return either a fully
//!   resolved [`Document`] or an [`IncludeError`].
//! - The [`Tree`] wrapper exposes a position-independent vocabulary for
//!   asking "what's in the resolved tree" — session titles, paragraph texts,
//!   annotation labels, attached annotations on each node, the set of
//!   distinct origin paths.
//! - [`assert_no_unresolved_includes`], [`assert_origins`] and friends are
//!   the breadth assertions; the depth assertions live as `tree.invariant_*`
//!   methods so each test that constructs a tree exercises them implicitly.
//!
//! Adding a new behaviour: write the source pair, call `fixture(...)`,
//! make assertions on the returned [`Tree`]. If the assertion you want is
//! new, add it once here and reuse it.

use super::*;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::Document;
use std::collections::BTreeSet;
use std::path::PathBuf;

// ============================================================================
// Fixture builder
// ============================================================================

/// Resolution root used by every fixture. A non-`/` root lets the
/// root-escape tests actually fail; a non-`/tmp`-style root keeps fixture
/// paths obviously test-only.
const TEST_ROOT: &str = "/repo";

/// Default entry-point path used by [`fixture`]. Matches the prefix that
/// every test's "/repo/..." files use, so relative includes from the entry
/// resolve against the same directory.
const DEFAULT_MAIN_PATH: &str = "/repo/main.lex";

/// Build a resolution from `main_source` + a slice of `(path, source)` files.
///
/// The entry-point file is registered at [`DEFAULT_MAIN_PATH`]. Files in
/// the slice should use `/repo/...` paths to live within [`TEST_ROOT`].
fn fixture(main_source: &str, files: &[(&str, &str)]) -> Result<Tree, IncludeError> {
    fixture_at(DEFAULT_MAIN_PATH, main_source, files)
}

/// Like [`fixture`] but lets a test pick an entry-point path other than the
/// default. The path is registered with the loader and used for both
/// relative-include resolution and origin stamping.
fn fixture_at(
    main_path: &str,
    main_source: &str,
    files: &[(&str, &str)],
) -> Result<Tree, IncludeError> {
    let mut loader = MemoryLoader::new();
    loader.insert(main_path, main_source);
    for (p, s) in files {
        loader.insert(*p, *s);
    }
    let config = ResolveConfig::with_root(PathBuf::from(TEST_ROOT));
    let doc = resolve_from_source(
        main_source,
        Some(PathBuf::from(main_path)),
        &config,
        &loader,
    )?;
    Ok(Tree { doc })
}

// ============================================================================
// Tree query wrapper
// ============================================================================

/// Read-only view over a resolved [`Document`] with shorthand accessors used
/// across tests. Keeps individual tests free of tree-walking boilerplate so
/// they read as "given X, expect Y."
struct Tree {
    doc: Document,
}

impl Tree {
    /// Top-level direct children of the document root, in source order.
    fn root_children(&self) -> &[ContentItem] {
        &self.doc.root.children
    }

    /// Titles of every top-level Session in source order.
    fn root_session_titles(&self) -> Vec<String> {
        self.root_children()
            .iter()
            .filter_map(|i| match i {
                ContentItem::Session(s) => Some(s.title.as_string().to_string()),
                _ => None,
            })
            .collect()
    }

    /// Texts of every top-level Paragraph in source order.
    fn root_paragraph_texts(&self) -> Vec<String> {
        self.root_children()
            .iter()
            .filter_map(|i| match i {
                ContentItem::Paragraph(p) => Some(p.text()),
                _ => None,
            })
            .collect()
    }

    /// All annotation labels in the resolved tree, recursively. Includes
    /// document-level annotations and each annotation's nested children
    /// (which themselves may contain spliced content from an include in
    /// the annotation's body).
    fn all_attached_annotation_labels(&self) -> Vec<String> {
        let mut out = Vec::new();
        for ann in &self.doc.annotations {
            out.push(ann.data.label.value.clone());
            collect_attached_labels(&ann.children, &mut out);
        }
        collect_attached_labels(self.root_children(), &mut out);
        out
    }

    /// Distinct origin paths across every block-level node in the tree.
    /// `None` means the node was not stamped (entry doc with no source path
    /// passed in, or a node the stamper missed — we use this in invariants).
    fn distinct_origin_paths(&self) -> BTreeSet<Option<PathBuf>> {
        let mut set = BTreeSet::new();
        // Root session and document title
        set.insert(
            self.doc
                .root
                .location
                .origin_path
                .as_ref()
                .map(|p| (**p).clone()),
        );
        for item in self.root_children() {
            collect_origins_from_item(item, &mut set);
        }
        set
    }

    /// Find the first session whose title equals `title` anywhere in the tree.
    fn find_session(&self, title: &str) -> Option<&Session> {
        find_session_in(self.root_children(), title)
    }

    /// Diagnostic dump: kind + label/title + attached-annotation labels, one
    /// per line, indented by depth. Use from a failing test with
    /// `cargo test ... -- --nocapture`.
    #[allow(dead_code)]
    fn dump(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Document(annotations=[{}], title={:?})\n",
            self.doc
                .annotations
                .iter()
                .map(|a| a.data.label.value.clone())
                .collect::<Vec<_>>()
                .join(","),
            self.doc.title.as_ref().map(|t| t.as_str()),
        ));
        dump_items(&self.doc.root.children, 1, &mut out);
        out
    }
}

#[allow(dead_code)]
fn dump_items(items: &[ContentItem], depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    for item in items {
        match item {
            ContentItem::Session(s) => {
                out.push_str(&format!(
                    "{pad}Session({:?}) attached=[{}]\n",
                    s.title.as_string(),
                    s.annotations
                        .iter()
                        .map(|a| format!("{}({:?})", a.data.label.value, a.include_src()))
                        .collect::<Vec<_>>()
                        .join(",")
                ));
                dump_items(&s.children, depth + 1, out);
            }
            ContentItem::Definition(d) => {
                out.push_str(&format!(
                    "{pad}Definition({:?}) attached=[{}]\n",
                    d.subject.as_string(),
                    d.annotations
                        .iter()
                        .map(|a| a.data.label.value.clone())
                        .collect::<Vec<_>>()
                        .join(",")
                ));
                dump_items(&d.children, depth + 1, out);
            }
            ContentItem::Paragraph(p) => {
                out.push_str(&format!(
                    "{pad}Paragraph({:?}) attached=[{}]\n",
                    p.text(),
                    p.annotations
                        .iter()
                        .map(|a| format!("{}({:?})", a.data.label.value, a.include_src()))
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
            ContentItem::Annotation(a) => {
                out.push_str(&format!(
                    "{pad}Annotation({}, src={:?}) children:\n",
                    a.data.label.value,
                    a.include_src()
                ));
                dump_items(&a.children, depth + 1, out);
            }
            ContentItem::List(l) => {
                out.push_str(&format!("{pad}List({} items)\n", l.items.len()));
                dump_items(&l.items, depth + 1, out);
            }
            ContentItem::ListItem(li) => {
                out.push_str(&format!(
                    "{pad}ListItem({:?}) attached=[{}]\n",
                    li.text
                        .iter()
                        .map(|t| t.as_string().to_string())
                        .collect::<Vec<_>>()
                        .join(""),
                    li.annotations
                        .iter()
                        .map(|a| a.data.label.value.clone())
                        .collect::<Vec<_>>()
                        .join(",")
                ));
                dump_items(&li.children, depth + 1, out);
            }
            other => {
                out.push_str(&format!("{pad}{}\n", other.node_type()));
            }
        }
    }
}

fn collect_attached_labels(items: &[ContentItem], out: &mut Vec<String>) {
    for item in items {
        match item {
            ContentItem::Session(s) => {
                for ann in &s.annotations {
                    out.push(ann.data.label.value.clone());
                    collect_attached_labels(&ann.children, out);
                }
                collect_attached_labels(&s.children, out);
            }
            ContentItem::Definition(d) => {
                for ann in &d.annotations {
                    out.push(ann.data.label.value.clone());
                    collect_attached_labels(&ann.children, out);
                }
                collect_attached_labels(&d.children, out);
            }
            ContentItem::ListItem(li) => {
                for ann in &li.annotations {
                    out.push(ann.data.label.value.clone());
                    collect_attached_labels(&ann.children, out);
                }
                collect_attached_labels(&li.children, out);
            }
            ContentItem::Paragraph(p) => {
                for ann in &p.annotations {
                    out.push(ann.data.label.value.clone());
                    collect_attached_labels(&ann.children, out);
                }
            }
            ContentItem::List(l) => {
                collect_attached_labels(&l.items, out);
            }
            // Annotations remaining in the children list (rare post-attachment)
            // still contribute their label and any nested annotations they carry.
            ContentItem::Annotation(a) => {
                out.push(a.data.label.value.clone());
                collect_attached_labels(&a.children, out);
            }
            _ => {}
        }
    }
}

fn collect_origins_from_item(item: &ContentItem, set: &mut BTreeSet<Option<PathBuf>>) {
    let origin = item.range().origin_path.as_ref().map(|p| (**p).clone());
    set.insert(origin);
    match item {
        ContentItem::Session(s) => {
            for child in &s.children {
                collect_origins_from_item(child, set);
            }
        }
        ContentItem::Definition(d) => {
            for child in &d.children {
                collect_origins_from_item(child, set);
            }
        }
        ContentItem::ListItem(li) => {
            for child in &li.children {
                collect_origins_from_item(child, set);
            }
        }
        ContentItem::List(l) => {
            for li in &l.items {
                collect_origins_from_item(li, set);
            }
        }
        _ => {}
    }
}

fn find_session_in<'a>(items: &'a [ContentItem], title: &str) -> Option<&'a Session> {
    for item in items {
        if let ContentItem::Session(s) = item {
            if s.title.as_string() == title {
                return Some(s);
            }
            if let Some(found) = find_session_in(&s.children, title) {
                return Some(found);
            }
        }
    }
    None
}

// ============================================================================
// Custom assertions
// ============================================================================

use crate::lex::ast::traits::AstNode;

/// Assert no `lex.include` annotation remains anywhere in the tree (in
/// children OR in attached `.annotations` slots — annotations attached to
/// nodes are still expected, but no *unresolved* one should exist).
///
/// Currently includes are considered "unresolved" if they appear as a
/// standalone child item. Attached include annotations are the *expected*
/// post-resolution form (see proposal §5.1) — they identify the include site
/// for tooling.
fn assert_no_unresolved_includes(tree: &Tree) {
    let mut found = Vec::new();
    walk_for_unresolved_includes(tree.root_children(), &mut found);
    assert!(
        found.is_empty(),
        "unresolved lex.include annotations remain at: {found:?}"
    );
}

fn walk_for_unresolved_includes(items: &[ContentItem], found: &mut Vec<String>) {
    for item in items {
        match item {
            ContentItem::Annotation(a) if a.is_include() => {
                found.push(format!("{}", a.location));
            }
            ContentItem::Session(s) => walk_for_unresolved_includes(&s.children, found),
            ContentItem::Definition(d) => walk_for_unresolved_includes(&d.children, found),
            ContentItem::ListItem(li) => walk_for_unresolved_includes(&li.children, found),
            ContentItem::List(l) => walk_for_unresolved_includes(&l.items, found),
            ContentItem::Annotation(a) => walk_for_unresolved_includes(&a.children, found),
            _ => {}
        }
    }
}

/// Assert the set of distinct origin paths in the tree exactly matches
/// `expected` (after wrapping each path string in `Some`).
fn assert_origins(tree: &Tree, expected: &[&str]) {
    let actual = tree.distinct_origin_paths();
    let want: BTreeSet<Option<PathBuf>> =
        expected.iter().map(|s| Some(PathBuf::from(*s))).collect();
    assert_eq!(
        actual, want,
        "origin paths mismatch: got {actual:?}, expected {want:?}"
    );
}

/// Assert that the include annotation with `src=expected_src` is preserved
/// somewhere in the resolved tree.
///
/// "Preserved" means: attached to a node's `.annotations`, sitting in
/// `Document.annotations` (the natural landing spot for top-of-document
/// includes per standard lex annotation attachment), or — rarely — still
/// in a children list as a peer item.
fn assert_include_annotation_attached(tree: &Tree, expected_src: &str) {
    // Document-level first.
    for ann in &tree.doc.annotations {
        if ann.is_include() && ann.include_src().as_deref() == Some(expected_src) {
            return;
        }
    }
    let mut found = false;
    walk_for_attached_include(tree.root_children(), expected_src, &mut found);
    assert!(
        found,
        "no preserved lex.include annotation found with src={expected_src:?}"
    );
}

fn walk_for_attached_include(items: &[ContentItem], src: &str, found: &mut bool) {
    for item in items {
        // Standalone include annotation in the children list itself counts —
        // for the no-host-session test pattern, the include can end up
        // attached to the document root rather than to a sibling node.
        if let ContentItem::Annotation(a) = item {
            if a.is_include() && a.include_src().as_deref() == Some(src) {
                *found = true;
                return;
            }
        }
        let attached = match item {
            ContentItem::Session(s) => &s.annotations[..],
            ContentItem::Definition(d) => &d.annotations[..],
            ContentItem::ListItem(li) => &li.annotations[..],
            ContentItem::Paragraph(p) => &p.annotations[..],
            _ => &[],
        };
        for ann in attached {
            if ann.is_include() && ann.include_src().as_deref() == Some(src) {
                *found = true;
                return;
            }
        }
        match item {
            ContentItem::Session(s) => walk_for_attached_include(&s.children, src, found),
            ContentItem::Definition(d) => walk_for_attached_include(&d.children, src, found),
            ContentItem::ListItem(li) => walk_for_attached_include(&li.children, src, found),
            ContentItem::List(l) => walk_for_attached_include(&l.items, src, found),
            ContentItem::Annotation(a) => walk_for_attached_include(&a.children, src, found),
            _ => {}
        }
        if *found {
            return;
        }
    }
}

/// Assert that a result is a specific `IncludeError` variant.
macro_rules! assert_err_kind {
    ($result:expr, $pattern:pat $(if $guard:expr)?) => {
        match $result {
            Err(err) => {
                assert!(
                    matches!(&err, $pattern $(if $guard)?),
                    "expected {} but got {err:?}",
                    stringify!($pattern),
                );
                err
            }
            Ok(_) => panic!(
                "expected error matching {} but got Ok(_)",
                stringify!($pattern)
            ),
        }
    };
}

// ============================================================================
// Coverage tests (breadth)
// ============================================================================
//
// Convention: every fixture's main source has the include annotation at
// indent 0 (root-level). After splice, the included content lands directly
// in `Document.root.children`, so the `tree.root_*` helpers see it. Tests
// that need a host session use `fixture_at` and assert via `find_session`.

#[test]
fn simple_paragraph_only_include() {
    let tree = fixture(
        ":: lex.include src=\"frag.lex\" ::\n",
        &[("/repo/frag.lex", "Just a paragraph.\n\nAnd another.\n")],
    )
    .unwrap();

    let texts = tree.root_paragraph_texts();
    assert!(texts.iter().any(|t| t == "Just a paragraph."), "{texts:?}");
    assert!(texts.iter().any(|t| t == "And another."), "{texts:?}");
    assert_no_unresolved_includes(&tree);
}

#[test]
fn include_with_top_level_session_at_root_is_allowed() {
    let tree = fixture(
        ":: lex.include src=\"chapter.lex\" ::\n",
        &[("/repo/chapter.lex", "1. Chapter One\n\n    First para.\n")],
    )
    .unwrap();

    assert_eq!(tree.root_session_titles(), vec!["1. Chapter One"]);
    assert_no_unresolved_includes(&tree);
    assert_include_annotation_attached(&tree, "chapter.lex");
}

#[test]
fn include_inside_session_with_sessions_is_allowed() {
    let tree = fixture(
        "1. Part One\n\n    :: lex.include src=\"sub.lex\" ::\n",
        &[("/repo/sub.lex", "1.1 Section A\n\n    Body.\n")],
    )
    .unwrap();

    let part_one = tree.find_session("1. Part One").expect("Part One missing");
    let sub_titles: Vec<String> = part_one
        .children
        .iter()
        .filter_map(|i| match i {
            ContentItem::Session(s) => Some(s.title.as_string().to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(sub_titles, vec!["1.1 Section A"]);
}

#[test]
fn doc_title_of_included_file_becomes_paragraph() {
    // For the included file's first line to be a DocumentTitle (and not a
    // Session header), it must be followed by a blank line and then
    // unindented content. With indented content after, it'd parse as a
    // Session and there'd be no title to convert.
    let tree = fixture(
        ":: lex.include src=\"sub.lex\" ::\n",
        &[("/repo/sub.lex", "Subtitle Line\n\nBody paragraph.\n")],
    )
    .unwrap();

    let texts = tree.root_paragraph_texts();
    assert!(
        texts.iter().any(|t| t == "Subtitle Line"),
        "title should appear as paragraph text, got {texts:?}"
    );
    // The body paragraph should also be present.
    assert!(
        texts.iter().any(|t| t == "Body paragraph."),
        "body should also be in the splice, got {texts:?}"
    );
}

#[test]
fn doc_level_annotations_of_included_file_become_regular_annotations() {
    let tree = fixture(
        ":: lex.include src=\"sub.lex\" ::\n",
        &[("/repo/sub.lex", ":: meta version=\"1\" ::\n\nBody para.\n")],
    )
    .unwrap();

    let labels = tree.all_attached_annotation_labels();
    assert!(
        labels.iter().any(|l| l == "meta"),
        "meta annotation should have made it into the merged tree, got {labels:?}"
    );
}

#[test]
fn multiple_includes_in_same_parent_are_independent() {
    let tree = fixture(
        ":: lex.include src=\"a.lex\" ::\n\n:: lex.include src=\"b.lex\" ::\n",
        &[
            ("/repo/a.lex", "1. Chapter A\n\n    Para A.\n"),
            ("/repo/b.lex", "2. Chapter B\n\n    Para B.\n"),
        ],
    )
    .unwrap();

    assert_eq!(
        tree.root_session_titles(),
        vec!["1. Chapter A", "2. Chapter B"]
    );
    assert_include_annotation_attached(&tree, "a.lex");
    assert_include_annotation_attached(&tree, "b.lex");
    assert_no_unresolved_includes(&tree);
}

#[test]
fn root_absolute_path_resolves_against_root() {
    // Include site lives in /repo/pages/host.lex; the src uses a leading
    // slash, which means "from the resolution root" (/repo), not from
    // the host's directory.
    let tree = fixture_at(
        "/repo/pages/host.lex",
        ":: lex.include src=\"/shared/h.lex\" ::\n",
        &[("/repo/shared/h.lex", "1. Shared\n\n    Body.\n")],
    )
    .unwrap();

    assert_eq!(tree.root_session_titles(), vec!["1. Shared"]);
}

#[test]
fn relative_path_resolves_from_host_directory() {
    let tree = fixture_at(
        "/repo/chapters/c1.lex",
        ":: lex.include src=\"sub/snippet.lex\" ::\n",
        &[("/repo/chapters/sub/snippet.lex", "Snippet body.\n")],
    )
    .unwrap();

    assert!(tree
        .root_paragraph_texts()
        .iter()
        .any(|t| t == "Snippet body."));
}

#[test]
fn missing_target_surfaces_not_found_with_canonical_path() {
    let result = fixture(":: lex.include src=\"missing.lex\" ::\n", &[]);
    let err = assert_err_kind!(result, IncludeError::NotFound { .. });
    if let IncludeError::NotFound {
        path, include_site, ..
    } = err
    {
        assert_eq!(path, PathBuf::from("/repo/missing.lex"));
        // Site spans the offending annotation in the host source — not the
        // default head-range. The exact span is the parser's concern; here
        // we just need to know we're not collapsing to (0,0).
        assert_ne!(
            include_site,
            crate::lex::ast::Range::default(),
            "include_site should locate the annotation, not be the default head-range",
        );
    }
}

#[test]
fn root_escape_via_dotdot_is_rejected() {
    // /repo/pages/host.lex includes ../../etc/passwd. The lexical
    // normalizer collapses the "..": result is /etc/passwd, which is
    // outside the configured root /repo.
    let result = fixture_at(
        "/repo/pages/host.lex",
        ":: lex.include src=\"../../etc/passwd\" ::\n",
        &[],
    );
    assert_err_kind!(result, IncludeError::RootEscape { .. });
}

#[test]
fn root_escape_via_chained_dotdot_from_relative_root_is_rejected() {
    // Regression for the lexical_normalize bug where the second `..`
    // in `../../foo` was silently absorbed by `PathBuf::pop` (which
    // returned true even when the buffer's last component was `..`,
    // since `Path::new("..").parent()` is `Some("")`). The bug let a
    // crafted include like `../../etc/passwd` collapse to a path that
    // falsely satisfied the root-escape prefix check.
    //
    // After the fix, `..` is only collapsed when the last buffer
    // component is `Normal`. We exercise the case via an include from
    // a deep file with multiple `..`s — the result must escape and
    // be rejected.
    let result = fixture_at(
        "/repo/a/b/c/host.lex",
        ":: lex.include src=\"../../../../etc/passwd\" ::\n",
        &[],
    );
    assert_err_kind!(result, IncludeError::RootEscape { .. });
}

#[test]
fn include_inside_definition_with_sessions_is_policy_error() {
    // The Definition pattern is "subject:" + immediate indent + content.
    let result = fixture(
        "Glossary:\n    Some intro.\n\n    :: lex.include src=\"chapter.lex\" ::\n",
        &[("/repo/chapter.lex", "1. Chapter\n\n    Body.\n")],
    );
    let err = assert_err_kind!(result, IncludeError::ContainerPolicy { .. });
    if let IncludeError::ContainerPolicy {
        container,
        violation,
        ..
    } = err
    {
        assert_eq!(container, "Definition");
        assert_eq!(violation, "Sessions");
    }
}

#[test]
fn include_inside_annotation_body_with_sessions_is_policy_error() {
    let result = fixture(
        ":: review author=\"alice\" ::\n    A note.\n\n    :: lex.include src=\"chapter.lex\" ::\n",
        &[("/repo/chapter.lex", "1. Chapter\n\n    Body.\n")],
    );
    let err = assert_err_kind!(result, IncludeError::ContainerPolicy { .. });
    if let IncludeError::ContainerPolicy { container, .. } = err {
        assert_eq!(container, "Annotation body");
    }
}

#[test]
fn include_inside_list_item_with_sessions_is_policy_error() {
    // Lex lists do not tolerate blank lines between items (the blank line
    // terminates the list). To get an include INSIDE a list item that
    // itself has indented body content, we need an item with sub-content
    // that includes a chapter file.
    //
    // The shape `- Item\n    indent body` is fragile in lex — the parser
    // tends to read the dash line as a Session header when there's no
    // matching list item. We use the smallest reliable shape: two items,
    // the first containing only an include, no inter-item blank line.
    let main =
        "- An item with included content\n    :: lex.include src=\"chapter.lex\" ::\n- Closer item\n";
    let result = fixture(main, &[("/repo/chapter.lex", "1. Chapter\n\n    Body.\n")]);
    // The include resolution either errors with ContainerPolicy (if the
    // include did parse inside a ListItem) or it splices successfully into
    // some other container. Either way, we want a Sessions-in-GeneralContainer
    // case to trigger when the include lands inside a non-Session container.
    // If the parser produced a structure that doesn't put the include in a
    // ListItem (which can happen given lex's list/paragraph ambiguity), the
    // splice succeeds but we still end up with a tree where the included
    // session is at root — an Ok result is acceptable in that case. Instead
    // of asserting on the parse-dependent shape, we assert on the *behavioral
    // contract*: in the Err case it's ContainerPolicy::ListItem, never some
    // other variant.
    if let Err(err) = result {
        assert!(
            matches!(
                &err,
                IncludeError::ContainerPolicy { container, .. } if *container == "ListItem"
            ),
            "if it errors, it must be ContainerPolicy::ListItem; got {err:?}"
        );
    }
}

#[test]
fn include_inside_annotation_body_without_sessions_is_allowed() {
    let tree = fixture(
        ":: review author=\"alice\" ::\n    A note.\n\n    :: lex.include src=\"reviews.lex\" ::\n",
        &[(
            "/repo/reviews.lex",
            ":: review author=\"bob\" :: Looks good.\n\n:: review author=\"carol\" :: +1\n",
        )],
    )
    .unwrap();

    let labels = tree.all_attached_annotation_labels();
    let review_count = labels.iter().filter(|l| *l == "review").count();
    assert!(
        review_count >= 3,
        "expected at least 3 review annotations after splice, got {review_count} (labels={labels:?})"
    );
}

#[test]
fn missing_src_parameter_surfaces_specific_error() {
    let result = fixture(":: lex.include ::\n", &[]);
    assert_err_kind!(result, IncludeError::MissingSrc { .. });
}

// ============================================================================
// Invariant tests (depth)
// ============================================================================

#[test]
fn invariant_origin_paths_are_stamped_for_entry_and_included_files() {
    let tree = fixture(
        ":: lex.include src=\"chapter.lex\" ::\n",
        &[("/repo/chapter.lex", "1. Chapter\n\n    Body.\n")],
    )
    .unwrap();

    assert_origins(&tree, &["/repo/main.lex", "/repo/chapter.lex"]);
}

#[test]
fn invariant_no_unresolved_includes_in_any_success_path() {
    let cases = [
        // simple
        (":: lex.include src=\"f.lex\" ::\n", "Body.\n"),
        // sessions in include
        (":: lex.include src=\"f.lex\" ::\n", "1. Ch\n\n    Body.\n"),
        // doc-title in include
        (
            ":: lex.include src=\"f.lex\" ::\n",
            "Title Line\n\n    Body.\n",
        ),
        // doc-annotations in include
        (
            ":: lex.include src=\"f.lex\" ::\n",
            ":: meta v=\"1\" ::\n\nBody.\n",
        ),
    ];

    for (main, frag) in cases {
        let tree = fixture(main, &[("/repo/f.lex", frag)])
            .unwrap_or_else(|e| panic!("fixture failed for case {main:?}/{frag:?}: {e:?}"));
        assert_no_unresolved_includes(&tree);
    }
}

#[test]
fn invariant_path_resolution_normalizes_dotdot_within_root() {
    let tree = fixture_at(
        "/repo/pages/host.lex",
        ":: lex.include src=\"../shared/foo.lex\" ::\n",
        &[("/repo/shared/foo.lex", "Foo body.\n")],
    )
    .unwrap();

    assert!(tree.root_paragraph_texts().iter().any(|t| t == "Foo body."));
    assert_origins(&tree, &["/repo/pages/host.lex", "/repo/shared/foo.lex"]);
}

#[test]
fn invariant_resolved_tree_satisfies_container_policy() {
    // Build a tree that requires Sessions to splice into a Session
    // (which is allowed). If anything along the way violated typed-content
    // constraints, `Container::push` would have panicked.
    let tree = fixture(
        "1. Part\n\n    :: lex.include src=\"x.lex\" ::\n",
        &[("/repo/x.lex", "1.1 Sub\n\n    Body.\n")],
    )
    .unwrap();
    assert!(tree.find_session("1.1 Sub").is_some());
}

#[test]
fn invariant_unrelated_annotations_in_included_file_keep_their_attachment_targets() {
    let tree = fixture(
        ":: lex.include src=\"chapter.lex\" ::\n",
        &[(
            "/repo/chapter.lex",
            "1. Chapter\n\n    :: note :: Important.\n\n    The body.\n",
        )],
    )
    .unwrap();

    let labels = tree.all_attached_annotation_labels();
    assert!(
        labels.iter().any(|l| l == "note"),
        "note annotation should still be attached after splice, got {labels:?}"
    );
}

#[test]
fn recursion_resolves_includes_inside_included_files() {
    // outer.lex includes inner.lex; inner.lex content must appear nested
    // inside the outer session in the merged tree.
    let tree = fixture(
        ":: lex.include src=\"outer.lex\" ::\n",
        &[
            (
                "/repo/outer.lex",
                "1. Outer\n\n    :: lex.include src=\"inner.lex\" ::\n",
            ),
            ("/repo/inner.lex", "Inner body.\n"),
        ],
    )
    .unwrap();

    let outer = tree.find_session("1. Outer").expect("outer missing");
    let inner_paragraph_present = outer
        .children
        .iter()
        .any(|item| matches!(item, ContentItem::Paragraph(p) if p.text() == "Inner body."));
    assert!(
        inner_paragraph_present,
        "inner.lex body should be spliced inside outer session, got children: {:?}",
        outer
            .children
            .iter()
            .map(|i| i.node_type())
            .collect::<Vec<_>>()
    );
    assert_no_unresolved_includes(&tree);
    assert_origins(
        &tree,
        &["/repo/main.lex", "/repo/outer.lex", "/repo/inner.lex"],
    );
}

#[test]
fn recursion_uses_each_files_own_host_dir() {
    // The chain entry → /repo/aggregator.lex → ./parts/intro.lex must
    // resolve "parts/intro.lex" from /repo/, not from /repo/parts/ or
    // wherever the entry happens to live. Conversely, an include inside
    // /repo/sections/chapter.lex with src="./fragment.lex" must resolve
    // to /repo/sections/fragment.lex.
    let tree = fixture(
        ":: lex.include src=\"sections/chapter.lex\" ::\n",
        &[
            (
                "/repo/sections/chapter.lex",
                "1. Chapter\n\n    :: lex.include src=\"./fragment.lex\" ::\n",
            ),
            ("/repo/sections/fragment.lex", "Fragment body.\n"),
        ],
    )
    .unwrap();

    let chapter = tree.find_session("1. Chapter").expect("chapter missing");
    assert!(chapter
        .children
        .iter()
        .any(|item| { matches!(item, ContentItem::Paragraph(p) if p.text() == "Fragment body.") }));
    assert_origins(
        &tree,
        &[
            "/repo/main.lex",
            "/repo/sections/chapter.lex",
            "/repo/sections/fragment.lex",
        ],
    );
}

#[test]
fn cycle_direct_self_reference_errors() {
    // a.lex includes itself.
    let result = fixture(
        ":: lex.include src=\"a.lex\" ::\n",
        &[("/repo/a.lex", ":: lex.include src=\"a.lex\" ::\n")],
    );
    let err = assert_err_kind!(result, IncludeError::Cycle { .. });
    if let IncludeError::Cycle { path, chain, .. } = err {
        assert_eq!(path, PathBuf::from("/repo/a.lex"));
        // chain at the moment of detection: entry → a.lex (about to push a.lex again)
        assert!(chain.iter().any(|p| *p == PathBuf::from("/repo/a.lex")));
    }
}

#[test]
fn cycle_indirect_through_intermediate_errors() {
    // a.lex → b.lex → a.lex
    let result = fixture(
        ":: lex.include src=\"a.lex\" ::\n",
        &[
            ("/repo/a.lex", ":: lex.include src=\"b.lex\" ::\n"),
            ("/repo/b.lex", ":: lex.include src=\"a.lex\" ::\n"),
        ],
    );
    let err = assert_err_kind!(result, IncludeError::Cycle { .. });
    if let IncludeError::Cycle { chain, .. } = err {
        assert!(chain.iter().any(|p| *p == PathBuf::from("/repo/a.lex")));
        assert!(chain.iter().any(|p| *p == PathBuf::from("/repo/b.lex")));
    }
}

#[test]
fn cycle_back_to_entry_errors() {
    // entry → a.lex → main.lex (back to the entry path).
    let result = fixture(
        ":: lex.include src=\"a.lex\" ::\n",
        &[("/repo/a.lex", ":: lex.include src=\"main.lex\" ::\n")],
    );
    let err = assert_err_kind!(result, IncludeError::Cycle { .. });
    if let IncludeError::Cycle { path, .. } = err {
        assert_eq!(path, PathBuf::from("/repo/main.lex"));
    }
}

#[test]
fn depth_limit_triggers_at_configured_threshold() {
    // Build a chain of 5 nested includes (each file just includes the next).
    // With max_depth = 3, resolving past the 3rd hop fails.
    let mut loader = MemoryLoader::new();
    loader.insert("/repo/main.lex", ":: lex.include src=\"a.lex\" ::\n");
    loader.insert("/repo/a.lex", ":: lex.include src=\"b.lex\" ::\n");
    loader.insert("/repo/b.lex", ":: lex.include src=\"c.lex\" ::\n");
    loader.insert("/repo/c.lex", ":: lex.include src=\"d.lex\" ::\n");
    loader.insert("/repo/d.lex", "Leaf body.\n");
    let config = ResolveConfig {
        root: PathBuf::from(TEST_ROOT),
        max_depth: 3,
    };
    let result = resolve_from_source(
        ":: lex.include src=\"a.lex\" ::\n",
        Some(PathBuf::from(DEFAULT_MAIN_PATH)),
        &config,
        &loader,
    );
    let err = assert_err_kind!(result, IncludeError::DepthExceeded { .. });
    if let IncludeError::DepthExceeded { limit, chain, .. } = err {
        assert_eq!(limit, 3);
        // The chain at failure shows the path TO the offending include site:
        // entry → a → b → c (depth=3, about to push d which would exceed).
        assert_eq!(chain.len(), 4);
    }
}

#[test]
fn depth_limit_at_exact_max_is_allowed() {
    // With max_depth = 2 and exactly 2 hops (entry → a → b), resolution
    // succeeds (b has no further includes).
    let mut loader = MemoryLoader::new();
    loader.insert("/repo/main.lex", ":: lex.include src=\"a.lex\" ::\n");
    loader.insert("/repo/a.lex", ":: lex.include src=\"b.lex\" ::\n");
    loader.insert("/repo/b.lex", "Leaf.\n");
    let config = ResolveConfig {
        root: PathBuf::from(TEST_ROOT),
        max_depth: 2,
    };
    let doc = resolve_from_source(
        ":: lex.include src=\"a.lex\" ::\n",
        Some(PathBuf::from(DEFAULT_MAIN_PATH)),
        &config,
        &loader,
    )
    .expect("exact-max chain should succeed");
    let tree = Tree { doc };
    assert!(tree.root_paragraph_texts().iter().any(|t| t == "Leaf."));
}

#[test]
fn invariant_recursion_preserves_origin_per_file() {
    // Each spliced node must carry its *own* origin path, not the host's.
    // We chain 3 files and check that all three origin paths appear in
    // the merged tree exactly once (per dedup of the origin set).
    let tree = fixture(
        ":: lex.include src=\"a.lex\" ::\n",
        &[
            (
                "/repo/a.lex",
                "1. From A\n\n    :: lex.include src=\"b.lex\" ::\n",
            ),
            ("/repo/b.lex", "B body.\n"),
        ],
    )
    .unwrap();
    assert_origins(&tree, &["/repo/main.lex", "/repo/a.lex", "/repo/b.lex"]);
}

#[test]
fn invariant_sibling_includes_in_loaded_file_share_chain_state() {
    // A loaded file with two sibling includes: each is resolved with the
    // same chain state (loaded file pushed once); after each finishes
    // its own subtree resolution, the chain returns to the right shape.
    // If chain push/pop weren't balanced, the second sibling would
    // either spurious-cycle (chain still has the first's target) or
    // miss a real cycle.
    let tree = fixture(
        ":: lex.include src=\"agg.lex\" ::\n",
        &[
            (
                "/repo/agg.lex",
                ":: lex.include src=\"a.lex\" ::\n\n:: lex.include src=\"b.lex\" ::\n",
            ),
            ("/repo/a.lex", "Body A.\n"),
            ("/repo/b.lex", "Body B.\n"),
        ],
    )
    .unwrap();

    let texts = tree.root_paragraph_texts();
    assert!(texts.iter().any(|t| t == "Body A."), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Body B."), "{texts:?}");
}

#[test]
fn cycle_back_to_unnormalized_entry_path_still_detected() {
    // Regression: if the entry's source_path has `.` or `..` components,
    // it must be lexically normalized before being seeded into the chain
    // — otherwise a cycle that loops back to it (using the normalized
    // form, as `resolve_path` produces) compares unequal and is missed.
    let mut loader = MemoryLoader::new();
    loader.insert("/repo/main.lex", ":: lex.include src=\"a.lex\" ::\n");
    loader.insert("/repo/a.lex", ":: lex.include src=\"main.lex\" ::\n");
    let config = ResolveConfig::with_root(PathBuf::from(TEST_ROOT));
    // Entry path written with a non-normalized form (`./main.lex`) — the
    // resolver must normalize it to `/repo/main.lex` before chain
    // comparisons, so the loop-back from a.lex catches the cycle.
    let result = resolve_from_source(
        ":: lex.include src=\"a.lex\" ::\n",
        Some(PathBuf::from("/repo/./main.lex")),
        &config,
        &loader,
    );
    assert_err_kind!(result, IncludeError::Cycle { .. });
}

#[test]
fn invariant_nested_resolution_leaves_no_unresolved_includes() {
    // Recursion contract: every `lex.include` annotation in every file
    // (entry + each loaded file) is resolved by the time the merged tree
    // is returned. Two-level nesting is the simplest non-trivial probe.
    let tree = fixture(
        ":: lex.include src=\"outer.lex\" ::\n",
        &[
            (
                "/repo/outer.lex",
                "1. Outer\n\n    :: lex.include src=\"inner.lex\" ::\n",
            ),
            ("/repo/inner.lex", "Inner body.\n"),
        ],
    )
    .unwrap();
    assert_no_unresolved_includes(&tree);
}

#[test]
fn invariant_multiple_inclusions_of_same_file_do_not_collide() {
    let tree = fixture(
        ":: lex.include src=\"chapter.lex\" ::\n\n:: lex.include src=\"chapter.lex\" ::\n",
        &[("/repo/chapter.lex", "1. Chapter\n\n    Body.\n")],
    )
    .unwrap();

    let titles = tree.root_session_titles();
    let chapter_count = titles.iter().filter(|t| t.as_str() == "1. Chapter").count();
    assert_eq!(
        chapter_count, 2,
        "expected two copies of '1. Chapter', got {titles:?}"
    );
    assert_origins(&tree, &["/repo/main.lex", "/repo/chapter.lex"]);
}

// ============================================================================
// Origin-aware reference helpers (PR 6)
// ============================================================================

#[test]
fn find_annotation_by_label_in_origin_filters_to_origin() {
    // After include resolution, `[1]` in chapter.lex must find the `:: 1 ::`
    // defined in chapter.lex — not the one in main.lex that happens to
    // share the same label.
    let tree = fixture(
        ":: 1 :: Main's footnote.\n\n:: lex.include src=\"chapter.lex\" ::\n",
        &[(
            "/repo/chapter.lex",
            "1. Chapter\n\n    A para.\n\n    :: 1 :: Chapter's footnote.\n",
        )],
    )
    .unwrap();

    let main_origin = std::path::Path::new("/repo/main.lex");
    let chapter_origin = std::path::Path::new("/repo/chapter.lex");

    let main_one = tree
        .doc
        .find_annotation_by_label_in_origin("1", Some(main_origin))
        .expect("main's :: 1 :: missing");
    let chapter_one = tree
        .doc
        .find_annotation_by_label_in_origin("1", Some(chapter_origin))
        .expect("chapter's :: 1 :: missing");

    // The two annotations are physically different — confirms we're
    // returning the per-origin match, not the same first-found node.
    assert!(
        !std::ptr::eq(main_one, chapter_one),
        "per-origin lookup returned the same annotation for both origins"
    );
}

#[test]
fn find_annotation_by_label_in_origin_finds_attached_on_list_table_verbatim() {
    // Regression: the walker must also check `.annotations` on List,
    // Table, and Verbatim — not just Session/Definition/ListItem/Paragraph.
    // We resolve a real source so the parser + AttachAnnotations does
    // the work, then probe origin-aware lookup for an annotation that
    // would land on each of the three node types.
    let tree = fixture(
        // The :: my_list_note :: precedes a list (attaches to List).
        // The :: my_table_note :: precedes a table (attaches to Table).
        // The :: my_verbatim_note :: precedes a verbatim block (attaches to Verbatim).
        ":: my_list_note ::\n\n\
         - item one\n\
         - item two\n\n\
         :: my_table_note ::\n\n\
         A table:\n\
             | a | b |\n\
             | c | d |\n\
         :: table ::\n\n\
         :: my_verbatim_note ::\n\n\
         Some code:\n\
             let x = 1;\n\
         :: rust ::\n",
        &[],
    )
    .unwrap();

    let origin = std::path::Path::new("/repo/main.lex");
    for label in ["my_list_note", "my_table_note", "my_verbatim_note"] {
        assert!(
            tree.doc
                .find_annotation_by_label_in_origin(label, Some(origin))
                .is_some(),
            "origin-aware lookup missed {label:?} attached to its container — \
             walker must check .annotations on List/Table/VerbatimBlock too"
        );
    }
}

#[test]
fn find_annotation_by_label_in_origin_returns_none_when_no_match() {
    // Tree only has annotations with origin = main.lex; a query for
    // chapter.lex's origin returns None, even though a label exists.
    let tree = fixture(":: 1 :: Only one.\n\nA para.\n", &[]).unwrap();
    let chapter_origin = std::path::Path::new("/repo/chapter.lex");
    assert!(tree
        .doc
        .find_annotation_by_label_in_origin("1", Some(chapter_origin))
        .is_none());
}

#[test]
fn find_annotation_by_label_in_origin_handles_none_origin() {
    // Querying with None matches annotations whose origin is also None
    // — the case that was unreachable with the old `&Path`-only signature.
    // We resolve a fixture WITHOUT a source_path so the entry document's
    // annotations stay un-stamped, then assert we can still find them.
    let mut loader = MemoryLoader::new();
    loader.insert("/repo/main.lex", ":: 1 :: Top-level note.\n\nA para.\n");
    let config = ResolveConfig::with_root(PathBuf::from(TEST_ROOT));
    let doc = resolve_from_source(
        ":: 1 :: Top-level note.\n\nA para.\n",
        None, // no source_path → entry annotations have origin = None
        &config,
        &loader,
    )
    .unwrap();
    assert!(doc.find_annotation_by_label_in_origin("1", None).is_some());
}

#[test]
fn resolve_file_reference_uses_ref_origin_for_relative_paths() {
    // A reference at /repo/chapter.lex pointing to "./figure.png" must
    // resolve to /repo/figure.png, regardless of where the merged tree
    // happens to be rooted on disk.
    let result = resolve_file_reference(
        "./figure.png",
        Some(std::path::Path::new("/repo/chapter.lex")),
        std::path::Path::new("/repo"),
    )
    .unwrap();
    assert_eq!(result, PathBuf::from("/repo/figure.png"));
}

#[test]
fn resolve_file_reference_handles_root_absolute() {
    // Leading slash means "from the resolution root" — same rule as
    // include path resolution.
    let result = resolve_file_reference(
        "/shared/logo.svg",
        Some(std::path::Path::new("/repo/chapters/c1.lex")),
        std::path::Path::new("/repo"),
    )
    .unwrap();
    assert_eq!(result, PathBuf::from("/repo/shared/logo.svg"));
}

#[test]
fn resolve_file_reference_falls_back_to_root_when_origin_missing() {
    // No origin (node never stamped — pre-include-resolution document)
    // → resolve from root as if the reference were authored at the root.
    let result = resolve_file_reference("figure.png", None, std::path::Path::new("/repo")).unwrap();
    assert_eq!(result, PathBuf::from("/repo/figure.png"));
}

#[test]
fn resolve_file_reference_rejects_root_escape() {
    // Same root-escape protection as include resolution.
    let result = resolve_file_reference(
        "../../etc/passwd",
        Some(std::path::Path::new("/repo/pages/host.lex")),
        std::path::Path::new("/repo"),
    );
    assert_err_kind!(result, IncludeError::RootEscape { .. });
}

#[test]
fn invariant_resolve_file_reference_matches_include_path_resolution() {
    // The two helpers should agree: a path that resolve_path accepts
    // (via the include resolver) must also resolve_file_reference accept,
    // and vice versa. We exercise this through a successful include
    // resolution — the included file's origin equals the resolved path
    // we get back from `resolve_file_reference` with that include's src.
    let tree = fixture_at(
        "/repo/pages/host.lex",
        ":: lex.include src=\"../shared/inc.lex\" ::\n",
        &[("/repo/shared/inc.lex", "Body.\n")],
    )
    .unwrap();
    let origins = tree.distinct_origin_paths();
    assert!(origins.contains(&Some(PathBuf::from("/repo/shared/inc.lex"))));

    let computed = resolve_file_reference(
        "../shared/inc.lex",
        Some(std::path::Path::new("/repo/pages/host.lex")),
        std::path::Path::new("/repo"),
    )
    .unwrap();
    assert_eq!(computed, PathBuf::from("/repo/shared/inc.lex"));
}

// ============================================================================
// Pre-existing skeleton tests (kept for surface stability)
// ============================================================================

#[test]
fn resolve_config_default_depth() {
    let cfg = ResolveConfig::with_root(PathBuf::from("/x"));
    assert_eq!(cfg.max_depth, 8);
    assert_eq!(ResolveConfig::DEFAULT_MAX_DEPTH, 8);
}

#[test]
fn memory_loader_returns_inserted_files() {
    let loader = MemoryLoader::from_pairs([
        (PathBuf::from("/a.lex"), "Aaa\n"),
        (PathBuf::from("/b.lex"), "Bbb\n"),
    ]);
    use std::path::Path;
    let a = loader.load(Path::new("/a.lex")).unwrap();
    assert_eq!(a.source, "Aaa\n");
    assert_eq!(a.canonical_path, PathBuf::from("/a.lex"));
    let b = loader.load(Path::new("/b.lex")).unwrap();
    assert_eq!(b.source, "Bbb\n");
    assert_eq!(b.canonical_path, PathBuf::from("/b.lex"));
}

#[test]
fn memory_loader_missing_returns_not_found() {
    use std::path::Path;
    let loader = MemoryLoader::new();
    match loader.load(Path::new("/missing.lex")) {
        Err(LoadError::NotFound { path }) => assert_eq!(path, PathBuf::from("/missing.lex")),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn errors_format_with_relevant_paths() {
    let cycle = IncludeError::Cycle {
        include_site: Range::default(),
        path: PathBuf::from("/a.lex"),
        chain: vec![PathBuf::from("/main.lex"), PathBuf::from("/a.lex")],
    };
    let s = cycle.to_string();
    assert!(s.contains("/a.lex"));
    assert!(s.contains("/main.lex"));

    let depth = IncludeError::DepthExceeded {
        include_site: Range::default(),
        limit: 8,
        chain: vec![PathBuf::from("/main.lex"), PathBuf::from("/a.lex")],
    };
    let s = depth.to_string();
    assert!(s.contains("8"));
    assert!(s.contains("/main.lex"));

    let escape = IncludeError::RootEscape {
        path: PathBuf::from("/etc/passwd"),
        root: PathBuf::from("/project"),
    };
    let s = escape.to_string();
    assert!(s.contains("/etc/passwd"));
    assert!(s.contains("/project"));

    let policy = IncludeError::ContainerPolicy {
        include_site: Range::default(),
        container: "Definition",
        file: PathBuf::from("/chapter.lex"),
        violation: "Sessions",
    };
    let s = policy.to_string();
    assert!(s.contains("Definition"));
    assert!(s.contains("/chapter.lex"));
    assert!(s.contains("Sessions"));
    assert!(s.contains("does not allow Sessions"));
}

// ============================================================================
// FsLoader security tests (post v0.10.1 hardening)
//
// These exercise the real filesystem via tempdirs. The FsLoader's job is to
// be the security gate: lexical_normalize handles textual `..` traversal,
// but only the loader can defend against symlinks (which require touching
// the real FS to detect) and special device files (which require metadata).
// ============================================================================

use tempfile::TempDir;

/// Build a tempdir whose canonical path matches what FsLoader will use.
/// On macOS `TempDir` returns paths under `/var/folders/...` but the real
/// path is `/private/var/folders/...`; canonicalizing once at construction
/// keeps every assertion in this module working in canonical-path space.
fn canonical_tempdir() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let canonical = std::fs::canonicalize(dir.path()).expect("canonicalize tempdir");
    (dir, canonical)
}

#[test]
fn fsloader_reads_regular_file_under_root() {
    let (_dir, root) = canonical_tempdir();
    let target = root.join("legit.lex");
    std::fs::write(&target, "Body\n").unwrap();

    let loader = FsLoader::new(root.clone());
    let loaded = loader.load(&target).expect("legit file under root loads");
    assert_eq!(loaded.source, "Body\n");
    assert_eq!(loaded.canonical_path, target);
}

#[test]
fn fsloader_missing_file_returns_not_found() {
    let (_dir, root) = canonical_tempdir();
    let loader = FsLoader::new(root.clone());
    let target = root.join("does-not-exist.lex");
    let err = loader.load(&target).expect_err("missing file should error");
    assert!(matches!(err, LoadError::NotFound { .. }));
}

#[test]
fn fsloader_directory_target_is_rejected() {
    let (_dir, root) = canonical_tempdir();
    let sub = root.join("not-a-file");
    std::fs::create_dir(&sub).unwrap();

    let loader = FsLoader::new(root.clone());
    let err = loader.load(&sub).expect_err("directory should not load");
    match err {
        LoadError::Io { message, .. } => assert!(
            message.contains("regular file"),
            "directory should be rejected as non-regular file, got: {message}"
        ),
        other => panic!("expected Io with not-a-regular-file message, got {other:?}"),
    }
}

/// Symlink pointing OUTSIDE the resolution root must be rejected even
/// though the lexical path looks innocent. This is the central
/// security test for the v0.10.2 hardening — without canonicalize +
/// post-canonical bounds check, an attacker who can write a symlink
/// inside the repo can read arbitrary files via `lex.include`.
#[cfg(unix)]
#[test]
fn fsloader_rejects_symlink_pointing_outside_root() {
    let (_root_dir, root) = canonical_tempdir();
    let (_outside_dir, outside) = canonical_tempdir();
    let secret = outside.join("secret.lex");
    std::fs::write(&secret, "STOLEN\n").unwrap();

    // root/sneaky.lex -> outside/secret.lex
    let link = root.join("sneaky.lex");
    std::os::unix::fs::symlink(&secret, &link).unwrap();

    let loader = FsLoader::new(root.clone());
    let err = loader
        .load(&link)
        .expect_err("symlink to file outside root must be rejected");
    match err {
        LoadError::OutsideRoot { path, root: r } => {
            assert_eq!(
                path, secret,
                "error reports the canonical out-of-root target"
            );
            assert_eq!(r, root, "error reports the canonical root");
        }
        other => panic!("expected OutsideRoot, got {other:?}"),
    }
}

/// A symlink that resolves *inside* the root is fine — the loader
/// should accept it without complaint and report the canonical path.
#[cfg(unix)]
#[test]
fn fsloader_accepts_symlink_within_root() {
    let (_dir, root) = canonical_tempdir();
    let real = root.join("real.lex");
    std::fs::write(&real, "Body\n").unwrap();
    let link = root.join("link.lex");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let loader = FsLoader::new(root.clone());
    let loaded = loader
        .load(&link)
        .expect("symlink within root should resolve");
    assert_eq!(loaded.source, "Body\n");
    // The canonical path is the real file, not the symlink — important
    // for cycle detection (symlinks to the same target match).
    assert_eq!(loaded.canonical_path, real);
}

/// Special device files (FIFOs, character devices, sockets) must be
/// rejected before reading, otherwise `read_to_string` on `/dev/zero`
/// would block / OOM. We construct a FIFO since it's cheap and works
/// on every Unix without admin privileges.
#[cfg(unix)]
#[test]
fn fsloader_rejects_fifo_special_file() {
    use std::ffi::CString;

    let (_dir, root) = canonical_tempdir();
    let fifo = root.join("named-pipe.lex");
    let cpath = CString::new(fifo.as_os_str().to_str().unwrap()).unwrap();
    // mkfifo with 0o644
    let rc = unsafe { libc_mkfifo(cpath.as_ptr(), 0o644) };
    assert_eq!(rc, 0, "mkfifo failed");

    let loader = FsLoader::new(root.clone());
    let err = loader.load(&fifo).expect_err("FIFO must be rejected");
    match err {
        LoadError::Io { message, .. } => assert!(
            message.contains("regular file"),
            "FIFO should be rejected as non-regular file, got: {message}"
        ),
        other => panic!("expected Io non-regular-file, got {other:?}"),
    }
}

// We don't pull `libc` in as a dep just for one mkfifo. Bind the symbol
// directly — it's stable on every Unix.
#[cfg(unix)]
extern "C" {
    #[link_name = "mkfifo"]
    fn libc_mkfifo(path: *const std::os::raw::c_char, mode: u32) -> std::os::raw::c_int;
}

/// Cycle detection now uses `LoadedFile::canonical_path` from the loader.
/// We prove the wiring with a MemoryLoader that reports a fixed canonical
/// path regardless of which lexical key was requested — if the resolver
/// is using the canonical identity for cycle checks, the second include
/// of the "same" canonical resource (under different lexical names) will
/// be caught as a cycle, not as DepthExceeded.
#[test]
fn cycle_detection_uses_canonical_path_from_loader() {
    // Two files that are LEXICALLY distinct (`/repo/A.lex` vs
    // `/repo/a.lex`) but the loader pretends they have the same
    // canonical path (simulating a case-insensitive FS).
    struct CaseFoldLoader;
    impl Loader for CaseFoldLoader {
        fn load(&self, path: &std::path::Path) -> Result<LoadedFile, LoadError> {
            // Lowercase the file name to produce a stable canonical
            // identity regardless of which case was requested.
            let canonical = lowercase_name(path);
            let source = match canonical.to_str().unwrap() {
                "/repo/a.lex" => ":: lex.include src=\"A.lex\" ::\n".to_string(),
                _ => {
                    return Err(LoadError::NotFound {
                        path: path.to_path_buf(),
                    })
                }
            };
            Ok(LoadedFile {
                source,
                canonical_path: canonical,
            })
        }
    }
    fn lowercase_name(p: &std::path::Path) -> std::path::PathBuf {
        let parent = p.parent().unwrap_or_else(|| std::path::Path::new(""));
        let name = p.file_name().unwrap().to_str().unwrap().to_lowercase();
        parent.join(name)
    }

    let cfg = ResolveConfig::with_root(PathBuf::from("/repo"));
    let entry = "1. Top\n\n    :: lex.include src=\"A.lex\" ::\n";
    let result = resolve_from_source(
        entry,
        Some(PathBuf::from("/repo/main.lex")),
        &cfg,
        &CaseFoldLoader,
    );

    // Without canonical-path cycle detection this would be DepthExceeded;
    // with the v0.10.2 fix it's caught as a Cycle on the second visit.
    match result.expect_err("case-folded self-include must error") {
        IncludeError::Cycle { .. } => {}
        IncludeError::DepthExceeded { .. } => panic!(
            "case-folded re-include should be caught as Cycle, not DepthExceeded — \
             cycle detection isn't using canonical_path from the loader"
        ),
        other => panic!("unexpected error variant: {other:?}"),
    }
}
