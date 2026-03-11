//! Data node assertions

use crate::lex::ast::elements::data::Data;

pub struct DataAssertion<'a> {
    pub(crate) data: &'a Data,
    pub(crate) context: String,
}

impl<'a> DataAssertion<'a> {
    pub fn label(self, expected: &str) -> Self {
        let actual = &self.data.label.value;
        assert_eq!(
            actual, expected,
            "{}: Expected data label '{}', got '{}'",
            self.context, expected, actual
        );
        self
    }

    pub fn parameter_count(self, expected: usize) -> Self {
        let actual = self.data.parameters.len();
        assert_eq!(
            actual, expected,
            "{}: Expected {} parameters, found {}",
            self.context, expected, actual
        );
        self
    }
}
