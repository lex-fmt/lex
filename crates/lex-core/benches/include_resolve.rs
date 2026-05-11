// Criterion bench for `resolve_from_source` — the hot path the
// extension-system PRs reshaped (PR 3d, `3a46fed3`). Routes through
// `Registry::dispatch_resolve_raw` → `LexIncludeHandler` → wire codec
// → splice. Initial run of this rig (vs `f0c7f1f0`, the last commit
// before PR 1) showed the wire-codec tax sits below the measurement
// floor across the seven scenarios below.
//
// # Running
//
//     python3 crates/lex-core/benches/corpus/gen.py
//     cargo bench -p lex-core --bench include_resolve
//
// The generator writes deterministic fixtures under
// `crates/lex-core/benches/corpus/s*/` (gitignored). Override the
// corpus location via the `BENCH_CORPUS` env var when comparing
// alternative inputs.
//
// # Cross-revision comparisons
//
// Use `git worktree add` to materialise the revisions you want to
// compare in sibling directories, copy this bench file into each (the
// pre-flip baseline takes `&dyn Loader` instead of `&Registry`), point
// `BENCH_CORPUS` at a shared corpus, and `cargo bench` in each
// worktree. CI does not run this bench — runner noise dwarfs the
// deltas it would need to detect.

use std::path::PathBuf;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lex_core::lex::builtins;
use lex_core::lex::includes::{resolve_from_source, FsLoader, ResolveConfig};
use lex_extension_host::registry::Registry;

/// Path to the in-repo corpus generator. Always reported in error
/// hints — even when `BENCH_CORPUS` points at an external directory,
/// the generator that built that directory's fixtures (or the one the
/// user should run if they meant to use the default) lives here.
const GEN_PY_REL: &str = "benches/corpus/gen.py";

fn corpus_root() -> PathBuf {
    let raw = if let Ok(p) = std::env::var("BENCH_CORPUS") {
        PathBuf::from(p)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/corpus")
    };
    // `ResolveConfig::root` is documented as needing to be absolute and
    // lexically normalised; an unnormalised root weakens the resolver's
    // root-escape prefix check (see `lex/includes.rs`). Canonicalise
    // here so the bench mirrors what the CLI/LSP do in production, and
    // so a fat-fingered `BENCH_CORPUS=./corpus` fails fast instead of
    // silently changing resolution semantics.
    raw.canonicalize().unwrap_or_else(|e| {
        panic!(
            "could not canonicalise corpus root {}: {e}\nRun: python3 {}/{}",
            raw.display(),
            env!("CARGO_MANIFEST_DIR"),
            GEN_PY_REL,
        )
    })
}

struct Scenario {
    name: &'static str,
    dir: &'static str,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "s1_no_includes",
        dir: "s1_no_includes",
    },
    Scenario {
        name: "s2_one_small",
        dir: "s2_one_small",
    },
    Scenario {
        name: "s3_one_medium",
        dir: "s3_one_medium",
    },
    Scenario {
        name: "s4_one_large",
        dir: "s4_one_large",
    },
    Scenario {
        name: "s5_many_small",
        dir: "s5_many_small",
    },
    Scenario {
        name: "s6_deep_chain",
        dir: "s6_deep_chain",
    },
    Scenario {
        name: "s7_realistic",
        dir: "s7_realistic",
    },
];

fn bench_resolve(c: &mut Criterion) {
    let root = corpus_root();
    let mut group = c.benchmark_group("include_resolve");
    group.measurement_time(std::time::Duration::from_secs(10));
    group.warm_up_time(std::time::Duration::from_secs(3));

    for sc in SCENARIOS {
        let scenario_root = root.join(sc.dir);
        let host_path = scenario_root.join("host.lex");
        let source = std::fs::read_to_string(&host_path).unwrap_or_else(|e| {
            panic!(
                "missing fixture {}: {e}\nRun: python3 {}/{}",
                host_path.display(),
                env!("CARGO_MANIFEST_DIR"),
                GEN_PY_REL,
            )
        });
        let loader = Arc::new(FsLoader::new(scenario_root.clone()));
        let config = ResolveConfig::with_root(scenario_root.clone());
        let registry = Registry::new();
        builtins::register_into(&registry, loader, config.clone())
            .expect("register lex.* built-ins");

        group.bench_function(sc.name, |b| {
            b.iter(|| {
                let doc = resolve_from_source(
                    black_box(&source),
                    black_box(Some(host_path.clone())),
                    black_box(&config),
                    black_box(&registry),
                )
                .expect("resolve must succeed");
                black_box(doc);
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_resolve);
criterion_main!(benches);
