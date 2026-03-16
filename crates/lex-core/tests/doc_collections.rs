use lex_core::lex::testing::assert_ast;
use lex_core::lex::testing::lexplore::Lexplore;

// ============================================================================
// Trifecta documents
// ============================================================================

#[test]
fn trifecta_000_paragraphs() {
    let doc = Lexplore::trifecta(0).parse().unwrap();
    assert_ast(&doc).item_count(6);
}

#[test]
fn trifecta_010_paragraphs_sessions_flat_single() {
    let doc = Lexplore::trifecta(10).parse().unwrap();
    assert_ast(&doc).item_count(5);
}

#[test]
fn trifecta_020_paragraphs_sessions_flat_multiple() {
    let doc = Lexplore::trifecta(20).parse().unwrap();
    assert_ast(&doc).item_count(8);
}

#[test]
fn trifecta_030_paragraphs_sessions_nested_multiple() {
    let doc = Lexplore::trifecta(30).parse().unwrap();
    assert_ast(&doc).item_count(4);
}

#[test]
fn trifecta_040_lists() {
    let doc = Lexplore::trifecta(40).parse().unwrap();
    assert_ast(&doc).item_count(15);
}

#[test]
fn trifecta_050_paragraph_lists() {
    let doc = Lexplore::trifecta(50).parse().unwrap();
    assert_ast(&doc).item_count(24);
}

#[test]
fn trifecta_051_definitions_no_blank() {
    let doc = Lexplore::trifecta(51).parse().unwrap();
    assert_ast(&doc).item_count(8);
}

#[test]
fn trifecta_060_trifecta_nesting() {
    let doc = Lexplore::trifecta(60).parse().unwrap();
    assert_ast(&doc).item_count(4);
}

#[test]
fn trifecta_070_trifecta_flat_simple() {
    let doc = Lexplore::trifecta(70).parse().unwrap();
    assert_ast(&doc).item_count(8);
}

// ============================================================================
// Benchmark documents
// ============================================================================

#[test]
fn benchmark_000_empty() {
    let doc = Lexplore::benchmark(0).parse().unwrap();
    assert_ast(&doc).item_count(0);
}

#[test]
fn benchmark_010_kitchensink() {
    let doc = Lexplore::benchmark(10).parse().unwrap();
    assert_ast(&doc).item_count(7);
}

#[test]
fn benchmark_020_ideas_naked() {
    let doc = Lexplore::benchmark(20).parse().unwrap();
    assert_ast(&doc).item_count(6);
}

#[test]
fn benchmark_030_a_place_for_ideas() {
    let doc = Lexplore::benchmark(30).parse().unwrap();
    assert_ast(&doc).item_count(4);
}

#[test]
fn benchmark_040_on_parsing() {
    let doc = Lexplore::benchmark(40).parse().unwrap();
    assert_ast(&doc).item_count(10);
}

#[test]
fn benchmark_050_lsp_fixture() {
    let doc = Lexplore::benchmark(50).parse().unwrap();
    assert_ast(&doc).item_count(3);
}

#[test]
fn benchmark_060_injection_multilang() {
    let doc = Lexplore::benchmark(60).parse().unwrap();
    assert_ast(&doc).item_count(7);
}

#[test]
fn benchmark_070_semantic_tokens() {
    let doc = Lexplore::benchmark(70).parse().unwrap();
    assert_ast(&doc).item_count(4);
}

#[test]
fn benchmark_080_gentle_introduction() {
    let doc = Lexplore::benchmark(80).parse().unwrap();
    assert_ast(&doc).item_count(16);
}

// ============================================================================
// Discovery: ensure all spec files are covered by tests above
// ============================================================================

#[test]
fn all_trifecta_files_covered() {
    let numbers =
        lex_core::lex::testing::lexplore::specfile_finder::list_available_numbers("trifecta", None)
            .unwrap();
    let tested: Vec<usize> = vec![0, 10, 20, 30, 40, 50, 51, 60, 70];
    for n in &numbers {
        assert!(
            tested.contains(n),
            "Trifecta file #{n:03} exists but has no test — add one above"
        );
    }
}

#[test]
fn all_benchmark_files_covered() {
    let numbers = lex_core::lex::testing::lexplore::specfile_finder::list_available_numbers(
        "benchmark",
        None,
    )
    .unwrap();
    let tested: Vec<usize> = vec![0, 10, 20, 30, 40, 50, 60, 70, 80];
    for n in &numbers {
        assert!(
            tested.contains(n),
            "Benchmark file #{n:03} exists but has no test — add one above"
        );
    }
}
