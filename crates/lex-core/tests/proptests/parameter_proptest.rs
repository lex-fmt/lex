//! Property-based tests for parameter parsing
//!
//! These tests ensure that parameter parsing is robust and handles
//! various valid inputs correctly according to the simplified grammar:
//! - Parameters must have key=value format (no boolean shorthand)
//! - Parameters are separated by commas only (not whitespace)
//! - Whitespace around parameters is ignored

use lex_core::lex::assembling::AttachRoot;
use lex_core::lex::escape::escape_quoted;
use lex_core::lex::parsing::engine::parse_from_flat_tokens;
use lex_core::lex::parsing::{parse_document, ContentItem, Document};
use lex_core::lex::testing::assert_ast;
use lex_core::lex::transforms::standard::LEXING;
use lex_core::lex::transforms::Runnable;
use proptest::prelude::*;

fn parse_annotation_without_attachment(source: &str) -> Result<Document, String> {
    let source = if !source.is_empty() && !source.ends_with('\n') {
        format!("{source}\n")
    } else {
        source.to_string()
    };
    let tokens = LEXING.run(source.clone()).map_err(|e| e.to_string())?;
    let root = parse_from_flat_tokens(tokens, &source)?;
    AttachRoot::new().run(root).map_err(|e| e.to_string())
}

/// Generate valid parameter keys
fn parameter_key_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Simple keys
        "[a-z][a-z0-9_-]{0,10}",
        // Keys with underscores
        "[a-z][a-z0-9_]{1,10}",
        // Keys with dashes
        "[a-z][a-z0-9-]{1,10}",
        // Mixed
        "[a-z][a-z0-9_-]{2,10}",
    ]
}

/// Generate valid unquoted parameter values
fn unquoted_value_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Simple alphanumeric values
        "[a-zA-Z0-9]+",
        // Values with dashes
        "[a-zA-Z0-9-]+",
        // Values with periods (for versions)
        "[0-9]+\\.[0-9]+",
        "[0-9]+\\.[0-9]+\\.[0-9]+",
    ]
}

/// Generate valid quoted parameter values
/// Note: We avoid commas and whitespace-only values for simplicity in testing
fn quoted_value_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Simple text with spaces (at least one non-space character)
        "[a-zA-Z0-9][a-zA-Z0-9 ]{0,19}",
        // Text with punctuation (no commas, at least one non-space)
        "[a-zA-Z0-9][a-zA-Z0-9 .-]{0,19}",
        // Simple alphanumeric text
        "[a-zA-Z0-9]{1,10}",
    ]
}

/// Generate semantic content that may contain quotes and backslashes.
/// The returned string is the plain text content (before escaping for embedding in source).
fn escaped_quoted_content_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Content with embedded quotes
        "[a-zA-Z]{1,5} \"[a-zA-Z]{1,5}\"",
        // Content with backslashes
        "[a-zA-Z]{1,5}\\\\[a-zA-Z]{1,5}",
        // Content with both
        "[a-zA-Z]{1,3}\\\\[a-zA-Z]{1,3} \"[a-zA-Z]{1,3}\"",
        // Simple content (no escapes needed)
        "[a-zA-Z0-9 ]{1,15}",
    ]
}

/// Generate a single valid parameter (key=value format only)
fn parameter_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Unquoted values
        (parameter_key_strategy(), unquoted_value_strategy()).prop_map(|(k, v)| format!("{k}={v}")),
        // Quoted values
        (parameter_key_strategy(), quoted_value_strategy())
            .prop_map(|(k, v)| format!("{k}=\"{v}\"")),
    ]
}

/// Generate valid parameter lists (comma-separated)
fn parameter_list_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(parameter_strategy(), 1..5).prop_map(|params| params.join(","))
}

#[cfg(test)]
mod proptest_tests {
    use super::*;

    // @audit: hardcoded_source
    proptest! {
        // Reduce cases to speed up slow tests
        #![proptest_config(ProptestConfig::with_cases(50))]

        // @audit: hardcoded_source
        #[test]
        fn test_single_parameter_parsing(param in parameter_strategy()) {
            let source = format!(":: note {param} ::\n\nText. {{{{paragraph}}}}\n");
            let result = parse_annotation_without_attachment(&source);

            // Should parse successfully
            prop_assert!(result.is_ok(), "Failed to parse: {}", source);

            if let Ok(doc) = result {
                let annotation = doc.root.children[0].as_annotation().unwrap();
                prop_assert_eq!(annotation.data.parameters.len(), 1);

                // Extract key and value from the parameter string
                let parts: Vec<&str> = param.splitn(2, '=').collect();
                prop_assert_eq!(&annotation.data.parameters[0].key, parts[0]);
            }
        }

        // @audit: hardcoded_source
        #[test]
        fn test_multiple_parameters_parsing(params in parameter_list_strategy()) {
            let source = format!(":: note {params} ::\n\nText. {{{{paragraph}}}}\n");
            let result = parse_annotation_without_attachment(&source);

            // Should parse successfully
            prop_assert!(result.is_ok(), "Failed to parse: {}", source);

            if let Ok(doc) = result {
                let annotation = doc.root.children[0].as_annotation().unwrap();
                let expected_count = params.split(',').count();
                prop_assert_eq!(annotation.data.parameters.len(), expected_count);
            }
        }

        // @audit: hardcoded_source
        #[test]
        fn test_parameter_key_preservation(key in parameter_key_strategy(), value in unquoted_value_strategy()) {
            let source = format!(":: note {key}={value} ::\n\nText. {{{{paragraph}}}}\n");
            let result = parse_annotation_without_attachment(&source);

            prop_assert!(result.is_ok(), "Failed to parse: {}", source);

            if let Ok(doc) = result {
                let annotation = doc.root.children[0].as_annotation().unwrap();
                prop_assert_eq!(&annotation.data.parameters[0].key, &key);
                prop_assert_eq!(&annotation.data.parameters[0].value, &value);
            }
        }

        // @audit: hardcoded_source
        #[test]
        fn test_quoted_value_preservation(key in parameter_key_strategy(), value in quoted_value_strategy()) {
            let source = format!(":: note {key}=\"{value}\" ::\n\nText. {{{{paragraph}}}}\n");
            let result = parse_annotation_without_attachment(&source);

            prop_assert!(result.is_ok(), "Failed to parse: {}", source);

            if let Ok(doc) = result {
                let annotation = doc.root.children[0].as_annotation().unwrap();
                prop_assert_eq!(&annotation.data.parameters[0].key, &key);
                // Quotes are preserved in the value
                let expected_value = format!("\"{value}\"");
                prop_assert_eq!(&annotation.data.parameters[0].value, &expected_value);
            }
        }

        #[test]
        fn test_parameter_order_preservation(params in parameter_list_strategy()) {
            let source = format!(":: note {params} ::\n\nText. {{{{paragraph}}}}\n");
            let result = parse_annotation_without_attachment(&source);

            prop_assert!(result.is_ok(), "Failed to parse: {}", source);

            if let Ok(doc) = result {
                let annotation = doc.root.children[0].as_annotation().unwrap();

                // Extract keys from the parameter string
                let expected_keys: Vec<&str> = params
                    .split(',')
                    .map(|p| p.split('=').next().unwrap())
                    .collect();

                let actual_keys: Vec<&str> = annotation.data.parameters
                    .iter()
                    .map(|p| p.key.as_str())
                    .collect();

                prop_assert_eq!(actual_keys, expected_keys);
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        #[test]
        fn test_escaped_quoted_value_roundtrip(
            key in parameter_key_strategy(),
            content in escaped_quoted_content_strategy()
        ) {
            // Escape content for embedding in source (\" and \\)
            let escaped_content = escape_quoted(&content);
            let source = format!(":: note {key}=\"{escaped_content}\" ::\n\nText. {{{{paragraph}}}}\n");
            let result = parse_annotation_without_attachment(&source);

            prop_assert!(result.is_ok(), "Failed to parse: {}", source);

            if let Ok(doc) = result {
                let annotation = doc.root.children[0].as_annotation().unwrap();
                prop_assert_eq!(annotation.data.parameters.len(), 1);
                prop_assert_eq!(&annotation.data.parameters[0].key, &key);
                // Verify unquoted_value recovers the original content
                let recovered = annotation.data.parameters[0].unquoted_value();
                prop_assert_eq!(&recovered, &content,
                    "roundtrip failed: content={:?}, escaped={:?}, stored={:?}",
                    content, escaped_content, annotation.data.parameters[0].value);
            }
        }
    }
}

#[test]
fn test_parameter_only_header_is_not_annotation() {
    let source = ":: severity=high ::\n\nBody. {{paragraph}}\n";
    let doc = parse_document(source).expect("parser should not fail on invalid annotations");

    assert!(doc
        .root
        .children
        .iter()
        .all(|item| !matches!(item, ContentItem::Annotation(_))));
}

#[cfg(test)]
mod specific_tests {
    use super::*;

    #[test]
    fn test_comma_only_separator() {
        let source = ":: note key1=val1,key2=val2,key3=val3 ::\n\nText. {{paragraph}}\n";
        let result = parse_annotation_without_attachment(source);
        assert!(result.is_ok());

        let doc = result.unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(3)
                .has_parameter_with_value("key1", "val1")
                .has_parameter_with_value("key2", "val2")
                .has_parameter_with_value("key3", "val3");
        });
    }

    #[test]
    fn test_whitespace_around_commas_ignored() {
        let source = ":: note key1=val1 , key2=val2 , key3=val3 ::\n\nText. {{paragraph}}\n";
        let result = parse_annotation_without_attachment(source);
        assert!(result.is_ok());

        let doc = result.unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(3)
                .parameter(0, "key1", "val1")
                .parameter(1, "key2", "val2")
                .parameter(2, "key3", "val3");
        });
    }

    #[test]
    fn test_whitespace_around_equals_ignored() {
        let source = ":: note key1 = val1 , key2 = val2 ::\n\nText. {{paragraph}}\n";
        let result = parse_annotation_without_attachment(source);
        assert!(result.is_ok());

        let doc = result.unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(2)
                .has_parameter_with_value("key1", "val1")
                .has_parameter_with_value("key2", "val2");
        });
    }

    #[test]
    fn test_quoted_values_with_spaces() {
        let source = ":: note message=\"Hello World\" ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(1)
                .has_parameter_with_value("message", "\"Hello World\"");
        });
    }

    #[test]
    fn test_quoted_values_with_commas() {
        let source = ":: note message=\"value with, comma\" ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(1)
                .has_parameter_with_value("message", "\"value with, comma\"");
        });
    }

    #[test]
    fn test_empty_quoted_value() {
        let source = ":: note message=\"\" ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(1)
                .has_parameter_with_value("message", "\"\"");
        });
    }

    #[test]
    fn test_version_number_values() {
        let source = ":: note version=3.11.2 ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(1)
                .has_parameter_with_value("version", "3.11.2");
        });
    }

    #[test]
    fn test_keys_with_dashes_and_underscores() {
        let source = ":: note ref-id=123,api_version=2 ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(2)
                .parameter(0, "ref-id", "123")
                .parameter(1, "api_version", "2");
        });
    }

    #[test]
    fn test_escaped_quote_in_quoted_value() {
        let source = ":: note message=\"say \\\"hello\\\"\" ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(1)
                .has_parameter_with_value("message", "\"say \\\"hello\\\"\"");
        });
        // Verify unquoted_value resolves escapes
        let annotation = doc.root.children[0].as_annotation().unwrap();
        assert_eq!(
            annotation.data.parameters[0].unquoted_value(),
            "say \"hello\""
        );
    }

    #[test]
    fn test_escaped_backslash_in_quoted_value() {
        let source = ":: note path=\"C:\\\\Users\\\\name\" ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(1)
                .has_parameter_with_value("path", "\"C:\\\\Users\\\\name\"");
        });
        let annotation = doc.root.children[0].as_annotation().unwrap();
        assert_eq!(
            annotation.data.parameters[0].unquoted_value(),
            "C:\\Users\\name"
        );
    }

    #[test]
    fn test_escaped_backslash_before_closing_quote() {
        // \\" = escaped backslash then real closing quote
        let source = ":: note trail=\"end\\\\\" ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(1)
                .has_parameter_with_value("trail", "\"end\\\\\"");
        });
        let annotation = doc.root.children[0].as_annotation().unwrap();
        assert_eq!(annotation.data.parameters[0].unquoted_value(), "end\\");
    }

    #[test]
    fn test_unquoted_value_has_no_escaping() {
        let source = ":: note key=simple ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        let annotation = doc.root.children[0].as_annotation().unwrap();
        assert_eq!(annotation.data.parameters[0].unquoted_value(), "simple");
    }

    #[test]
    fn test_quoted_value_unquoted_value_strips_quotes() {
        let source = ":: note message=\"Hello World\" ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        let annotation = doc.root.children[0].as_annotation().unwrap();
        assert_eq!(
            annotation.data.parameters[0].unquoted_value(),
            "Hello World"
        );
    }

    #[test]
    fn test_lex_marker_inside_quoted_value() {
        let source = ":: note foo=\":: jane\" ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(1)
                .has_parameter_with_value("foo", "\":: jane\"");
        });
    }

    #[test]
    fn test_multiple_lex_markers_inside_quoted_value() {
        let source = ":: note msg=\"a :: b :: c\" ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(1)
                .has_parameter_with_value("msg", "\"a :: b :: c\"");
        });
    }

    #[test]
    fn test_single_colon_inside_quoted_value() {
        let source = ":: note title=\"Chapter: Introduction\" ::\n\nText. {{paragraph}}\n";
        let doc = parse_annotation_without_attachment(source).unwrap();
        assert_ast(&doc).item(0, |item| {
            item.assert_annotation()
                .label("note")
                .parameter_count(1)
                .has_parameter_with_value("title", "\"Chapter: Introduction\"");
        });
    }
}
