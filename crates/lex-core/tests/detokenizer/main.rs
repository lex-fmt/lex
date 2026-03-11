use lex_core::lex::formats::detokenizer::detokenize;
use lex_core::lex::lexing::{lex, tokenize};
use lex_core::lex::testing::lexplore::Lexplore;

mod lexplore_tests {
    use super::*;

    #[test]
    fn detokenize_paragraph_raw() {
        let source = Lexplore::paragraph(1).source();
        let tokens: Vec<_> = tokenize(&source).into_iter().map(|(t, _)| t).collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }

    #[test]
    fn detokenize_list_raw() {
        let source = Lexplore::list(1).source();
        let tokens: Vec<_> = tokenize(&source).into_iter().map(|(t, _)| t).collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }

    #[test]
    fn detokenize_list_semantic() {
        let source = Lexplore::list(2).source();
        let raw_tokens = tokenize(&source);
        let tokens: Vec<_> = lex(raw_tokens)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }

    #[test]
    fn detokenize_session_raw() {
        let source = Lexplore::session(1).source();
        let tokens: Vec<_> = tokenize(&source).into_iter().map(|(t, _)| t).collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }

    #[test]
    fn detokenize_session_semantic() {
        let source = Lexplore::session(1).source();
        let raw_tokens = tokenize(&source);
        let tokens: Vec<_> = lex(raw_tokens)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }

    #[test]
    fn detokenize_verbatim_raw() {
        let source = Lexplore::verbatim(1).source();
        let tokens: Vec<_> = tokenize(&source).into_iter().map(|(t, _)| t).collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }

    #[test]
    fn detokenize_annotation_raw() {
        let source = Lexplore::annotation(1).source();
        let tokens: Vec<_> = tokenize(&source).into_iter().map(|(t, _)| t).collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }

    #[test]
    fn detokenize_definition_raw() {
        let source = Lexplore::definition(1).source();
        let tokens: Vec<_> = tokenize(&source).into_iter().map(|(t, _)| t).collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }

    #[test]
    fn detokenize_definition_semantic() {
        let source = Lexplore::definition(1).source();
        let raw_tokens = tokenize(&source);
        let tokens: Vec<_> = lex(raw_tokens)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }
}

mod lexplore_document_tests {
    use super::*;

    #[test]
    fn detokenize_trifecta_000_raw() {
        let source = Lexplore::trifecta(0).source();
        let tokens: Vec<_> = tokenize(&source).into_iter().map(|(t, _)| t).collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }

    #[test]
    fn detokenize_trifecta_060_semantic() {
        let source = Lexplore::trifecta(60).source();
        let raw_tokens = tokenize(&source);
        let tokens: Vec<_> = lex(raw_tokens)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }

    #[test]
    fn detokenize_benchmark_010_semantic() {
        let source = Lexplore::benchmark(10).source();
        let raw_tokens = tokenize(&source);
        let tokens: Vec<_> = lex(raw_tokens)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        let detokenized = detokenize(&tokens);
        insta::assert_snapshot!(detokenized);
    }
}
