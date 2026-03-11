//! Tokenization tests for complete lex documents using Lexplore
//!
//! These tests verify that the lexer correctly tokenizes complete documents,
//! including structural interactions between elements. This includes:
//! - Trifecta documents (sessions, paragraphs, lists in various combinations)
//! - Benchmark documents (comprehensive "kitchensink" tests)
//!
//! These tests complement the element-level tests by ensuring that element
//! interactions and document structure are properly tokenized.

use lex_core::lex::testing::lexplore::Lexplore;

// ===== Trifecta Document Tokenization Tests =====
// Trifecta tests focus on the core structural elements (sessions, paragraphs, lists)
// and their interactions, which are the most complex tokenization scenarios.
//
// Files in specs/v1/trifecta/:
// - 000-paragraphs.lex
// - 010-paragraphs-sessions-flat-single.lex
// - 020-paragraphs-sessions-flat-multiple.lex
// - 030-paragraphs-sessions-nested-multiple.lex
// - 040-lists.lex
// - 050-paragraph-lists.lex
// - 070-trifecta-flat-simple.lex (renamed from 050 to avoid duplicate)
// - 060-trifecta-nesting.lex

#[test]
fn test_trifecta_000_paragraphs() {
    let tokens = Lexplore::trifecta(0).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_trifecta_010_paragraphs_sessions_flat_single() {
    let tokens = Lexplore::trifecta(10).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_trifecta_020_paragraphs_sessions_flat_multiple() {
    let tokens = Lexplore::trifecta(20).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_trifecta_030_sessions_nested_multiple() {
    let tokens = Lexplore::trifecta(30).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_trifecta_040_lists() {
    let tokens = Lexplore::trifecta(40).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_trifecta_050_paragraph_lists() {
    let tokens = Lexplore::trifecta(50).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_trifecta_070_flat_simple() {
    // Renamed from 050 to 070 to avoid duplicate numbers
    let tokens = Lexplore::trifecta(70).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_trifecta_060_nesting() {
    let tokens = Lexplore::trifecta(60).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

// ===== Benchmark Document Tokenization Tests =====
// Benchmark tests are comprehensive smoke tests that include all elements
// in their variations. These ensure overall tokenizer compliance.

#[test]
fn test_benchmark_010_kitchensink() {
    let tokens = Lexplore::benchmark(10).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}
