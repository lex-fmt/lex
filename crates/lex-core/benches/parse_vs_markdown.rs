// Order-of-magnitude sanity check: how does
// `lex_core::lex::parsing::parse_document` compare to
// `comrak::parse_document` (the Markdown parser lex-babel already
// depends on) when parsing equivalent-content documents?
//
// Markdown's grammar is strictly less expressive than Lex's
// (no indentation-driven sessions, no annotations, no atomized
// includes), so a Markdown parser will come out faster on principle.
// The point isn't to crown a winner — it's to learn whether Lex's
// parser sits within ~2×, ~20×, or ~200× of a battle-tested
// CommonMark/GFM parser on documents that say roughly the same thing.
//
// # Fixtures
//
// Four documents from `comms/specs/benchmark/`. Each pair spans the
// same content in both formats:
//
//   tier   fixture                  source of .md
//   ----   ------------------------ -------------------------------
//   A      010-kitchensink          hand-authored in comms/
//   A      20-ideas-naked           hand-authored in comms/
//   B      040-on-parsing           auto-converted via `lexd ... --to markdown`
//   B      080-gentle-introduction  auto-converted via `lexd ... --to markdown`
//
// Tier A is the fairest comparison (a human chose how to express the
// same ideas in each format). Tier B doubles the dataset; the
// converter is well-exercised production code, so any
// converter-introduced bias should be small.
//
// # Running
//
//     python3 crates/lex-core/benches/md_corpus/prep.py
//     cargo bench -p lex-core --bench parse_vs_markdown

use std::path::{Path, PathBuf};

use comrak::{Arena, ComrakOptions};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

// Repo-root-relative path to the prep generator. The panic in `load`
// concatenates this onto `repo_root()`, so it must reach the file from
// the repo root — not from `CARGO_MANIFEST_DIR` (which would be
// `crates/lex-core/`).
const GEN_SCRIPT: &str = "crates/lex-core/benches/md_corpus/prep.py";

struct Fixture {
    name: &'static str,
    /// Path to the `.lex` source, relative to the repo root.
    lex: &'static str,
    /// Path to the `.md` source. Either a hand-authored file under
    /// `comms/` (tier A) or an auto-converted file under
    /// `benches/md_corpus/auto/` (tier B, produced by `prep.py`).
    md: &'static str,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "010-kitchensink",
        lex: "comms/specs/benchmark/010-kitchensink.lex",
        md: "comms/specs/benchmark/010-kitchensink.md",
    },
    Fixture {
        name: "20-ideas-naked",
        lex: "comms/specs/benchmark/20-ideas-naked.lex",
        md: "comms/specs/benchmark/20-ideas-naked.md",
    },
    Fixture {
        name: "040-on-parsing",
        lex: "comms/specs/benchmark/040-on-parsing.lex",
        md: "crates/lex-core/benches/md_corpus/auto/040-on-parsing.md",
    },
    Fixture {
        name: "080-gentle-introduction",
        lex: "comms/specs/benchmark/080-gentle-introduction.lex",
        md: "crates/lex-core/benches/md_corpus/auto/080-gentle-introduction.md",
    },
];

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is .../crates/lex-core; the repo root is two
    // levels up. Canonicalise so the bench mirrors how real tools see
    // these paths.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("../..")
        .canonicalize()
        .expect("repo root must canonicalise")
}

fn load(repo: &Path, rel: &str) -> String {
    let path = repo.join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing fixture {}: {e}\n\
             For comms/specs/* fixtures, init the submodule: `git submodule update --init`\n\
             For md_corpus/auto/* fixtures, run: python3 {}/{}",
            path.display(),
            repo.display(),
            GEN_SCRIPT,
        )
    })
}

fn bench_parse(c: &mut Criterion) {
    let repo = repo_root();
    let mut group = c.benchmark_group("parse_vs_markdown");
    group.measurement_time(std::time::Duration::from_secs(10));
    group.warm_up_time(std::time::Duration::from_secs(3));

    let md_opts = ComrakOptions::default();

    for fx in FIXTURES {
        let lex_src = load(&repo, fx.lex);
        let md_src = load(&repo, fx.md);

        let lex_name = format!("{}/lex", fx.name);
        group.bench_function(&lex_name, |b| {
            b.iter(|| {
                let doc =
                    lex_core::lex::parsing::parse_document(black_box(&lex_src)).expect("lex parse");
                black_box(doc);
            });
        });

        let md_name = format!("{}/md", fx.name);
        group.bench_function(&md_name, |b| {
            b.iter(|| {
                // Fresh arena per iter mirrors what lex-core does
                // (each call allocates its own AST), so the two
                // parsers are charged for comparable allocation work.
                let arena = Arena::new();
                let doc = comrak::parse_document(&arena, black_box(&md_src), &md_opts);
                black_box(doc);
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
