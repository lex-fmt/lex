use lex_core::lex::testing::lexplore::Lexplore;

fn assert_not_empty(doc: &lex_core::lex::ast::Document, label: &str) {
    assert!(!doc.root.children.is_empty(), "{label} should have content");
}

#[test]
fn trifecta_000_paragraphs() {
    let doc = Lexplore::trifecta(0).parse().unwrap();
    assert_not_empty(&doc, "trifecta_000_paragraphs");
}

#[test]
fn trifecta_010_paragraphs_sessions_flat_single() {
    let doc = Lexplore::trifecta(10).parse().unwrap();
    assert_not_empty(&doc, "trifecta_010_paragraphs_sessions_flat_single");
}

#[test]
fn trifecta_020_paragraphs_sessions_flat_multiple() {
    let doc = Lexplore::trifecta(20).parse().unwrap();
    assert_not_empty(&doc, "trifecta_020_paragraphs_sessions_flat_multiple");
}

#[test]
fn trifecta_030_paragraphs_sessions_nested_multiple() {
    let doc = Lexplore::trifecta(30).parse().unwrap();
    assert_not_empty(&doc, "trifecta_030_paragraphs_sessions_nested_multiple");
}

#[test]
fn trifecta_040_lists() {
    let doc = Lexplore::trifecta(40).parse().unwrap();
    assert_not_empty(&doc, "trifecta_040_lists");
}

#[test]
fn trifecta_050_paragraph_lists() {
    let doc = Lexplore::trifecta(50).parse().unwrap();
    assert_not_empty(&doc, "trifecta_050_paragraph_lists");
}

#[test]
fn trifecta_060_trifecta_nesting() {
    let doc = Lexplore::trifecta(60).parse().unwrap();
    assert_not_empty(&doc, "trifecta_060_trifecta_nesting");
}

#[test]
fn trifecta_070_trifecta_flat_simple() {
    let doc = Lexplore::trifecta(70).parse().unwrap();
    assert_not_empty(&doc, "trifecta_070_trifecta_flat_simple");
}

#[test]
fn benchmark_010_kitchensink() {
    let doc = Lexplore::benchmark(10).parse().unwrap();
    assert_not_empty(&doc, "benchmark_010_kitchensink");
}
