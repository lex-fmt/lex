//! Integration tests for the `lex.include` resolution feature.
//!
//! These exercise the wiring of `lex_core::lex::includes::resolve_from_source`
//! into the CLI: include resolution must work for `convert` and `inspect`
//! by default, must not run for `format` (per spec §11.4), and must be
//! disablable via `--no-includes`. Tests use `tempfile::TempDir` to set up
//! real on-disk fixtures so `FsLoader` is exercised end-to-end (no
//! MemoryLoader bypass).

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ============================================================================
// Test scaffolding
// ============================================================================

/// Build a temp directory containing the given `(relpath, contents)` files.
/// Creates parent directories as needed. Returns the TempDir (drop = cleanup).
fn fixture_dir(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    for (rel, contents) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir -p");
        }
        fs::write(&path, contents).expect("write fixture file");
    }
    dir
}

/// Path-from-temp helper.
fn path_in(dir: &TempDir, rel: &str) -> std::path::PathBuf {
    dir.path().join(rel)
}

fn lexd() -> assert_cmd::Command {
    cargo_bin_cmd!("lexd")
}

// ============================================================================
// convert: include expansion happens by default for lex input
// ============================================================================

#[test]
fn convert_lex_to_lex_expands_includes_by_default() {
    let dir = fixture_dir(&[
        ("main.lex", ":: lex.include src=\"chapter.lex\" ::\n"),
        ("chapter.lex", "1. Chapter\n\n    Body of the chapter.\n"),
    ]);
    let main = path_in(&dir, "main.lex");

    lexd()
        .arg(&main)
        .arg("--to")
        .arg("lex")
        .assert()
        .success()
        .stdout(predicate::str::contains("1. Chapter"))
        .stdout(predicate::str::contains("Body of the chapter."))
        // The literal include directive is gone — content was spliced in.
        .stdout(predicate::str::contains("lex.include").not());
}

#[test]
fn convert_lex_to_html_expands_includes() {
    let dir = fixture_dir(&[
        ("main.lex", ":: lex.include src=\"frag.lex\" ::\n"),
        ("frag.lex", "Just a paragraph.\n"),
    ]);
    let main = path_in(&dir, "main.lex");

    lexd()
        .arg(&main)
        .arg("--to")
        .arg("html")
        .assert()
        .success()
        .stdout(predicate::str::contains("Just a paragraph."));
}

#[test]
fn convert_lex_relative_include_resolves_from_entry_directory() {
    let dir = fixture_dir(&[
        (
            "docs/main.lex",
            ":: lex.include src=\"chapters/c1.lex\" ::\n",
        ),
        ("docs/chapters/c1.lex", "Chapter content.\n"),
    ]);
    let main = path_in(&dir, "docs/main.lex");

    lexd()
        .arg(&main)
        .arg("--to")
        .arg("lex")
        .assert()
        .success()
        .stdout(predicate::str::contains("Chapter content."));
}

// ============================================================================
// --no-includes: opts out
// ============================================================================

#[test]
fn convert_with_no_includes_flag_skips_resolution() {
    // The behavioural guarantee of `--no-includes` is that the chapter
    // content is *not* spliced — the resolver doesn't run. We assert on
    // that negative directly. (Asserting on the literal `lex.include`
    // line surviving the round trip is a separate concern: the lex
    // serializer's visitor does not currently emit attached
    // session annotations, so the directive may or may not appear in
    // the output depending on where attachment landed it. That's a
    // pre-existing serializer limitation, not include-feature behaviour.)
    let dir = fixture_dir(&[
        (
            "main.lex",
            "1. Host\n\n    Some intro paragraph.\n\n    :: lex.include src=\"chapter.lex\" ::\n",
        ),
        ("chapter.lex", "Should not appear.\n"),
    ]);
    let main = path_in(&dir, "main.lex");

    lexd()
        .arg("--no-includes")
        .arg(&main)
        .arg("--to")
        .arg("lex")
        .assert()
        .success()
        .stdout(predicate::str::contains("1. Host"))
        .stdout(predicate::str::contains("Some intro paragraph."))
        // The chapter content is NOT spliced — this is the
        // behavioural contract of --no-includes.
        .stdout(predicate::str::contains("Should not appear.").not());
}

// ============================================================================
// format: never expands (proposal §11.4)
// ============================================================================

#[test]
fn format_never_expands_includes() {
    // Spec §11.4: `lex format` never expands includes. As above, we
    // assert on the negative (chapter not spliced) without depending
    // on directive serialization.
    let dir = fixture_dir(&[
        (
            "main.lex",
            "1. Host\n\n    Intro line.\n\n    :: lex.include src=\"chapter.lex\" ::\n",
        ),
        ("chapter.lex", "Chapter would-be body.\n"),
    ]);
    let main = path_in(&dir, "main.lex");

    lexd()
        .arg("format")
        .arg(&main)
        .assert()
        .success()
        .stdout(predicate::str::contains("1. Host"))
        .stdout(predicate::str::contains("Chapter would-be body.").not());
}

// ============================================================================
// Error surfaces
// ============================================================================

#[test]
fn missing_include_target_surfaces_error_with_path() {
    let dir = fixture_dir(&[("main.lex", ":: lex.include src=\"missing.lex\" ::\n")]);
    let main = path_in(&dir, "main.lex");

    lexd()
        .arg(&main)
        .arg("--to")
        .arg("lex")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Include resolution error"))
        .stderr(predicate::str::contains("missing.lex"));
}

#[test]
fn cycle_in_includes_is_reported() {
    let dir = fixture_dir(&[
        ("main.lex", ":: lex.include src=\"a.lex\" ::\n"),
        ("a.lex", ":: lex.include src=\"b.lex\" ::\n"),
        ("b.lex", ":: lex.include src=\"a.lex\" ::\n"),
    ]);
    let main = path_in(&dir, "main.lex");

    lexd()
        .arg(&main)
        .arg("--to")
        .arg("lex")
        .assert()
        .failure()
        .stderr(predicate::str::contains("cycle"));
}

// ============================================================================
// --includes-root: explicit override
// ============================================================================

#[test]
fn includes_root_flag_constrains_resolution() {
    // Without --includes-root, the resolver walks up to find a lex.toml or
    // falls back to the entry's directory. With --includes-root pointing
    // at a sibling subtree, an include outside that root fails RootEscape.
    let dir = fixture_dir(&[
        // Entry lives in /docs but tries to include from /sibling
        (
            "docs/main.lex",
            ":: lex.include src=\"../sibling/x.lex\" ::\n",
        ),
        ("sibling/x.lex", "Should not be reachable.\n"),
    ]);
    let main = path_in(&dir, "docs/main.lex");
    let docs_root = dir.path().join("docs");

    lexd()
        .arg("--includes-root")
        .arg(&docs_root)
        .arg(&main)
        .arg("--to")
        .arg("lex")
        .assert()
        .failure()
        .stderr(predicate::str::contains("escapes resolution root"));
}

// ============================================================================
// Documents that don't use includes are unaffected
// ============================================================================

#[test]
fn convert_lex_without_includes_works_unchanged() {
    let dir = fixture_dir(&[("main.lex", "1. Chapter\n\n    Body.\n")]);
    let main = path_in(&dir, "main.lex");

    lexd()
        .arg(&main)
        .arg("--to")
        .arg("lex")
        .assert()
        .success()
        .stdout(predicate::str::contains("1. Chapter"))
        .stdout(predicate::str::contains("Body."));
}

#[test]
fn nearest_lex_toml_walks_upward_to_find_root() {
    // /project/lex.toml exists; /project/docs/sub/main.lex includes a file
    // /project/shared/foo.lex via root-absolute path. With a lex.toml at
    // /project, root-absolute resolves under /project — so /shared/foo.lex
    // hits /project/shared/foo.lex.
    let dir = fixture_dir(&[
        ("project/.lex.toml", "# minimal\n"),
        (
            "project/docs/sub/main.lex",
            ":: lex.include src=\"/shared/foo.lex\" ::\n",
        ),
        ("project/shared/foo.lex", "Foo body content.\n"),
    ]);
    let main = path_in(&dir, "project/docs/sub/main.lex");

    lexd()
        .arg(&main)
        .arg("--to")
        .arg("lex")
        .assert()
        .success()
        .stdout(predicate::str::contains("Foo body content."));
}

// ============================================================================
// Regression: prose mentions of "lex.include" + verbatim blocks
// ============================================================================

/// `lexd inspect` resolves includes by default, which (before the fix in
/// lex#505) sent the source through a parse → serialize → re-parse round
/// trip whenever the literal string `lex.include` appeared anywhere — even
/// in prose. The serializer dropped the blank line that separates a
/// paragraph from a verbatim block's subject, and the re-parser then
/// merged the subject into the paragraph and lost the verbatim.
///
/// Asserts that `inspect` (default, includes enabled) produces the same
/// AST tree as `inspect --no-includes` for a document with `lex.include`
/// in prose and a verbatim block.
#[test]
fn inspect_preserves_verbatim_when_lex_include_is_only_in_prose() {
    // Bare `lex.include` mention in prose, no actual annotation, followed
    // by a verbatim block (subject line + indented body + closing marker)
    // which must survive the round trip.
    let fixture = concat!(
        "Title\n",
        "=====\n",
        "\n",
        "Some text mentioning `lex.include` in prose.\n",
        "\n",
        "Code Example:\n",
        "\n",
        "    fn main() {}\n",
        "\n",
        ":: rust ::\n",
        "\n",
        "End.\n",
    );
    let dir = fixture_dir(&[("doc.lex", fixture)]);
    let doc = path_in(&dir, "doc.lex");

    let default_out = lexd()
        .arg("inspect")
        .arg(&doc)
        .output()
        .expect("run lexd inspect");
    assert!(default_out.status.success());
    let default_stdout = String::from_utf8(default_out.stdout).unwrap();

    let noinc_out = lexd()
        .arg("inspect")
        .arg("--no-includes")
        .arg(&doc)
        .output()
        .expect("run lexd inspect --no-includes");
    assert!(noinc_out.status.success());
    let noinc_stdout = String::from_utf8(noinc_out.stdout).unwrap();

    assert_eq!(
        default_stdout, noinc_stdout,
        "inspect with includes resolved must equal --no-includes when no actual lex.include exists.\n--- default ---\n{default_stdout}\n--- --no-includes ---\n{noinc_stdout}"
    );
    assert!(
        default_stdout.contains("𝒱"),
        "expected verbatim block in inspect output, got:\n{default_stdout}"
    );
}

// Ensure the fixture helper doesn't get pruned by dead-code lint when
// tests change shape during development.
#[test]
fn _fixture_helper_is_used() {
    let _dir = fixture_dir(&[("a.lex", "x\n")]);
    let _ = Path::new("placeholder");
}
