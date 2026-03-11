//! Tokenization tests for individual lex elements using Lexplore
//!
//! These tests verify that the lexer correctly tokenizes individual element types.
//! Each test loads an element from the Lexplore test library and verifies its
//! tokenization using snapshot testing.
//!
//! This replaces the old approach of loading sample files directly, providing:
//! - Better granularity (element-by-element testing)
//! - Centralized test corpus management via Lexplore
//! - Clearer test intent and easier debugging

use lex_core::lex::testing::lexplore::Lexplore;

// ===== Paragraph Tokenization Tests =====

#[test]
fn test_paragraph_flat_oneline() {
    let tokens = Lexplore::paragraph(1).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_paragraph_flat_multiline() {
    let tokens = Lexplore::paragraph(2).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_paragraph_flat_punctuation() {
    let tokens = Lexplore::paragraph(3).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_paragraph_nested_in_session() {
    let tokens = Lexplore::paragraph(4).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_paragraph_nested_in_list() {
    let tokens = Lexplore::paragraph(5).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

// ===== List Tokenization Tests =====

#[test]
fn test_list_flat_simple_dash() {
    let tokens = Lexplore::list(1).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_list_flat_simple_numbered() {
    let tokens = Lexplore::list(2).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_list_flat_simple_lettered() {
    let tokens = Lexplore::list(3).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_list_flat_simple_parenthetical() {
    let tokens = Lexplore::list(4).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_list_flat_mixed_decorations() {
    let tokens = Lexplore::list(5).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_list_nested_simple() {
    let tokens = Lexplore::list(6).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_list_nested_multiple_levels() {
    let tokens = Lexplore::list(7).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_list_nested_mixed_content() {
    let tokens = Lexplore::list(8).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

// ===== Session Tokenization Tests =====

#[test]
fn test_session_flat_numbered() {
    let tokens = Lexplore::session(1).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_session_flat_unnumbered() {
    let tokens = Lexplore::session(2).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_session_nested_one_level() {
    let tokens = Lexplore::session(3).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_session_nested_multiple_levels() {
    let tokens = Lexplore::session(4).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

// ===== Definition Tokenization Tests =====

#[test]
fn test_definition_flat_simple() {
    let tokens = Lexplore::definition(1).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_definition_flat_multiline_label() {
    let tokens = Lexplore::definition(2).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_definition_nested_in_session() {
    let tokens = Lexplore::definition(3).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

// ===== Annotation Tokenization Tests =====

#[test]
fn test_annotation_flat_marker_simple() {
    let tokens = Lexplore::annotation(1).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_annotation_flat_marker_with_params() {
    let tokens = Lexplore::annotation(2).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_annotation_flat_marker_with_label() {
    let tokens = Lexplore::annotation(3).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_annotation_flat_marker_with_label_and_params() {
    let tokens = Lexplore::annotation(4).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_annotation_flat_block_simple() {
    let tokens = Lexplore::annotation(5).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_annotation_nested_in_session() {
    let tokens = Lexplore::annotation(6).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

// ===== Verbatim Block Tokenization Tests =====

#[test]
fn test_verbatim_flat_simple() {
    let tokens = Lexplore::verbatim(1).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_verbatim_flat_with_label() {
    let tokens = Lexplore::verbatim(2).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_verbatim_flat_marker_no_content() {
    let tokens = Lexplore::verbatim(3).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}

#[test]
fn test_verbatim_nested_in_session() {
    let tokens = Lexplore::verbatim(4).tokenize().unwrap();
    insta::assert_debug_snapshot!(tokens);
}
