//! Annotation assertions

use super::{data::DataAssertion, summarize_items, visible_len, visible_nth};
use crate::lex::ast::traits::Container;
use crate::lex::ast::Annotation;
use crate::lex::testing::ast_assertions::ContentItemAssertion;

pub struct AnnotationAssertion<'a> {
    pub(crate) annotation: &'a Annotation,
    pub(crate) context: String,
}

impl<'a> AnnotationAssertion<'a> {
    pub fn label(self, expected: &str) -> Self {
        let actual = &self.annotation.data.label.value;
        assert_eq!(
            actual, expected,
            "{}: Expected annotation label to be '{}', but got '{}'",
            self.context, expected, actual
        );
        self
    }
    pub fn data<F>(self, assertion: F) -> Self
    where
        F: FnOnce(DataAssertion<'a>),
    {
        assertion(DataAssertion {
            data: &self.annotation.data,
            context: format!("{}:data", self.context),
        });
        self
    }
    pub fn parameter_count(self, expected: usize) -> Self {
        let actual = self.annotation.data.parameters.len();
        assert_eq!(
            actual, expected,
            "{}: Expected {} parameters, found {} parameters",
            self.context, expected, actual
        );
        self
    }

    /// Assert that a parameter with the given key exists (any value)
    pub fn has_parameter(self, key: &str) -> Self {
        let found = self.annotation.data.parameters.iter().any(|p| p.key == key);
        assert!(
            found,
            "{}: Expected parameter with key '{}' to exist, but found parameters: [{}]",
            self.context,
            key,
            self.annotation
                .data
                .parameters
                .iter()
                .map(|p| format!("{}={}", p.key, p.value))
                .collect::<Vec<_>>()
                .join(", ")
        );
        self
    }

    /// Assert that a parameter with the given key does NOT exist
    pub fn no_parameter(self, key: &str) -> Self {
        let found = self.annotation.data.parameters.iter().any(|p| p.key == key);
        assert!(
            !found,
            "{}: Expected no parameter with key '{}', but found it with value '{}'",
            self.context,
            key,
            self.annotation
                .data
                .parameters
                .iter()
                .find(|p| p.key == key)
                .map(|p| p.value.as_str())
                .unwrap_or("")
        );
        self
    }

    /// Assert that a parameter with the given key has the expected value
    pub fn has_parameter_with_value(self, key: &str, value: &str) -> Self {
        let param = self
            .annotation
            .data
            .parameters
            .iter()
            .find(|p| p.key == key);
        match param {
            Some(p) => {
                assert_eq!(
                    p.value, value,
                    "{}: Expected parameter '{}' to have value '{}', but got '{}'",
                    self.context, key, value, p.value
                );
            }
            None => {
                panic!(
                    "{}: Expected parameter '{}={}' to exist, but parameter '{}' not found. Available parameters: [{}]",
                    self.context,
                    key,
                    value,
                    key,
                        self.annotation
                            .data
                            .parameters
                        .iter()
                        .map(|p| format!("{}={}", p.key, p.value))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        self
    }

    /// Assert on a specific parameter by index
    pub fn parameter(self, index: usize, expected_key: &str, expected_value: &str) -> Self {
        assert!(
            index < self.annotation.data.parameters.len(),
            "{}: Parameter index {} out of bounds (annotation has {} parameters)",
            self.context,
            index,
            self.annotation.data.parameters.len()
        );
        let param = &self.annotation.data.parameters[index];
        assert_eq!(
            param.key, expected_key,
            "{}: Expected parameter[{}].key to be '{}', but got '{}'",
            self.context, index, expected_key, param.key
        );
        assert_eq!(
            param.value, expected_value,
            "{}: Expected parameter[{}].value to be '{}', but got '{}'",
            self.context, index, expected_value, param.value
        );
        self
    }

    /// Assert that parameter at given index has the expected key (any value)
    pub fn parameter_key(self, index: usize, expected_key: &str) -> Self {
        assert!(
            index < self.annotation.data.parameters.len(),
            "{}: Parameter index {} out of bounds (annotation has {} parameters)",
            self.context,
            index,
            self.annotation.data.parameters.len()
        );
        let param = &self.annotation.data.parameters[index];
        assert_eq!(
            param.key, expected_key,
            "{}: Expected parameter[{}].key to be '{}', but got '{}'",
            self.context, index, expected_key, param.key
        );
        self
    }

    pub fn child_count(self, expected: usize) -> Self {
        let actual = visible_len(self.annotation.children());
        assert_eq!(
            actual,
            expected,
            "{}: Expected {} children, found {} children: [{}]",
            self.context,
            expected,
            actual,
            summarize_items(self.annotation.children())
        );
        self
    }
    pub fn child<F>(self, index: usize, assertion: F) -> Self
    where
        F: FnOnce(ContentItemAssertion<'a>),
    {
        let children = self.annotation.children();
        let visible_children = visible_len(children);
        assert!(
            index < visible_children,
            "{}: Child index {} out of bounds (annotation has {} children)",
            self.context,
            index,
            visible_children
        );
        let child =
            visible_nth(children, index).expect("visible child should exist at computed index");
        assertion(ContentItemAssertion {
            item: child,
            context: format!("{}:children[{}]", self.context, index),
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::range::{Position, Range};
    use crate::lex::ast::{Data, Label, Parameter};

    fn create_test_annotation(label: &str, parameters: Vec<(&str, &str)>) -> Annotation {
        let location = Range::new(0..0, Position::new(0, 0), Position::new(0, 10));
        let label = Label::new(label.to_string()).at(location.clone());
        let parameters: Vec<Parameter> = parameters
            .into_iter()
            .map(|(k, v)| Parameter {
                key: k.to_string(),
                value: v.to_string(),
                location: location.clone(),
            })
            .collect();
        let data = Data::new(label, parameters).at(location.clone());
        Annotation::from_data(data, Vec::new()).at(location)
    }

    #[test]
    fn test_annotation_label_assertion() {
        let annotation = create_test_annotation("test", vec![]);
        assert_eq!(annotation.data.label.value, "test");

        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.label("test");
    }

    #[test]
    #[should_panic(expected = "Expected annotation label to be 'wrong'")]
    fn test_annotation_label_assertion_fails() {
        let annotation = create_test_annotation("test", vec![]);
        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.label("wrong");
    }

    #[test]
    fn test_parameter_count_assertion() {
        let annotation = create_test_annotation("test", vec![("key1", "val1"), ("key2", "val2")]);
        assert_eq!(annotation.data.parameters.len(), 2);

        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.parameter_count(2);
    }

    #[test]
    #[should_panic(expected = "Expected 3 parameters, found 2 parameters")]
    fn test_parameter_count_assertion_fails() {
        let annotation = create_test_annotation("test", vec![("key1", "val1"), ("key2", "val2")]);
        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.parameter_count(3);
    }

    #[test]
    fn test_has_parameter_assertion() {
        let annotation = create_test_annotation("test", vec![("foo", "bar"), ("baz", "qux")]);
        assert!(annotation.data.parameters.iter().any(|p| p.key == "foo"));

        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.has_parameter("foo");
    }

    #[test]
    #[should_panic(expected = "Expected parameter with key 'missing'")]
    fn test_has_parameter_assertion_fails() {
        let annotation = create_test_annotation("test", vec![("foo", "bar")]);
        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.has_parameter("missing");
    }

    #[test]
    fn test_no_parameter_assertion() {
        let annotation = create_test_annotation("test", vec![("foo", "bar")]);
        assert!(!annotation
            .data
            .parameters
            .iter()
            .any(|p| p.key == "missing"));

        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.no_parameter("missing");
    }

    #[test]
    #[should_panic(expected = "Expected no parameter with key 'foo'")]
    fn test_no_parameter_assertion_fails() {
        let annotation = create_test_annotation("test", vec![("foo", "bar")]);
        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.no_parameter("foo");
    }

    #[test]
    fn test_has_parameter_with_value_assertion() {
        let annotation = create_test_annotation("test", vec![("key", "value"), ("other", "data")]);
        let param = annotation.data.parameters.iter().find(|p| p.key == "key");
        assert!(param.is_some());
        assert_eq!(param.unwrap().value, "value");

        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.has_parameter_with_value("key", "value");
    }

    #[test]
    #[should_panic(expected = "Expected parameter 'key' to have value 'wrong'")]
    fn test_has_parameter_with_value_assertion_fails_wrong_value() {
        let annotation = create_test_annotation("test", vec![("key", "value")]);
        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.has_parameter_with_value("key", "wrong");
    }

    #[test]
    #[should_panic(expected = "parameter 'missing' not found")]
    fn test_has_parameter_with_value_assertion_fails_missing_key() {
        let annotation = create_test_annotation("test", vec![("key", "value")]);
        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.has_parameter_with_value("missing", "value");
    }

    #[test]
    fn test_parameter_by_index_assertion() {
        let annotation = create_test_annotation("test", vec![("first", "1"), ("second", "2")]);
        assert_eq!(annotation.data.parameters[0].key, "first");
        assert_eq!(annotation.data.parameters[0].value, "1");
        assert_eq!(annotation.data.parameters[1].key, "second");
        assert_eq!(annotation.data.parameters[1].value, "2");

        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion
            .parameter(0, "first", "1")
            .parameter(1, "second", "2");
    }

    #[test]
    #[should_panic(expected = "Parameter index 2 out of bounds")]
    fn test_parameter_by_index_assertion_out_of_bounds() {
        let annotation = create_test_annotation("test", vec![("key", "value")]);
        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.parameter(2, "key", "value");
    }

    #[test]
    #[should_panic(expected = "Expected parameter[0].key to be 'wrong'")]
    fn test_parameter_by_index_assertion_fails_key() {
        let annotation = create_test_annotation("test", vec![("key", "value")]);
        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.parameter(0, "wrong", "value");
    }

    #[test]
    #[should_panic(expected = "Expected parameter[0].value to be 'wrong'")]
    fn test_parameter_by_index_assertion_fails_value() {
        let annotation = create_test_annotation("test", vec![("key", "value")]);
        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion.parameter(0, "key", "wrong");
    }

    #[test]
    fn test_parameter_key_by_index_assertion() {
        let annotation = create_test_annotation("test", vec![("key1", "val1"), ("key2", "val2")]);
        assert_eq!(annotation.data.parameters[0].key, "key1");
        assert_eq!(annotation.data.parameters[1].key, "key2");

        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion
            .parameter_key(0, "key1")
            .parameter_key(1, "key2");
    }

    #[test]
    fn test_fluent_parameter_assertions() {
        let annotation = create_test_annotation(
            "test",
            vec![("foo", "bar"), ("baz", "qux"), ("other", "data")],
        );

        let annotation_assertion = AnnotationAssertion {
            annotation: &annotation,
            context: "test".to_string(),
        };
        annotation_assertion
            .label("test")
            .parameter_count(3)
            .has_parameter("foo")
            .has_parameter("baz")
            .has_parameter_with_value("foo", "bar")
            .has_parameter_with_value("baz", "qux")
            .parameter(0, "foo", "bar")
            .parameter(1, "baz", "qux")
            .no_parameter("missing")
            .child_count(0);
    }
}
