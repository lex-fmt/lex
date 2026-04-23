use lex_core::lex::testing::lexplore::Lexplore;

fn assert_non_empty(doc: &lex_core::lex::ast::Document, label: &str) {
    assert!(
        !doc.root.children.is_empty(),
        "{label} should produce items"
    );
}

#[test]
fn verbatim_dual_01() {
    let doc = Lexplore::verbatim(1).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_01");
}

#[test]
fn verbatim_dual_02() {
    let doc = Lexplore::verbatim(2).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_02");
}

#[test]
fn verbatim_dual_03() {
    let doc = Lexplore::verbatim(3).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_03");
}

#[test]
fn verbatim_dual_04() {
    let doc = Lexplore::verbatim(4).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_04");
}

#[test]
fn verbatim_dual_05() {
    let doc = Lexplore::verbatim(5).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_05");
}

#[test]
fn verbatim_dual_06() {
    let doc = Lexplore::verbatim(6).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_06");
}

#[test]
fn verbatim_dual_07() {
    let doc = Lexplore::verbatim(7).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_07");
}

#[test]
fn verbatim_dual_08() {
    let doc = Lexplore::verbatim(8).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_08");
}

#[test]
fn verbatim_dual_09() {
    let doc = Lexplore::verbatim(9).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_09");
}

#[test]
fn verbatim_dual_10() {
    let doc = Lexplore::verbatim(10).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_10");
}

#[test]
fn verbatim_dual_11() {
    let doc = Lexplore::verbatim(11).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_11");
}

#[test]
fn verbatim_dual_12() {
    let doc = Lexplore::verbatim(12).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_12");
}

#[test]
fn verbatim_dual_13() {
    let doc = Lexplore::verbatim(13).parse().unwrap();
    assert_non_empty(&doc, "verbatim_dual_13");
}
